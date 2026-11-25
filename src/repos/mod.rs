mod actions;
mod refs;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use crossbeam_channel::Receiver;
use serde::{Deserialize, Serialize};

#[cfg(test)]
use self::actions::parse_count;
pub use self::actions::{busy, collect_garbage, delete_branches};
use self::actions::{count_objects, default_branch, merged_branches, read};
use self::refs::OnDisk;

use crate::config::{Config, ConfigError, GitConfig};
use crate::dupes::{Files, Options};
use crate::platform::dir::EntryKind;
use crate::platform::{self, MountTable};
use crate::protect::Protected;
use crate::scan::walk::{Companion, FileEntry, Flow, Visitor, Walker};
use crate::scan::{self, Cancel, Issue, IssueKind, Scan};
use crate::toolchains::PinFinder;

const KIB: u64 = 1024;
const STALE_GC: Duration = Duration::from_secs(12 * 60 * 60);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Objects {
    pub loose: u64,
    pub loose_bytes: u64,
    pub packs: u64,
    pub packed_bytes: u64,
    pub prunable: u64,
    pub garbage_bytes: u64,
}

impl Objects {
    /// @name Objects::gain
    /// @description Estimates what `git gc` frees: the loose objects it packs and the garbage
    /// files it removes. Packing keeps a compressed copy, so the real gain is somewhat lower.
    ///
    /// @example
    /// println!("up to {} freed", ByteSize(objects.gain()));
    pub fn gain(&self) -> u64 {
        self.loose_bytes + self.garbage_bytes
    }

