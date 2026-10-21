use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use kuizer::caches::{self, KnownCache, Locations};
use kuizer::cli::SortKey;
use kuizer::commands::{measure_caches_with, sort_candidates};
use kuizer::config::Config;
use kuizer::delete::{self, Guard, Mode, Removal, Status, Target};
use kuizer::detect::{Category, RuleInfo, Safety};
use kuizer::docker;
use kuizer::dupes::{self, Duplicates, Options};
use kuizer::history::restore::{RestoreError, Status as RestoreStatus};
use kuizer::history::{self, Action, Entry, History, ItemState, Recorder};
use kuizer::output::display_path;
use kuizer::platform;
use kuizer::protect::Protected;
use kuizer::repos::{self, Repository};
use kuizer::scan::{self, Cancel, Candidate, Collected, Issue, Progress, Summary};
use kuizer::toolchains::{self, Toolchain};

use crate::activity::{self, Found, Step, Tracker, grouped, plural};
use crate::start::ScanOptions;
use crate::theme;

pub const CANCELLED: &str = "cancelled";

/// One section of a scan that arrives on its own: the parts run side by side and each is
/// shown the moment it is done.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Part {
    Artifacts,
    Caches,
    Toolchains,
    Docker,
    Repos,
    History,
}

impl Part {
    /// @name Part::expected
    /// @description The parts a scan with these options delivers. Parts the options turn off
    /// never arrive, so nothing waits for them.
    ///
    /// @example
    /// let pending = Part::expected(&options);
    pub fn expected(options: &ScanOptions) -> HashSet<Part> {
        [
            (true, Self::Artifacts),
            (options.caches, Self::Caches),
            (options.toolchains, Self::Toolchains),
            (options.docker, Self::Docker),
            (options.repos, Self::Repos),
            (true, Self::History),
        ]
        .into_iter()
        .filter_map(|(on, part)| on.then_some(part))
        .collect()
    }

    pub fn source(self) -> Option<Source> {
        match self {
            Self::Artifacts => Some(Source::Artifact),
            Self::Caches => Some(Source::Cache),
            Self::Toolchains => Some(Source::Toolchain),
            Self::Docker => Some(Source::Docker),
            Self::Repos | Self::History => None,
        }
    }
}

/// What a running scan hands to the window: a batch of artifacts found so far while the folder
/// walk runs, or a finished part that replaces its section of the report.
pub enum Loaded {
    Found(Vec<Candidate>),
    Artifacts {
        candidates: Vec<Candidate>,
        issues: Vec<Issue>,
        summary: Summary,
        disks: Vec<Disk>,
    },
    Caches(Vec<(KnownCache, Candidate)>),
    Toolchains(Vec<(Toolchain, Candidate)>),
    Docker {
        entries: Vec<(docker::Item, Candidate)>,
        issues: Vec<String>,
    },
    Repos(Vec<Repository>),
    History(Result<HistoryView, String>),
}

impl Loaded {
    pub fn part(&self) -> Option<Part> {
        match self {
            Self::Found(_) => None,
            Self::Artifacts { .. } => Some(Part::Artifacts),
            Self::Caches(_) => Some(Part::Caches),
            Self::Toolchains(_) => Some(Part::Toolchains),
            Self::Docker { .. } => Some(Part::Docker),
            Self::Repos(_) => Some(Part::Repos),
            Self::History(_) => Some(Part::History),
        }
    }
}

pub struct Disk {
    pub available: Option<u64>,
    pub total: Option<u64>,
}

pub struct HistoryView {
    pub history: History,
    pub kept: HashSet<(usize, usize)>,
    pub trash_known: bool,
}

impl HistoryView {
    pub fn load() -> Result<Self, String> {
        let path = history::default_path().map_err(|error| error.to_string())?;
        let history = history::load(&path).map_err(|error| error.to_string())?;
        let trashed = history::trashed_paths();
        let kept = trashed
            .as_deref()
            .map(|paths| history::still_in_trash(&history, paths))
            .unwrap_or_default();
        Ok(Self {
            history,
            kept,
            trash_known: trashed.is_some(),
        })
    }

    pub fn freed(&self) -> u64 {
        history::freed_bytes(&self.history, self.trash_known.then_some(&self.kept))
    }

    pub fn in_trash(&self) -> u64 {
        self.kept
            .iter()
            .filter_map(|&(run, entry)| self.history.runs.get(run)?.deleted.get(entry))
            .map(|entry| entry.bytes)
            .sum()
    }

    pub fn delete_runs(&self) -> impl DoubleEndedIterator<Item = &history::Run> {
        self.history
            .runs
            .iter()
            .filter(|run| run.action == Action::Delete)
    }

    #[cfg(test)]
    pub fn trash_state(&self, run: usize) -> TrashState {
        self.state_of(&self.history.restored_ids(), run)
    }

    /// @name HistoryView::trash_states
    /// @description Says for every run, in run order, whether it can still be restored from the
    /// trash, using the library's per item state: what is left in the trash, all of it restored,
    /// gone from the trash, unknown because the trash cannot be read, or never trashed at all.
    /// Reads the restore runs once for all of them.
    ///
    /// @example
    /// if let TrashState::Restorable { items, bytes } = view.trash_states()[run] { .. }
    pub fn trash_states(&self) -> Vec<TrashState> {
        let restored = self.history.restored_ids();
        (0..self.history.runs.len())
            .map(|run| self.state_of(&restored, run))
            .collect()
    }

