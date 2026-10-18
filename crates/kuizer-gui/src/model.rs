use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use kuizer::caches::{self, KnownCache, Locations};
use kuizer::cli::SortKey;
use kuizer::commands::{measure_caches, sort_candidates};
use kuizer::config::Config;
use kuizer::delete::{self, Guard, Mode, Removal, Status, Target};
use kuizer::detect::{Category, Safety};
use kuizer::dupes::{self, Duplicates, Options};
use kuizer::history::restore::{RestoreError, Status as RestoreStatus};
use kuizer::history::{self, Action, Entry, History, ItemState, Recorder};
use kuizer::output::display_path;
use kuizer::platform;
use kuizer::protect::Protected;
use kuizer::scan::{self, Cancel, Candidate, Issue, Progress, Summary};
use kuizer::toolchains::{self, Toolchain};

use crate::start::ScanOptions;

pub const CANCELLED: &str = "cancelled";

#[derive(Debug, Clone, Copy, Default)]
pub enum Stage {
    #[default]
    Starting,
    Scanning(Progress),
    Duplicates(dupes::Stage),
    Caches,
    Toolchains,
    History,
}

impl Stage {
    pub fn describe(self) -> String {
        match self {
            Self::Starting => "Starting scan".to_owned(),
            Self::Scanning(progress) => format!(
                "Scanning  {} folders  {} files  {} found",
                progress.dirs, progress.files, progress.candidates
            ),
            Self::Duplicates(dupes::Stage::Walking { files }) => {
                format!("Duplicates  walking {files} files")
            }
            Self::Duplicates(dupes::Stage::Hashing { files }) => {
                format!("Duplicates  hashing {files} files")
            }
            Self::Duplicates(dupes::Stage::Verifying { groups }) => {
                format!("Duplicates  comparing {groups} groups")
            }
            Self::Caches => "Measuring global caches".to_owned(),
            Self::Toolchains => "Measuring installed toolchains".to_owned(),
            Self::History => "Reading the cleanup history".to_owned(),
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

    /// @name HistoryView::trash_state
    /// @description Says whether a delete run can still be restored from the trash, using the
    /// library's per item state: what is left in the trash, all of it restored, gone from the
    /// trash, unknown because the trash cannot be read, or never trashed at all.
    ///
    /// @example
    /// if let TrashState::Restorable { items, bytes } = view.trash_state(run) { .. }
    pub fn trash_state(&self, run: usize) -> TrashState {
        let Some(entries) = self.history.runs.get(run).map(|run| run.deleted.len()) else {
            return TrashState::NotTrashed;
        };
        let restored = self.history.restored_ids();
        let kept = self.trash_known.then_some(&self.kept);
        let states: Vec<ItemState> = (0..entries)
            .map(|entry| history::item_state(&self.history, &restored, kept, run, entry))
            .filter(|state| !matches!(state, ItemState::Deleted | ItemState::DryRun))
            .collect();
        if states.is_empty() {
            return TrashState::NotTrashed;
        }
        if !self.trash_known {
            return TrashState::Unknown;
        }
        let left = history::restorable(&self.history, &self.kept, run);
        if !left.is_empty() {
            return TrashState::Restorable {
                items: left.len(),
                bytes: left
                    .iter()
                    .map(|&(run, entry)| self.history.runs[run].deleted[entry].bytes)
                    .sum(),
            };
        }
        if states.iter().all(|state| *state == ItemState::Restored) {
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

fn set(stage: &Mutex<Stage>, next: Stage) {
    if let Ok(mut current) = stage.lock() {
        *current = next;
    }
}

/// @name load
/// @description Gathers everything the dashboard shows in one pass: project artifacts, duplicate
/// files and toolchain pins from a single walk, global caches, installed toolchains, the cleanup
/// history and free disk space.
/// The start screen's options decide which of these run. Reports progress through `stage` so the
/// window can show it while this runs on a worker thread, and stops early once `cancel` is set.
///
/// @example
/// let stage = Mutex::new(Stage::default());
/// let report = load(&config, &roots, &options, &Cancel::new(), &stage)?;
pub fn load(
    config: &Config,
    roots: &[PathBuf],
    options: &ScanOptions,
    cancel: &Cancel,
    stage: &Mutex<Stage>,
) -> Result<Report, String> {
    let config = options.apply(config);
    let duplicate_options = options.duplicates.then(|| Options::from_config(&config));
    let pin_search = options.toolchains && config.toolchains.pins;
    let (handle, found) = dupes::start_scan_with_pins(
        &config,
        roots,
        cancel.clone(),
        duplicate_options,
        pin_search,
    )
    .map_err(|error| error.to_string())?;
    let collected = scan::collect(&handle, |progress| set(stage, Stage::Scanning(progress)));
    let (files, pins) = found.recv().unwrap_or_default();
    if cancel.is_cancelled() {
        return Err(CANCELLED.to_owned());
    }
    let mut candidates = collected.candidates;
    sort_candidates(&mut candidates, SortKey::Size);

    let duplicates = duplicate_options
        .zip(files)
        .map(|(duplicate_options, files)| {
            dupes::group(files, duplicate_options, |progress| {
                set(stage, Stage::Duplicates(progress));
            })
        });
    if cancel.is_cancelled() {
        return Err(CANCELLED.to_owned());
    }

    let caches = if options.caches {
        set(stage, Stage::Caches);
        let excludes = config.excludes().ok();
        Locations::for_user()
            .map(|locations| {
                let found = caches::find(&locations, &Protected::for_user())
                    .into_iter()
                    .filter(|cache| {
                        excludes
                            .as_ref()
                            .is_none_or(|excludes| !excludes.path(&cache.path))
                    })
                    .collect();
                measure_caches(found)
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let toolchains = if options.toolchains {
        set(stage, Stage::Toolchains);
        toolchains::Locations::for_user()
            .map(|locations| {
                let mut found = toolchains::find(&locations, &config.toolchains.tools);
                if let Some(pins) = &pins {
                    toolchains::apply_pins(&mut found, &pins.pins);
                }
                toolchains::measure(found)
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    if cancel.is_cancelled() {
        return Err(CANCELLED.to_owned());
    }

    set(stage, Stage::History);
    let history = HistoryView::load();
    let disks = roots
        .iter()
        .map(|path| Disk {
            available: platform::available_space(path),
            total: platform::total_space(path),
        })
        .collect();

    Ok(Report {
        roots: roots.to_vec(),
        options: options.clone(),
        candidates,
        issues: collected.issues,
        summary: collected.summary,
        caches,
        toolchains,
        duplicates,
        history,
        disks,
        scanned: SystemTime::now(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Source {
    Artifact,
    Cache,
    Toolchain,
}

impl Source {
    /// Toolchains are deleted for good; everything else goes to the trash.
    pub fn permanent(self) -> bool {
        self == Self::Toolchain
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
            let result = if batch.source.permanent() {
                toolchains::remove(&guard, &candidate, mode)
            } else {
                let target = Target {
                    path: candidate.path.clone(),
                    identity: candidate.identity,
                };
                delete::delete(&guard, &target, mode)
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
/// @description Moves artifacts and caches to the trash and deletes toolchains for good. Each
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
