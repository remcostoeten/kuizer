use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use kuizer::caches::{self, KnownCache, Locations};
use kuizer::cli::SortKey;
use kuizer::commands::{measure_caches, sort_candidates};
use kuizer::config::Config;
use kuizer::delete::{self, Guard, Mode, Removal, Status, Target};
use kuizer::detect::{Category, Safety};
use kuizer::dupes::{self, Duplicates, Options};
use kuizer::history::{self, Action, Entry, History, Recorder};
use kuizer::output::display_path;
use kuizer::platform;
use kuizer::protect::Protected;
use kuizer::scan::{self, Cancel, Candidate, Issue, Progress, Summary};

use crate::start::ScanOptions;

pub const CANCELLED: &str = "cancelled";

#[derive(Debug, Clone, Copy, Default)]
pub enum Stage {
    #[default]
    Starting,
    Scanning(Progress),
    Duplicates(dupes::Stage),
    Caches,
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
}

pub struct Report {
    pub roots: Vec<PathBuf>,
    pub options: ScanOptions,
    pub candidates: Vec<Candidate>,
    pub issues: Vec<Issue>,
    pub summary: Summary,
    pub caches: Vec<(KnownCache, Candidate)>,
    pub duplicates: Option<Duplicates>,
    pub history: Result<HistoryView, String>,
    pub disks: Vec<Disk>,
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
/// @description Gathers everything the dashboard shows in one pass: project artifacts and
/// duplicate files from a single walk, global caches, the cleanup history and free disk space.
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
    let (handle, files) = dupes::start_scan(&config, roots, cancel.clone(), duplicate_options)
        .map_err(|error| error.to_string())?;
    let collected = scan::collect(&handle, |progress| set(stage, Stage::Scanning(progress)));
    let files = files.recv().ok().flatten();
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
        Locations::for_user()
            .map(|locations| measure_caches(caches::find(&locations, &Protected::for_user())))
            .unwrap_or_default()
    } else {
        Vec::new()
    };

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
        duplicates,
        history,
        disks,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Source {
    Artifact,
    Cache,
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
    pub history_error: Option<String>,
}

impl Outcome {
    pub fn removed(&self) -> impl Iterator<Item = &(Source, Removal)> {
        self.removals
            .iter()
            .filter(|(_, removal)| removal.status == Status::Removed)
    }

    pub fn failed(&self) -> impl Iterator<Item = &(Source, Removal)> {
        self.removals
            .iter()
            .filter(|(_, removal)| removal.status == Status::Failed)
    }
}

/// @name trash
/// @description Moves every candidate to the trash. Each batch is checked against its own guard
/// right before acting, exactly like the CLI, and everything that was moved is recorded as one
/// run in the history file so `kuizer history` and `kuizer restore` see it.
///
/// @example
/// let outcome = trash(vec![Batch { source: Source::Artifact, roots, candidates }]);
pub fn trash(batches: Vec<Batch>) -> Outcome {
    let mut all_roots: Vec<PathBuf> = batches
        .iter()
        .flat_map(|batch| batch.roots.iter().cloned())
        .collect();
    all_roots.sort();
    all_roots.dedup();
    let mut recorder = Recorder::for_user(history::now(), &all_roots);
    let mut removals = Vec::new();
    for batch in batches {
        let guard = Guard::new(&batch.roots);
        for candidate in batch.candidates {
            let target = Target {
                path: candidate.path.clone(),
                identity: candidate.identity,
            };
            let removal = match delete::delete(&guard, &target, Mode::Trash) {
                Ok(()) => {
                    recorder.add(Entry::new(&candidate, Mode::Trash));
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
    Outcome {
        removals,
        history_error: recorder.finish().err().map(|error| error.to_string()),
    }
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
            duplicates: None,
            history: Err(String::new()),
            disks: Vec::new(),
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