    pub fn total(&self) -> u64 {
        self.loose_bytes + self.packed_bytes + self.garbage_bytes
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Branch {
    pub name: String,
    pub commit: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Busy {
    Rebase,
    Merge,
    CherryPick,
    Revert,
    Bisect,
    Locked,
    Collecting,
}

impl Busy {
    pub fn describe(self) -> &'static str {
        match self {
            Self::Rebase => "a rebase is in progress",
            Self::Merge => "a merge is in progress",
            Self::CherryPick => "a cherry-pick is in progress",
            Self::Revert => "a revert is in progress",
            Self::Bisect => "a bisect is in progress",
            Self::Locked => "another git process holds a lock",
            Self::Collecting => "git gc is already running",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repository {
    pub root: PathBuf,
    pub git_dir: PathBuf,
    pub objects: Objects,
    pub default_branch: Option<String>,
    pub current_branch: Option<String>,
    pub merged: Vec<Branch>,
    pub busy: Option<Busy>,
}

impl Repository {
    /// @name Repository::worth
    /// @description Returns whether the repository has something to clean: a `git gc` gain of at
    /// least `min_gain`, or local branches already merged into the default branch.
    ///
    /// @example
    /// found.retain(|repository| repository.worth(config.git.min_gain.0));
    pub fn worth(&self, min_gain: u64) -> bool {
        self.collectable(min_gain) || !self.merged.is_empty()
    }

    /// @name Repository::collectable
    /// @description Returns whether `git gc` would free at least `min_gain` here, the bar for
    /// running it in a bulk cleanup.
    ///
    /// @example
    /// let ready = found.iter().filter(|repository| repository.collectable(min_gain));
    pub fn collectable(&self, min_gain: u64) -> bool {
        self.objects.gain() >= min_gain.max(1)
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RepoError {
    #[error("{} is not a git repository", .0.display())]
    NotRepository(PathBuf),
    #[error("skipped because {}", .0.describe())]
    Busy(Busy),
    #[error("branch `{0}` is no longer merged or has moved")]
    Changed(String),
    #[error("branch `{0}` is kept by the git.keep_branches setting or is checked out")]
    Kept(String),
    #[error("git failed: {0}")]
    Git(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Collected {
    pub before: u64,
    pub after: u64,
}

impl Collected {
    pub fn freed(&self) -> u64 {
        self.before.saturating_sub(self.after)
    }
}

#[derive(Debug)]
pub struct Pruned {
    pub branch: Branch,
    pub result: Result<(), RepoError>,
}

#[derive(Debug, Default)]
pub struct Repositories {
    pub roots: Vec<PathBuf>,
}

impl Visitor for Repositories {
    fn issue(&mut self, _issue: Issue) -> Flow {
        Flow::Continue
    }

    fn repository(&mut self, root: &Path) {
        self.roots.push(root.to_path_buf());
    }
}

pub struct Walked {
    pub files: Option<Files>,
    pub pins: Option<PinFinder>,
    pub repositories: Vec<PathBuf>,
}

impl Visitor for Walked {
    fn wants_files(&self) -> bool {
        self.files.wants_files() || self.pins.wants_files()
    }

    fn wants_stat(&self) -> bool {
        self.files.wants_stat() || self.pins.wants_stat()
    }

    fn issue(&mut self, issue: Issue) -> Flow {
        match (self.files.issue(issue.clone()), self.pins.issue(issue)) {
            (Flow::Continue, Flow::Continue) => Flow::Continue,
            _ => Flow::Stop,
        }
    }

    fn counted(&mut self, kind: EntryKind) {
        self.files.counted(kind);
        self.pins.counted(kind);
    }

    fn entered(&mut self, path: &Path) -> Flow {
        match (self.files.entered(path), self.pins.entered(path)) {
            (Flow::Stop, _) | (_, Flow::Stop) => Flow::Stop,
            (Flow::Skip, _) | (_, Flow::Skip) => Flow::Skip,
            _ => Flow::Continue,
        }
    }

    fn file(&mut self, path: &Path, kind: EntryKind) -> Flow {
        match (self.files.file(path, kind), self.pins.file(path, kind)) {
            (Flow::Continue, Flow::Continue) => Flow::Continue,
            _ => Flow::Stop,
        }
    }

    fn entry(&mut self, entry: &FileEntry<'_>) -> Flow {
        match (self.files.entry(entry), self.pins.entry(entry)) {
            (Flow::Continue, Flow::Continue) => Flow::Continue,
            _ => Flow::Stop,
        }
    }

    fn repository(&mut self, root: &Path) {
        self.files.repository(root);
        self.pins.repository(root);
        self.repositories.push(root.to_path_buf());
    }
}

impl Companion for Walked {
    fn fork(&self) -> Self {
        Self {
            files: self.files.fork(),
            pins: self.pins.fork(),
            repositories: Vec::new(),
        }
    }

    fn merge(&mut self, other: Self) {
        self.files.merge(other.files);
        self.pins.merge(other.pins);
        self.repositories.extend(other.repositories);
    }
}

/// @name start_scan
/// @description Starts an artifact scan that also notes every git repository during the same
/// walk, and collects files for duplicate detection with `duplicates` and toolchain pin files
/// with `pins`. The receiver yields what was collected once the walk ends.
///
/// @example
/// let (scan, walked) = repos::start_scan(&config, &roots, Cancel::new(), None, false)?;
/// let collected = scan::collect(&scan, |_| {});
/// let found = repos::inspect_all(walked.recv()?.repositories, &config.git, &Cancel::new());
pub fn start_scan(
    config: &Config,
    roots: &[PathBuf],
    cancel: Cancel,
    duplicates: Option<Options>,
    pins: bool,
) -> Result<(Scan, Receiver<Walked>), ConfigError> {
    scan::start_with(
        config,
        roots,
        cancel,
        Walked {
            files: duplicates.map(|options| Files::for_roots(options, roots)),
            pins: pins.then(PinFinder::new),
            repositories: Vec::new(),
        },
    )
}

/// @name find
/// @description Walks `roots` with the scan settings and returns the top-level folder of every
/// git repository found, without sizing any artifact.
///
/// @example
/// let roots = repos::find(&config, &[PathBuf::from("/home/me/apps")])?;
pub fn find(config: &Config, roots: &[PathBuf]) -> Result<Vec<PathBuf>, ConfigError> {
    let detector = config.detector()?;
    let excludes = config.excludes()?;
    let mounts = MountTable::load();
    let protected = Protected::for_user();
    let mut walker = Walker {
        scan: &config.scan,
        detector: &detector,
        excludes: &excludes,
        mounts: &mounts,
        protected: &protected,
        visitor: Repositories::default(),
    };
    for root in scan::normalize_roots(roots) {
        walker.walk_root(&root);
    }
    let mut found = walker.visitor.roots;
    found.sort();
    found.dedup();
    Ok(found)
}

const ROTATIONAL_THREADS: usize = 2;
const ROTATIONAL_SAMPLE: usize = 8;

fn threads(config: &GitConfig, roots: &[PathBuf]) -> usize {
    match config.threads {
        0 if roots
            .iter()
            .take(ROTATIONAL_SAMPLE)
            .any(|root| platform::rotational_at(root)) =>
        {
            ROTATIONAL_THREADS
        }
        0 => thread::available_parallelism().map_or(4, |n| n.get().min(8)),
        n => n,
    }
}

#[derive(Debug, Default)]
pub struct Inspected {
    pub repositories: Vec<Repository>,
    pub issues: Vec<Issue>,
}

/// @name inspect_all
/// @description Inspects every repository in parallel and keeps the ones worth cleaning, largest
/// gain first. Stops early once `cancel` is set.
///
/// @example
/// let found = repos::inspect_all(repos::find(&config, &roots)?, &config.git, &Cancel::new());
pub fn inspect_all(roots: Vec<PathBuf>, config: &GitConfig, cancel: &Cancel) -> Vec<Repository> {
    inspect_all_with(roots, config, cancel, |_| {})
}

/// @name inspect_all_with
/// @description Like `inspect_all`, but calls `on_inspected` from the inspecting thread with
/// each repository root once it was read, kept or not, so a frontend can count what is left.
///
/// @example
/// let found = repos::inspect_all_with(roots, &config.git, &cancel, |root| seen.push(root.to_owned()));
pub fn inspect_all_with(
    roots: Vec<PathBuf>,
    config: &GitConfig,
    cancel: &Cancel,
    on_inspected: impl Fn(&Path) + Sync,
) -> Vec<Repository> {
    inspect_all_collecting(roots, config, cancel, on_inspected).repositories
}

/// @name inspect_all_collecting
/// @description Like `inspect_all_with`, and returns any repository inspection failures as
/// [`Issue`] entries instead of dropping them.
///
/// @example
/// let inspected = repos::inspect_all_collecting(roots, &config.git, &cancel, |_| {});
pub fn inspect_all_collecting(
    roots: Vec<PathBuf>,
    config: &GitConfig,
    cancel: &Cancel,
    on_inspected: impl Fn(&Path) + Sync,
) -> Inspected {
    if roots.is_empty() {
        return Inspected::default();
    }
    let workers = threads(config, &roots).min(roots.len());
    let queue = Mutex::new(roots);
    let found = Mutex::new(Vec::new());
    let issues = Mutex::new(Vec::new());
    thread::scope(|scope| {
        let run_worker = || loop {
            if cancel.is_cancelled() {
                return;
            }
            let Some(root) = queue.lock().unwrap_or_else(PoisonError::into_inner).pop() else {
                return;
            };
            match inspect(&root, config) {
                Ok(repository) if repository.worth(config.min_gain.0) => {
                    found
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push(repository);
                }
                Ok(_) => {}
                Err(error) => {
                    let kind = match &error {
                        RepoError::NotRepository(_) if !root.join(".git").exists() => {
                            IssueKind::Vanished
                        }
                        _ => IssueKind::Io,
                    };
                    issues
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push(Issue {
                            path: root.clone(),
                            kind,
                            message: error.to_string(),
                        });
                }
            }
            on_inspected(&root);
        };
        let mut spawned = 0usize;
        for index in 0..workers {
            if thread::Builder::new()
                .name(format!("kuizer-repo-{index}"))
                .spawn_scoped(scope, run_worker)
                .is_ok()
            {
                spawned += 1;
            }
        }
        if spawned == 0 {
            run_worker();
        }
    });
    let mut repositories = found.into_inner().unwrap_or_else(PoisonError::into_inner);
    repositories.sort_by(|a, b| {
        b.objects
            .gain()
            .cmp(&a.objects.gain())
            .then_with(|| b.merged.len().cmp(&a.merged.len()))
            .then_with(|| a.root.cmp(&b.root))
    });
    let mut issues = issues.into_inner().unwrap_or_else(PoisonError::into_inner);
    issues.sort_by(|a, b| a.path.cmp(&b.path).then_with(|| a.message.cmp(&b.message)));
    Inspected {
        repositories,
        issues,
    }
}

/// @name inspect
/// @description Reads one repository: its object counts, its default and current branch, the
/// local branches already merged into the default branch that may be removed, and whether
/// another git operation is in progress. Refs and object counts are read from the git folder
/// directly; repositories with a layout this does not understand (reftable, SHA-256, linked
/// worktrees) are asked through `git` instead. Kept branches, the default branch and branches
/// checked out in any worktree are never listed as merged.
///
/// @example
/// let repository = repos::inspect(Path::new("/home/me/apps/kearn"), &config.git)?;
/// println!("{} merged branches", repository.merged.len());
pub fn inspect(root: &Path, config: &GitConfig) -> Result<Repository, RepoError> {
    let git_dir = root.join(".git");
    if !git_dir.is_dir() {
        return Err(RepoError::NotRepository(root.to_path_buf()));
    }
    let on_disk = OnDisk::read(&git_dir);
    inspect_with(root, git_dir, on_disk.as_ref(), config)
}

fn inspect_with(
    root: &Path,
    git_dir: PathBuf,
    on_disk: Option<&OnDisk>,
    config: &GitConfig,
) -> Result<Repository, RepoError> {
    let objects = match on_disk {
        Some(on_disk) => on_disk.objects,
        None => count_objects(root)?,
    };
    let refs = on_disk.map(|on_disk| &on_disk.refs);
    let current_branch = match refs {
        Some(refs) => refs.current.clone(),
        None => read(root, &["symbolic-ref", "--quiet", "--short", "HEAD"]),
    };
    let default_branch = default_branch(root, refs, config);
    let merged = match &default_branch {
        Some(base) => merged_branches(root, refs, base, current_branch.as_deref(), config),
        None => Vec::new(),
    };
    Ok(Repository {
        busy: busy(&git_dir),
        root: root.to_path_buf(),
        git_dir,
        objects,
        default_branch,
        current_branch,
        merged,
    })
}
