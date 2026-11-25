mod hash;
#[cfg(test)]
mod tests;
mod walk;

use std::path::PathBuf;
use std::thread;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use self::hash::{Pass, SAMPLED, same_size_candidates, verified};
pub use self::walk::Files;
use self::walk::{Reporting, seen};

use crate::config::{Config, ConfigError};
use crate::platform::{self, MountTable};
use crate::protect::Protected;
use crate::scan::walk::Walker;
use crate::scan::{Cancel, Issue};

const ROTATIONAL_WORKERS: usize = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Member {
    pub path: PathBuf,
    pub links: Vec<PathBuf>,
    pub modified: Option<SystemTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub size: u64,
    pub allocated: u64,
    pub hash: String,
    pub verified: bool,
    pub members: Vec<Member>,
}

impl Group {
    pub fn reclaimable(&self) -> u64 {
        self.allocated
            .saturating_mul(self.members.len().saturating_sub(1) as u64)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Duplicates {
    pub groups: Vec<Group>,
    pub issues: Vec<Issue>,
    pub files: u64,
    pub duration: Duration,
}

impl Duplicates {
    pub fn reclaimable(&self) -> u64 {
        self.groups.iter().map(Group::reclaimable).sum()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Walking { files: u64 },
    Hashing { files: usize },
    Verifying { groups: usize },
}

#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub min_size: u64,
    pub threads: usize,
    pub verify: bool,
    pub include_hidden: bool,
}

impl Options {
    pub fn from_config(config: &Config) -> Self {
        Self {
            min_size: config.duplicates.min_size.0,
            threads: config.duplicates.threads,
            verify: config.duplicates.verify,
            include_hidden: config.duplicates.include_hidden,
        }
    }

    fn workers(self, roots: &[PathBuf]) -> usize {
        match self.threads {
            0 if roots.iter().any(|root| platform::rotational_at(root)) => ROTATIONAL_WORKERS,
            0 => thread::available_parallelism().map_or(4, |n| n.get().min(8)),
            n => n,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct Seen {
    pub(super) path: PathBuf,
    pub(super) size: u64,
    pub(super) allocated: u64,
    pub(super) identity: Option<(u64, u64)>,
    pub(super) modified: Option<SystemTime>,
}

#[derive(Debug, Clone)]
pub(super) struct Candidate {
    pub(super) size: u64,
    pub(super) allocated: u64,
    pub(super) member: Member,
}

/// @name find
/// @description Finds files with identical contents under `roots`. Files are grouped by size,
/// then by a BLAKE3 hash of three 16 KiB samples (start, middle, end), then by a full hash, and
/// with `verify` by a byte comparison. Hard links to one inode count as a single member. Empty
/// files, symlinks, generated artifact directories, browser or credential data and, unless
/// `include_hidden` is set, hidden home folders and tool-managed caches are never considered.
///
/// @example
/// let found = dupes::find(&config, &roots, Options::from_config(&config), |_| {})?;
/// println!("{} reclaimable", found.reclaimable());
pub fn find(
    config: &Config,
    roots: &[PathBuf],
    options: Options,
    on_stage: impl FnMut(Stage),
) -> Result<Duplicates, ConfigError> {
    find_cancellable(config, roots, options, &Cancel::new(), on_stage)
}

/// @name find_cancellable
/// @description Like `find`, and stops within one file or one MiB of reading once `cancel` is
/// set, returning no groups.
///
/// @example
/// let cancel = Cancel::new();
/// let found = dupes::find_cancellable(&config, &roots, options, &cancel, |_| {})?;
pub fn find_cancellable(
    config: &Config,
    roots: &[PathBuf],
    options: Options,
    cancel: &Cancel,
    mut on_stage: impl FnMut(Stage),
) -> Result<Duplicates, ConfigError> {
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
        visitor: Reporting {
            files: Files::for_roots(options, roots),
            cancel,
            on_stage: &mut on_stage,
        },
    };
    for root in roots {
        walker.walk_root(root);
    }
    let files = walker.visitor.files;
    Ok(group_cancellable(files, options, cancel, on_stage))
}

/// @name group
/// @description Groups files collected during a walk into sets of identical contents, the same
/// way `find` does after its own walk. Lets a caller that already walks the tree, such as the
/// dashboard, collect `Files` alongside that walk instead of walking twice.
///
/// @example
/// let (scan, files) = scan::start_with(&config, &roots, Cancel::new(), Files::new(options))?;
/// let found = dupes::group(files.recv()?, options, |_| {});
pub fn group(files: Files, options: Options, on_stage: impl FnMut(Stage)) -> Duplicates {
    group_cancellable(files, options, &Cancel::new(), on_stage)
}

/// @name group_cancellable
/// @description Like `group`, and stops within one file or one MiB of reading once `cancel` is
/// set, returning no groups. A group is only reported after a full-content hash match, or a byte
/// comparison with `verify`.
///
/// @example
/// let found = dupes::group_cancellable(files, options, &cancel, |_| {});
pub fn group_cancellable(
    files: Files,
    options: Options,
    cancel: &Cancel,
    mut on_stage: impl FnMut(Stage),
) -> Duplicates {
    let Files {
        roots,
        paths,
        mut issues,
        counted,
        started,
        ..
    } = files;
    let workers = options.workers(&roots);
    let cancelled = |issues: Vec<Issue>| Duplicates {
        groups: Vec::new(),
        issues,
        files: counted,
        duration: started.elapsed(),
    };

    let seen = seen(&paths);
    drop(paths);
    if cancel.is_cancelled() {
        return cancelled(issues);
    }

    let (small, large): (Vec<_>, Vec<_>) = same_size_candidates(seen)
        .into_iter()
        .partition(|candidate| candidate.size <= SAMPLED);

    let pass = Pass { workers, cancel };
    let Some(mut hashed) = pass.full(small, &mut issues, &mut on_stage) else {
        return cancelled(issues);
    };
    let Some(survivors) = pass.sampled(large, &mut issues, &mut on_stage) else {
        return cancelled(issues);
    };
    let Some(full) = pass.full(survivors, &mut issues, &mut on_stage) else {
        return cancelled(issues);
    };
    hashed.extend(full);

    if options.verify {
        on_stage(Stage::Verifying {
            groups: hashed.len(),
        });
        let Some(checked) = verified(hashed, workers, cancel, &mut issues) else {
            return cancelled(issues);
        };
        hashed = checked;
    }

    let mut groups: Vec<Group> = hashed
        .into_iter()
        .map(|(hash, members)| {
            let size = members[0].size;
            let allocated = members[0].allocated;
            let mut members: Vec<Member> = members
                .into_iter()
                .map(|candidate| candidate.member)
                .collect();
            members.sort_by(|a, b| a.path.cmp(&b.path));
            Group {
                size,
                allocated,
                hash: hash.to_hex().to_string(),
                verified: options.verify,
                members,
            }
        })
        .collect();
    groups.sort_by(|a, b| {
        b.reclaimable()
            .cmp(&a.reclaimable())
            .then_with(|| a.members[0].path.cmp(&b.members[0].path))
    });
    Duplicates {
        groups,
        issues,
        files: counted,
        duration: started.elapsed(),
    }
}