    fn state_of(&self, restored: &HashSet<&str>, run: usize) -> TrashState {
        let Some(entries) = self.history.runs.get(run).map(|run| run.deleted.len()) else {
            return TrashState::NotTrashed;
        };
        let kept = self.trash_known.then_some(&self.kept);
        let states: Vec<(usize, ItemState)> = (0..entries)
            .map(|entry| {
                let state = history::item_state(&self.history, restored, kept, run, entry);
                (entry, state)
            })
            .filter(|(_, state)| !matches!(state, ItemState::Deleted | ItemState::DryRun))
            .collect();
        if states.is_empty() {
            return TrashState::NotTrashed;
        }
        if !self.trash_known {
            return TrashState::Unknown;
        }
        let left: Vec<usize> = states
            .iter()
            .filter(|(_, state)| *state == ItemState::InTrash)
            .map(|&(entry, _)| entry)
            .collect();
        if !left.is_empty() {
            return TrashState::Restorable {
                items: left.len(),
                bytes: left
                    .iter()
                    .map(|&entry| self.history.runs[run].deleted[entry].bytes)
                    .sum(),
            };
        }
        if states
            .iter()
            .all(|(_, state)| *state == ItemState::Restored)
        {
            TrashState::Restored
        } else {
            TrashState::Emptied
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrashState {
    Restorable { items: usize, bytes: u64 },
    Restored,
    Emptied,
    Unknown,
    NotTrashed,
}

pub struct Report {
    pub roots: Vec<PathBuf>,
    pub options: ScanOptions,
    pub candidates: Vec<Candidate>,
    pub issues: Vec<Issue>,
    pub summary: Summary,
    pub caches: Vec<(KnownCache, Candidate)>,
    pub toolchains: Vec<(Toolchain, Candidate)>,
    pub docker: Vec<(docker::Item, Candidate)>,
    pub docker_issues: Vec<String>,
    pub repos: Vec<Repository>,
    pub duplicates: Option<Duplicates>,
    pub history: Result<HistoryView, String>,
    pub disks: Vec<Disk>,
    pub scanned: SystemTime,
}

pub struct Bucket<K> {
    pub key: K,
    pub bytes: u64,
    pub count: usize,
}

impl Report {
    /// @name Report::empty
    /// @description A report with nothing in it yet, for the parts of a scan to fill in as they
    /// finish.
    ///
    /// @example
    /// let mut report = Report::empty(&roots, &options);
    pub fn empty(roots: &[PathBuf], options: &ScanOptions) -> Self {
        Self {
            roots: roots.to_vec(),
            options: options.clone(),
            candidates: Vec::new(),
            issues: Vec::new(),
            summary: Summary {
                progress: Progress::default(),
                duration: Duration::ZERO,
                cancelled: false,
            },
            caches: Vec::new(),
            toolchains: Vec::new(),
            docker: Vec::new(),
            docker_issues: Vec::new(),
            repos: Vec::new(),
            duplicates: None,
            history: Err(String::new()),
            disks: Vec::new(),
            scanned: SystemTime::now(),
        }
    }

    /// @name Report::apply
    /// @description Stores what a scan delivered. A batch of found artifacts joins the list,
    /// largest first; a finished part replaces its section. Returns what the section held
    /// before, so the caller can drop it where it does not hold up the window.
    ///
    /// @example
    /// let old = report.apply(Loaded::Caches(caches));
    pub fn apply(&mut self, loaded: Loaded) -> Loaded {
        match loaded {
            Loaded::Found(batch) => {
                self.candidates.extend(batch);
                sort_candidates(&mut self.candidates, SortKey::Size);
                Loaded::Found(Vec::new())
            }
            Loaded::Artifacts {
                candidates,
                issues,
                summary,
                disks,
            } => Loaded::Artifacts {
                candidates: std::mem::replace(&mut self.candidates, candidates),
                issues: std::mem::replace(&mut self.issues, issues),
                summary: std::mem::replace(&mut self.summary, summary),
                disks: std::mem::replace(&mut self.disks, disks),
            },
            Loaded::Caches(caches) => Loaded::Caches(std::mem::replace(&mut self.caches, caches)),
            Loaded::Toolchains(toolchains) => {
                Loaded::Toolchains(std::mem::replace(&mut self.toolchains, toolchains))
            }
            Loaded::Docker { entries, issues } => Loaded::Docker {
                entries: std::mem::replace(&mut self.docker, entries),
                issues: std::mem::replace(&mut self.docker_issues, issues),
            },
            Loaded::Repos(repos) => Loaded::Repos(std::mem::replace(&mut self.repos, repos)),
            Loaded::History(history) => {
                Loaded::History(std::mem::replace(&mut self.history, history))
            }
        }
    }

    /// @name Report::into_parts
    /// @description Splits a report into its sections, so a saved scan can fill in only the
    /// sections a fresh scan has not delivered yet.
    ///
    /// @example
    /// for part in saved.into_parts() { report.apply(part); }
    pub fn into_parts(self) -> Vec<Loaded> {
        vec![
            Loaded::Artifacts {
                candidates: self.candidates,
                issues: self.issues,
                summary: self.summary,
                disks: self.disks,
            },
            Loaded::Caches(self.caches),
            Loaded::Toolchains(self.toolchains),
            Loaded::Docker {
                entries: self.docker,
                issues: self.docker_issues,
            },
            Loaded::Repos(self.repos),
            Loaded::History(self.history),
        ]
    }

    /// The removable items of one section of the report.
    pub fn section(&self, source: Source) -> Box<dyn Iterator<Item = &Candidate> + '_> {
        match source {
            Source::Artifact => Box::new(self.candidates.iter()),
            Source::Cache => Box::new(self.caches.iter().map(|(_, candidate)| candidate)),
            Source::Toolchain => Box::new(self.toolchains.iter().map(|(_, candidate)| candidate)),
            Source::Docker => Box::new(self.docker.iter().map(|(_, candidate)| candidate)),
        }
    }

    #[cfg(test)]
    pub fn total(&self, safety: Option<Safety>) -> u64 {
        self.candidates
            .iter()
            .filter(|candidate| safety.is_none_or(|safety| candidate.rule.safety == safety))
            .map(|candidate| candidate.usage.allocated)
            .sum()
    }

    pub fn cache_total(&self, safety: Option<Safety>) -> u64 {
        self.caches
            .iter()
            .filter(|(_, candidate)| safety.is_none_or(|safety| candidate.rule.safety == safety))
            .map(|(_, candidate)| candidate.usage.allocated)
            .sum()
    }

    pub fn toolchain_total(&self, safety: Option<Safety>) -> u64 {
        self.toolchains
            .iter()
            .filter(|(_, candidate)| safety.is_none_or(|safety| candidate.rule.safety == safety))
            .map(|(_, candidate)| candidate.usage.allocated)
            .sum()
    }

    pub fn docker_total(&self, safety: Option<Safety>) -> u64 {
        self.docker
            .iter()
            .filter(|(_, candidate)| safety.is_none_or(|safety| candidate.rule.safety == safety))
            .map(|(_, candidate)| candidate.usage.allocated)
            .sum()
    }

    pub fn by_category(&self) -> Vec<Bucket<Category>> {
        let mut buckets: Vec<Bucket<Category>> = Category::ALL
            .iter()
            .map(|&key| Bucket {
                key,
                bytes: 0,
                count: 0,
            })
            .collect();
        for candidate in &self.candidates {
            if let Some(bucket) = buckets
                .iter_mut()
                .find(|bucket| bucket.key == candidate.rule.category)
            {
                bucket.bytes += candidate.usage.allocated;
                bucket.count += 1;
            }
        }
        buckets.retain(|bucket| bucket.count > 0);
        buckets.sort_by_key(|bucket| std::cmp::Reverse(bucket.bytes));
        buckets
    }

    /// @name Report::drop_paths
    /// @description Removes everything at or below any of `paths` from every list, the way a
    /// scan with those paths excluded would. Duplicate groups left with a single copy are dropped.
    ///
    /// @example
    /// report.drop_paths(&[PathBuf::from("/p/site/node_modules")]);
    pub fn drop_paths(&mut self, paths: &[PathBuf]) {
        if paths.is_empty() {
            return;
        }
        let gone = |path: &Path| paths.iter().any(|excluded| path.starts_with(excluded));
        self.candidates.retain(|candidate| !gone(&candidate.path));
        self.caches.retain(|(_, candidate)| !gone(&candidate.path));
        self.toolchains
            .retain(|(_, candidate)| !gone(&candidate.path));
        self.repos.retain(|repository| !gone(&repository.root));
        self.issues.retain(|issue| !gone(&issue.path));
        if let Some(duplicates) = &mut self.duplicates {
            for group in &mut duplicates.groups {
                group.members.retain(|member| !gone(&member.path));
            }
            duplicates.groups.retain(|group| group.members.len() > 1);
            duplicates
                .groups
                .sort_by_key(|group| std::cmp::Reverse(group.reclaimable()));
        }
    }

    pub fn by_project(&self) -> Vec<Bucket<PathBuf>> {
        let mut projects: BTreeMap<&Path, (u64, usize)> = BTreeMap::new();
        for candidate in &self.candidates {
            let entry = projects
                .entry(candidate.project.as_deref().unwrap_or(&candidate.path))
                .or_default();
            entry.0 += candidate.usage.allocated;
            entry.1 += 1;
        }
        let mut buckets: Vec<Bucket<PathBuf>> = projects
            .into_iter()
            .map(|(key, (bytes, count))| Bucket {
                key: key.to_path_buf(),
                bytes,
                count,
            })
            .collect();
        buckets.sort_by_key(|bucket| std::cmp::Reverse(bucket.bytes));
        buckets
    }
}

/// @name short_path
/// @description Shows a path relative to the scan root that holds it, so long roots are not
/// repeated on every row. Paths under the home directory root keep their `~` form.
///
/// @example
/// assert_eq!(short_path(Path::new("/p/a/dist"), &[PathBuf::from("/p")]), "a/dist");
pub fn short_path(path: &Path, roots: &[PathBuf]) -> String {
    let home = platform::home_dir();
    roots
        .iter()
        .filter(|root| Some(root.as_path()) != home.as_deref())
        .find_map(|root| {
            path.strip_prefix(root)
                .ok()
                .filter(|rest| !rest.as_os_str().is_empty())
        })
        .map_or_else(|| display_path(path), display_path)
}

/// @name load
/// @description Gathers everything the dashboard shows: project artifacts and toolchain pins
/// from one folder walk, global caches, installed toolchains, git repositories, Docker objects,
/// the cleanup history and free disk space.
/// The start screen's options decide which of these run. History, caches, toolchains and Docker
/// start with the walk and run beside it; repositories start when it ends, and toolchains wait
/// for its pins before they are delivered, because a pinned version is never safe to remove.
/// Artifacts go to `emit` in batches while the walk runs, and every part goes to `emit` the
/// moment it is done. Progress goes through `tracker`, and the work stops once `cancel` is set.
///
/// @example
/// let tracker = Tracker::new(|_| {});
/// load(&config, &roots, &options, &Cancel::new(), &tracker, &|loaded| show(loaded))?;
pub fn load(
    config: &Config,
    roots: &[PathBuf],
    options: &ScanOptions,
    cancel: &Cancel,
    tracker: &Tracker,
    emit: &(dyn Fn(Loaded) + Sync),
) -> Result<(), String> {
    let config = &options.apply(config);
    let pin_search = options.toolchains && config.toolchains.pins;
    let started = Instant::now();
    let (handle, walked) = repos::start_scan(config, roots, cancel.clone(), None, pin_search)
        .map_err(|error| error.to_string())?;
    tracker.update_now(|activity| {
        for step in Step::ALL {
            if step.enabled(options) && step != Step::Repos {
                activity.begin(step, None);
            }
        }
    });
    let finished = AtomicBool::new(false);
    let result = thread::scope(|scope| {
        let ticker = spawn_part(scope, "pulse", || {
            while !finished.load(Ordering::Relaxed) {
                thread::park_timeout(Duration::from_millis(100));
                tracker.flush();
            }
        });
        spawn_part(scope, "history", || {
            let history = HistoryView::load();
            tracker.update_now(|activity| {
                activity.finish(
                    Step::History,
                    started.elapsed(),
                    match &history {
                        Ok(view) => plural(view.delete_runs().count(), "cleanup"),
                        Err(_) => "Cannot read it".to_owned(),
                    },
                );
            });
            emit(Loaded::History(history));
        });
        if options.docker {
            spawn_part(scope, "docker", || {
                let listing = docker::list(
                    &config.docker.engines,
                    config.docker.timeout(),
                    config.docker.recent_hours,
                );
                tracker.update_now(|activity| {
                    activity.finish(
                        Step::Docker,
                        started.elapsed(),
                        plural(listing.items.len(), "object"),
                    );
                });
                emit(Loaded::Docker {
                    entries: docker::entries(listing.items),
                    issues: listing.issues,
                });
            });
        }
        if options.caches {
            spawn_part(scope, "caches", || {
                let caches = measure_caches(config, cancel, tracker);
                let bytes = caches.iter().map(|(_, c)| c.usage.allocated).sum();
                tracker.update_now(|activity| {
                    activity.finish(
                        Step::Caches,
                        started.elapsed(),
                        format!("{}, {}", plural(caches.len(), "cache"), theme::bytes(bytes)),
                    );
                });
                emit(Loaded::Caches(caches));
            });
        }
        let toolchains = options.toolchains.then(|| {
            spawn_part(scope, "toolchains", || {
                measure_toolchains(config, cancel, tracker)
            })
        });

        let collected = walk(&handle, tracker, emit);
        let (pins, found_repos) = walked
            .recv()
            .map(|walked| (walked.pins, walked.repositories))
            .unwrap_or_default();
        if cancel.is_cancelled() {
            finished.store(true, Ordering::Relaxed);
            ticker.thread().unpark();
            return Err(CANCELLED.to_owned());
        }
        let mut candidates = collected.candidates;
        sort_candidates(&mut candidates, SortKey::Size);
        let artifact_bytes: u64 = candidates.iter().map(|c| c.usage.allocated).sum();
        let found = candidates.len();
        let dirs = collected.summary.progress.dirs;
        emit(Loaded::Artifacts {
            candidates,
            issues: collected.issues,
            summary: collected.summary,
            disks: disks(roots),
        });
        tracker.update_now(|activity| {
            activity.finish(
                Step::Folders,
                started.elapsed(),
                format!(
                    "{} found, {} in {} folders",
                    plural(found, "item"),
                    theme::bytes(artifact_bytes),
                    grouped(dirs)
                ),
            );
            if options.repos {
                activity.begin(Step::Repos, Some(found_repos.len()));
            }
        });

        if options.repos {
            spawn_part(scope, "repos", move || {
                let checked = found_repos.len();
                let repos = repos::inspect_all_with(found_repos, &config.git, cancel, |root| {
                    tracker.update(|activity| activity.advance(Step::Repos, root));
                });
                tracker.update_now(|activity| {
                    activity.finish(
                        Step::Repos,
                        started.elapsed(),
                        format!(
                            "{} of {} worth cleaning",
                            repos.len(),
                            plural(checked, "repository")
                        ),
                    );
                });
                emit(Loaded::Repos(repos));
            });
        }
        if let Some(measured) = toolchains {
            let mut toolchains = measured.join().unwrap_or_default();
            if let Some(pins) = &pins {
                repin(&mut toolchains, &pins.pins);
            }
            let bytes = toolchains.iter().map(|(_, c)| c.usage.allocated).sum();
            tracker.update_now(|activity| {
                activity.finish(
                    Step::Toolchains,
                    started.elapsed(),
                    format!(
                        "{}, {}",
                        plural(toolchains.len(), "version"),
                        theme::bytes(bytes)
                    ),
                );
            });
            emit(Loaded::Toolchains(toolchains));
        }
        finished.store(true, Ordering::Relaxed);
        ticker.thread().unpark();
        Ok(())
    });
    if cancel.is_cancelled() {
        return Err(CANCELLED.to_owned());
    }
    result
}

fn spawn_part<'scope, T: Send + 'scope>(
    scope: &'scope thread::Scope<'scope, '_>,
    name: &str,
    work: impl FnOnce() -> T + Send + 'scope,
) -> thread::ScopedJoinHandle<'scope, T> {
    thread::Builder::new()
        .name(format!("kuizer-gui-{name}"))
        .spawn_scoped(scope, work)
        .expect("spawn scan part thread")
}

#[derive(Default)]
struct Streamed {
    batch: Vec<Candidate>,
    found: u64,
    bytes: u64,
    largest: Vec<Found>,
}

fn walk(handle: &scan::Scan, tracker: &Tracker, emit: &(dyn Fn(Loaded) + Sync)) -> Collected {
    let streamed = RefCell::new(Streamed::default());
    scan::collect_with(
        handle,
        |progress| {
            let mut streamed = streamed.borrow_mut();
            let (found, bytes) = (streamed.found, streamed.bytes);
            streamed.found = 0;
            streamed.bytes = 0;
            tracker.update(|activity| {
                activity.walked = progress;
                activity.touch(Step::Folders, handle.current());
                activity.merge(found, bytes, &streamed.largest);
            });
            if !streamed.batch.is_empty() {
                emit(Loaded::Found(std::mem::take(&mut streamed.batch)));
            }
        },
        |candidate| {
            let mut streamed = streamed.borrow_mut();
            let bytes = candidate.usage.allocated;
            streamed.batch.push(candidate.clone());
            streamed.found += 1;
            streamed.bytes += bytes;
            if activity::beats(&streamed.largest, bytes) {
                activity::keep(
                    &mut streamed.largest,
                    Found {
                        path: candidate.path.clone(),
                        name: candidate.rule.name.clone(),
                        bytes,
                    },
                );
            }
        },
    )
}

fn disks(roots: &[PathBuf]) -> Vec<Disk> {
    roots
        .iter()
        .map(|path| Disk {
            available: platform::available_space(path),
            total: platform::total_space(path),
        })
        .collect()
}

fn measure_caches(
    config: &Config,
    cancel: &Cancel,
    tracker: &Tracker,
) -> Vec<(KnownCache, Candidate)> {
    let excludes = config.excludes().ok();
    let found: Vec<KnownCache> = Locations::for_user()
        .map(|locations| {
            caches::find(&locations, &Protected::for_user())
                .into_iter()
                .filter(|cache| {
                    excludes
                        .as_ref()
                        .is_none_or(|excludes| !excludes.path(&cache.path))
                })
                .collect()
        })
        .unwrap_or_default();
    tracker.update_now(|activity| activity.begin(Step::Caches, Some(found.len())));
    measure_caches_with(found, cancel, |candidate| {
        tracker.update(|activity| {
            activity.advance(Step::Caches, &candidate.path);
            if candidate.usage.entries > 0 {
                activity.record(
                    &candidate.path,
                    &candidate.rule.name,
                    candidate.usage.allocated,
                );
            }
        });
    })
}

fn measure_toolchains(
    config: &Config,
    cancel: &Cancel,
    tracker: &Tracker,
) -> Vec<(Toolchain, Candidate)> {
    let excludes = config.excludes().ok();
    let found: Vec<Toolchain> = toolchains::Locations::for_user()
        .map(|locations| {
            toolchains::find(&locations, &config.toolchains.tools)
                .into_iter()
                .filter(|toolchain| {
                    excludes
                        .as_ref()
                        .is_none_or(|excludes| !excludes.path(&toolchain.path))
                })
                .collect()
        })
        .unwrap_or_default();
    tracker.update_now(|activity| activity.begin(Step::Toolchains, Some(found.len())));
    toolchains::measure_with(found, cancel, |candidate| {
        tracker.update(|activity| {
            activity.advance(Step::Toolchains, &candidate.path);
            activity.record(
                &candidate.path,
                &candidate.rule.name,
                candidate.usage.allocated,
            );
        });
    })
}

fn repin(entries: &mut [(Toolchain, Candidate)], pins: &[toolchains::Pin]) {
    let mut found: Vec<Toolchain> = entries
        .iter()
        .map(|(toolchain, _)| toolchain.clone())
        .collect();
    toolchains::apply_pins(&mut found, pins);
    for ((toolchain, candidate), pinned) in entries.iter_mut().zip(found) {
        let safety = pinned.safety();
        if candidate.rule.safety != safety {
            candidate.rule = Arc::new(RuleInfo {
                safety,
                ..RuleInfo::clone(&candidate.rule)
            });
        }
        *toolchain = pinned;
    }
}

/// @name find_duplicates
/// @description Finds duplicate files under the roots with the start screen's options laid
/// over the config, on request rather than as part of every scan. Groups are the same as a
/// scan that collects files during its walk would find.
///
/// @example
/// let found = find_duplicates(&config, &roots, &options, |stage| show(stage))?;
pub fn find_duplicates(
    config: &Config,
    roots: &[PathBuf],
    options: &ScanOptions,
    on_stage: impl FnMut(dupes::Stage),
) -> Result<Duplicates, String> {
    let config = options.apply(config);
    dupes::find(&config, roots, Options::from_config(&config), on_stage)
        .map_err(|error| error.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Source {
    Artifact,
    Cache,
    Toolchain,
    Docker,
}

impl Source {
    pub const ALL: [Source; 4] = [Self::Artifact, Self::Cache, Self::Toolchain, Self::Docker];

    /// Toolchains and Docker objects are deleted for good; everything else goes to the trash.
    pub fn permanent(self) -> bool {
        matches!(self, Self::Toolchain | Self::Docker)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confirmation {
    Simple,
    TypeCount(usize),
}

impl Confirmation {
    /// @name Confirmation::for_items
    /// @description Applies the same rule as `kuizer clean`: a plain yes when every item is
    /// marked safe, and typing the item count when any of them is marked review.
    ///
    /// @example
    /// let rule = Confirmation::for_items(selected.iter().copied());
    pub fn for_items<'a>(candidates: impl Iterator<Item = &'a Candidate>) -> Self {
        let mut count = 0;
        let mut review = false;
        for candidate in candidates {
            count += 1;
            review |= candidate.rule.safety == Safety::Review;
        }
        if review {
            Self::TypeCount(count)
        } else {
            Self::Simple
        }
    }

    pub fn accepts(self, typed: &str) -> bool {
        match self {
            Self::Simple => true,
            Self::TypeCount(count) => typed.trim() == count.to_string(),
        }
    }
}

pub struct Batch {
    pub source: Source,
    pub roots: Vec<PathBuf>,
    pub candidates: Vec<Candidate>,
}

/// @name cache_roots
/// @description Uses the parent folder of each cache as a guard root, the same way `kuizer cache
/// --clean` does, because global caches live outside the scanned folders.
///
/// @example
/// let roots = cache_roots(&selected_caches);
pub fn cache_roots(candidates: &[Candidate]) -> Vec<PathBuf> {
    let mut parents: Vec<PathBuf> = candidates
        .iter()
        .filter_map(|candidate| candidate.path.parent().map(Path::to_path_buf))
        .collect();
    parents.sort();
    parents.dedup();
    parents
}

pub struct Outcome {
    pub removals: Vec<(Source, Removal)>,
    pub session: Option<u64>,
    pub history_error: Option<String>,
}

impl Outcome {
    pub fn removed(&self) -> impl Iterator<Item = &(Source, Removal)> {
        self.removals
            .iter()
            .filter(|(_, removal)| removal.status == Status::Removed)
    }

    /// Items that were deleted for good, which the undo bar cannot bring back.
    pub fn deleted(&self) -> usize {
        self.removed()
            .filter(|(source, _)| source.permanent())
            .count()
    }

    pub fn failed(&self) -> impl Iterator<Item = &(Source, Removal)> {
        self.removals
            .iter()
            .filter(|(_, removal)| removal.status == Status::Failed)
    }
}

fn run_batches(
    batches: Vec<Batch>,
    mode: Mode,
    removals: &mut Vec<(Source, Removal)>,
) -> (Option<u64>, Option<String>) {
    let mut roots: Vec<PathBuf> = batches
        .iter()
        .flat_map(|batch| batch.roots.iter().cloned())
        .collect();
    roots.sort();
    roots.dedup();
    let mut recorder = Recorder::for_user(history::now(), &roots);
    for batch in batches {
        let guard = Guard::new(&batch.roots);
        for candidate in batch.candidates {
            let result = match batch.source {
                Source::Toolchain => toolchains::remove(&guard, &candidate, mode),
                Source::Docker => docker::remove(&guard, &candidate, mode),
                Source::Artifact | Source::Cache => {
                    let target = Target {
                        path: candidate.path.clone(),
                        identity: candidate.identity,
                    };
                    delete::delete(&guard, &target, mode)
                }
            };
            let removal = match result {
                Ok(()) => {
                    recorder.add(Entry::new(&candidate, mode));
                    Removal {
                        candidate,
                        status: Status::Removed,
                        error: None,
                    }
                }
                Err(error) => Removal {
                    candidate,
                    status: Status::Failed,
                    error: Some(error.to_string()),
                },
            };
            removals.push((batch.source, removal));
        }
    }
    let session = recorder.session();
    (
        session,
        recorder.finish().err().map(|error| error.to_string()),
    )
}

/// @name clean
/// @description Moves artifacts and caches to the trash and deletes toolchains and Docker
/// objects for good. Each
/// batch is checked against its own guard right before acting, exactly like the CLI. Trashed
/// items are recorded as one run in the history file, so `kuizer restore` and the undo bar see
/// it; permanent removals are recorded as a separate run that nothing offers to undo.
///
/// @example
/// let outcome = clean(vec![Batch { source: Source::Artifact, roots, candidates }]);
pub fn clean(batches: Vec<Batch>) -> Outcome {
    let (permanent, trashed): (Vec<Batch>, Vec<Batch>) = batches
        .into_iter()
        .partition(|batch| batch.source.permanent());
    let mut removals = Vec::new();
    let (session, trash_error) = run_batches(trashed, Mode::Trash, &mut removals);
    let (_, permanent_error) = run_batches(permanent, Mode::Permanent, &mut removals);
    Outcome {
        removals,
        session,
        history_error: trash_error.or(permanent_error),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Undo,
    StillInTrash,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Restored {
    pub items: usize,
    pub bytes: u64,
    pub problems: Vec<(PathBuf, String)>,
    pub history_error: Option<String>,
}

/// @name restore
/// @description Puts one cleanup run back from the trash with the library's restore, exactly like
/// `kuizer restore s4`, and records the restore in the history. `Scope::Undo` asks for every item
/// of the run, so anything that cannot come back is reported; `Scope::StillInTrash` only asks for
/// the items that are still in the trash and not restored yet.
///
/// @example
/// let restored = restore(4, Scope::Undo)?;
pub fn restore(session: u64, scope: Scope) -> Result<Restored, String> {
    let path = history::default_path().map_err(|error| error.to_string())?;
    let history = history::load(&path).map_err(|error| error.to_string())?;
    let id = format!("s{session}");
    let targets = match scope {
        Scope::Undo => history
            .find(&id)
            .ok_or_else(|| format!("the history has no cleanup {id}"))?,
        Scope::StillInTrash => {
            let run = history
                .runs
                .iter()
                .position(|run| run.id == session)
                .ok_or_else(|| format!("the history has no cleanup {id}"))?;
            let trashed =
                history::trashed_paths().ok_or_else(|| RestoreError::Unsupported.to_string())?;
            let kept = history::still_in_trash(&history, &trashed);
            history::restorable(&history, &kept, run)
        }
    };
    if targets.is_empty() {
        return Err(format!("nothing from {id} is still in the trash"));
    }
    let restore =
        history::restore::restore(&history, &targets, false).map_err(|error| error.to_string())?;
    Ok(Restored {
        items: restore.count(RestoreStatus::Restored),
        bytes: restore.bytes(RestoreStatus::Restored),
        problems: restore
            .outcomes
            .iter()
            .filter(|outcome| outcome.status != RestoreStatus::Restored)
            .map(|outcome| {
                (
                    outcome.path.clone(),
                    outcome.reason.clone().unwrap_or_default(),
                )
            })
            .collect(),
        history_error: restore.record().err().map(|error| error.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use kuizer::detect::{Kind, RuleInfo};
    use kuizer::scan::Usage;

    use super::*;

    fn candidate(id: u64, path: &str, safety: Safety) -> Candidate {
        Candidate {
            id,
            path: PathBuf::from(path),
            kind: Kind::Dir,
            rule: Arc::new(RuleInfo {
                name: "node_modules".to_owned(),
                category: Category::Dependencies,
                safety,
                ecosystem: "node".to_owned(),
            }),
            project: None,
            usage: Usage {
                allocated: id * 10,
                ..Usage::default()
            },
            newest: None,
            identity: None,
            project_last_active: None,
        }
    }

    fn trashed(path: &str, mode: Mode) -> Entry {
        Entry {
            deleted_at: history::now(),
            path: PathBuf::from(path),
            rule: "node_modules".to_owned(),
            category: "dependencies".to_owned(),
            mode,
            bytes: 10,
            size: "10 B".to_owned(),
            restores: None,
        }
    }

    #[test]
    fn each_run_says_what_can_still_be_restored() {
        let run = |entries| history::Run::new(history::now(), &[], entries);
        let mut runs = vec![
            run(vec![
                trashed("/p/a", Mode::Trash),
                trashed("/p/b", Mode::Trash),
            ]),
            run(vec![trashed("/p/c", Mode::Trash)]),
            run(vec![trashed("/p/d", Mode::Trash)]),
            run(vec![trashed("/p/e", Mode::Permanent)]),
        ];
        runs.push(history::Run::restore(
            history::now(),
            vec![Entry::restored(&runs[1].deleted[0], "s2.1".to_owned())],
        ));
        for (index, run) in runs.iter_mut().enumerate() {
            run.id = index as u64 + 1;
        }
        let view = HistoryView {
            history: History {
                next_session: 6,
                runs,
                ..History::default()
            },
            kept: HashSet::from([(0, 1)]),
            trash_known: true,
        };
        assert_eq!(
            view.trash_state(0),
            TrashState::Restorable {
                items: 1,
                bytes: 10
            }
        );
        assert_eq!(view.trash_state(1), TrashState::Restored);
        assert_eq!(view.trash_state(2), TrashState::Emptied);
        assert_eq!(view.trash_state(3), TrashState::NotTrashed);
        assert_eq!(view.trash_state(9), TrashState::NotTrashed);
        let unknown = HistoryView {
            trash_known: false,
            ..view
        };
        assert_eq!(unknown.trash_state(0), TrashState::Unknown);
    }

    #[test]
    fn duplicates_on_request_match_the_ones_a_scan_walk_collects() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("photos");
        for (folder, size) in [
            ("a", 8192),
            ("b", 8192),
            ("c", 9000),
            ("d", 9000),
            ("e", 5000),
        ] {
            std::fs::create_dir_all(root.join(folder)).unwrap();
            std::fs::write(root.join(folder).join("same.bin"), vec![b'z'; size]).unwrap();
        }
        std::fs::create_dir_all(root.join("site/node_modules")).unwrap();
        std::fs::write(root.join("site/package.json"), "{}").unwrap();
        std::fs::write(root.join("site/node_modules/same.bin"), vec![b'z'; 8192]).unwrap();
        let config = Config::default();
        let roots = scan::normalize_roots(&[root]);
        let options = ScanOptions::from_config(&config);
        let groups = |found: Duplicates| -> Vec<Vec<PathBuf>> {
            found
                .groups
                .into_iter()
                .map(|group| {
                    group
                        .members
                        .into_iter()
                        .map(|member| member.path)
                        .collect()
                })
                .collect()
        };

        let lazy = groups(find_duplicates(&config, &roots, &options, |_| {}).unwrap());
        let applied = options.apply(&config);
        let duplicate_options = Options::from_config(&applied);
        let (handle, walked) = repos::start_scan(
            &applied,
            &roots,
            Cancel::new(),
            Some(duplicate_options),
            false,
        )
        .unwrap();
        scan::collect(&handle, |_| {});
        let files = walked.recv().unwrap().files.unwrap();
        let walked = groups(dupes::group(files, duplicate_options, |_| {}));
        assert_eq!(lazy.len(), 2);
        assert_eq!(lazy, walked);
    }

    #[test]
    fn safe_items_need_a_plain_yes() {
        let items = [
            candidate(1, "/p/a", Safety::Safe),
            candidate(2, "/p/b", Safety::Safe),
        ];
        let rule = Confirmation::for_items(items.iter());
        assert_eq!(rule, Confirmation::Simple);
        assert!(rule.accepts(""));
    }

    #[test]
    fn any_review_item_requires_typing_the_count() {
        let items = [
            candidate(1, "/p/a", Safety::Safe),
            candidate(2, "/p/b", Safety::Review),
            candidate(3, "/p/c", Safety::Safe),
        ];
        let rule = Confirmation::for_items(items.iter());
        assert_eq!(rule, Confirmation::TypeCount(3));
        assert!(!rule.accepts(""));
        assert!(!rule.accepts("2"));
        assert!(!rule.accepts("y"));
        assert!(rule.accepts("3"));
        assert!(rule.accepts(" 3 "));
    }

    #[test]
    fn short_path_strips_the_scan_root() {
        let roots = [PathBuf::from("/p")];
        assert_eq!(short_path(Path::new("/p/a/dist"), &roots), "a/dist");
        assert_eq!(short_path(Path::new("/q/b"), &roots), "/q/b");
        assert_eq!(short_path(Path::new("/p"), &roots), "/p");
    }

    #[test]
    fn dropped_paths_leave_every_list() {
        let member = |path: &str| dupes::Member {
            path: PathBuf::from(path),
            links: Vec::new(),
            modified: None,
        };
        let group = |paths: &[&str]| dupes::Group {
            size: 10,
            allocated: 10,
            hash: String::new(),
            verified: true,
            members: paths.iter().map(|path| member(path)).collect(),
        };
        let mut report = Report {
            roots: vec![PathBuf::from("/p")],
            options: ScanOptions::from_config(&Config::default()),
            candidates: vec![
                candidate(1, "/p/a/node_modules", Safety::Safe),
                candidate(2, "/p/b/node_modules", Safety::Safe),
            ],
            issues: vec![Issue {
                path: PathBuf::from("/p/a/node_modules/locked"),
                kind: scan::IssueKind::PermissionDenied,
                message: String::new(),
            }],
            summary: Summary {
                progress: Progress::default(),
                duration: std::time::Duration::ZERO,
                cancelled: false,
            },
            caches: Vec::new(),
            toolchains: Vec::new(),
            docker: Vec::new(),
            docker_issues: Vec::new(),
            repos: Vec::new(),
            duplicates: Some(Duplicates {
                groups: vec![
                    group(&["/p/a/node_modules/x", "/p/b/x"]),
                    group(&["/p/a/node_modules/y", "/p/b/y", "/p/c/y"]),
                ],
                issues: Vec::new(),
                files: 5,
                duration: std::time::Duration::ZERO,
            }),
            history: Err(String::new()),
            disks: Vec::new(),
            scanned: SystemTime::now(),
        };
        report.drop_paths(&[PathBuf::from("/p/a/node_modules")]);
        assert_eq!(report.candidates.len(), 1);
        assert_eq!(report.candidates[0].id, 2);
        assert!(report.issues.is_empty());
        let groups = &report.duplicates.as_ref().unwrap().groups;
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].members.len(), 2);
    }

    #[test]
    fn cache_roots_are_the_unique_parents() {
        let items = [
            candidate(1, "/home/u/.cache/pip", Safety::Safe),
            candidate(2, "/home/u/.cache/yarn", Safety::Safe),
            candidate(3, "/home/u/.cargo/registry", Safety::Review),
        ];
        assert_eq!(
            cache_roots(&items),
            vec![
                PathBuf::from("/home/u/.cache"),
                PathBuf::from("/home/u/.cargo")
            ]
        );
    }

    #[test]
    fn report_groups_by_category_and_project() {
        let mut first = candidate(1, "/p/a/node_modules", Safety::Safe);
        first.project = Some(PathBuf::from("/p/a"));
        let mut second = candidate(4, "/p/a/dist", Safety::Review);
        second.project = Some(PathBuf::from("/p/a"));
        let report = Report {
            roots: vec![PathBuf::from("/p")],
            options: ScanOptions::from_config(&Config::default()),
            candidates: vec![first, second],
            issues: Vec::new(),
            summary: Summary {
                progress: Progress::default(),
                duration: std::time::Duration::ZERO,
                cancelled: false,
            },
            caches: Vec::new(),
            toolchains: Vec::new(),
            docker: Vec::new(),
            docker_issues: Vec::new(),
            repos: Vec::new(),
            duplicates: None,
            history: Err(String::new()),
            disks: Vec::new(),
            scanned: SystemTime::now(),
        };
        assert_eq!(report.total(None), 50);
        assert_eq!(report.total(Some(Safety::Review)), 40);
        let projects = report.by_project();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].bytes, 50);
        assert_eq!(projects[0].count, 2);
        let categories = report.by_category();
        assert_eq!(categories.len(), 1);
        assert_eq!(categories[0].count, 2);
    }
}
