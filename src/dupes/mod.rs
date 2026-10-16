use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use crate::config::{Config, ConfigError};
use crate::detect::Listing;
use crate::platform;
use crate::protect::{self, Protected};
use crate::scan::{Issue, IssueKind};

const PARTIAL: u64 = 16 * 1024;
const CHUNK: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub struct Member {
    pub path: PathBuf,
    pub links: Vec<PathBuf>,
    pub modified: Option<SystemTime>,
}

#[derive(Debug, Clone)]
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

#[derive(Debug, Default)]
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

#[derive(Debug, Clone, Copy)]
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
}

impl Options {
    pub fn from_config(config: &Config) -> Self {
        Self {
            min_size: config.duplicates.min_size.0,
            threads: config.duplicates.threads,
            verify: config.duplicates.verify,
        }
    }

    fn workers(self) -> usize {
        match self.threads {
            0 => thread::available_parallelism().map_or(4, |n| n.get().min(8)),
            n => n,
        }
    }
}

#[derive(Debug, Clone)]
struct Seen {
    path: PathBuf,
    size: u64,
    allocated: u64,
    identity: Option<(u64, u64)>,
    modified: Option<SystemTime>,
}

#[derive(Debug, Clone)]
struct Candidate {
    size: u64,
    allocated: u64,
    member: Member,
}

struct Pending {
    path: PathBuf,
    parent: Arc<Listing>,
    depth: usize,
    device: u64,
}

struct Walk<'a> {
    config: &'a Config,
    detector: crate::detect::Detector,
    excludes: crate::config::Excludes,
    protected: Protected,
    min_size: u64,
    files: Vec<Seen>,
    issues: Vec<Issue>,
    counted: u64,
    on_stage: &'a mut dyn FnMut(Stage),
}

impl Walk<'_> {
    fn excluded_mount(&self, path: &Path) -> bool {
        self.config
            .scan
            .excluded_mounts
            .iter()
            .any(|mount| path.starts_with(mount))
    }

    fn root(&mut self, root: &Path) {
        let metadata = match fs::metadata(root) {
            Ok(metadata) if metadata.is_dir() => metadata,
            Ok(_) => {
                self.issues.push(Issue {
                    path: root.to_path_buf(),
                    kind: IssueKind::InvalidRoot,
                    message: "not a directory".to_owned(),
                });
                return;
            }
            Err(error) => {
                self.issues.push(Issue::from_io(root.to_path_buf(), &error));
                return;
            }
        };
        if self.protected.covers(root) {
            self.issues.push(Issue {
                path: root.to_path_buf(),
                kind: IssueKind::Protected,
                message: "root holds browser or application data".to_owned(),
            });
            return;
        }
        let mut stack = vec![Pending {
            path: root.to_path_buf(),
            parent: Arc::new(Listing::new(Vec::new())),
            depth: 0,
            device: platform::device(&metadata),
        }];
        while let Some(pending) = stack.pop() {
            self.visit(pending, &mut stack);
        }
    }

    fn visit(&mut self, pending: Pending, stack: &mut Vec<Pending>) {
        let entries = match fs::read_dir(&pending.path) {
            Ok(entries) => entries,
            Err(error) => {
                self.issues.push(Issue::from_io(pending.path, &error));
                return;
            }
        };
        let mut children: Vec<(OsString, fs::FileType)> = Vec::new();
        for entry in entries {
            match entry.and_then(|entry| Ok((entry.file_name(), entry.file_type()?))) {
                Ok(child) => children.push(child),
                Err(error) => self
                    .issues
                    .push(Issue::from_io(pending.path.clone(), &error)),
            }
        }
        let own = Listing::new(children.iter().map(|(name, _)| name.clone()).collect());
        let scan = &self.config.scan;
        if scan.ignores(&own) || protect::holds_app_data(&own) {
            return;
        }
        if pending.depth > 0
            && self
                .detector
                .classify_dir(&pending.path, &pending.parent, &own)
                .is_some()
        {
            return;
        }
        if scan.max_depth.is_some_and(|max| pending.depth >= max) {
            return;
        }
        let own = Arc::new(own);
        for (name, file_type) in children {
            if file_type.is_symlink() {
                continue;
            }
            let path = pending.path.join(&name);
            if file_type.is_dir() {
                if scan.skip_dirs.iter().any(|skip| name == skip.as_str())
                    || self.excludes.path(&path)
                    || self.protected.is_known(&path)
                    || self.excluded_mount(&path)
                {
                    continue;
                }
                let metadata = match fs::symlink_metadata(&path) {
                    Ok(metadata) => metadata,
                    Err(error) => {
                        self.issues.push(Issue::from_io(path, &error));
                        continue;
                    }
                };
                let device = platform::device(&metadata);
                if device != pending.device && scan.one_file_system {
                    continue;
                }
                stack.push(Pending {
                    path,
                    parent: Arc::clone(&own),
                    depth: pending.depth + 1,
                    device,
                });
            } else if file_type.is_file() {
                self.counted += 1;
                if self.counted.is_multiple_of(4096) {
                    (self.on_stage)(Stage::Walking {
                        files: self.counted,
                    });
                }
                if self.excludes.extension(&name) || self.excludes.path(&path) {
                    continue;
                }
                let metadata = match fs::symlink_metadata(&path) {
                    Ok(metadata) => metadata,
                    Err(error) => {
                        self.issues.push(Issue::from_io(path, &error));
                        continue;
                    }
                };
                let size = metadata.len();
                if size == 0 || size < self.min_size {
                    continue;
                }
                self.files.push(Seen {
                    path,
                    size,
                    allocated: platform::allocated(&metadata),
                    identity: platform::identity(&metadata),
                    modified: metadata.modified().ok(),
                });
            }
        }
    }
}

fn merge_links(files: Vec<Seen>) -> Vec<Candidate> {
    let mut by_inode: HashMap<(u64, u64), usize> = HashMap::new();
    let mut merged: Vec<Candidate> = Vec::new();
    for file in files {
        if let Some(identity) = file.identity
            && let Some(&index) = by_inode.get(&identity)
        {
            merged[index].member.links.push(file.path);
            continue;
        }
        if let Some(identity) = file.identity {
            by_inode.insert(identity, merged.len());
        }
        merged.push(Candidate {
            size: file.size,
            allocated: file.allocated,
            member: Member {
                path: file.path,
                links: Vec::new(),
                modified: file.modified,
            },
        });
    }
    for candidate in &mut merged {
        candidate.member.links.sort();
    }
    merged
}

fn hash_file(path: &Path, limit: Option<u64>) -> io::Result<blake3::Hash> {
    let file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    match limit {
        Some(limit) => io::copy(&mut file.take(limit), &mut hasher)?,
        None => io::copy(&mut io::BufReader::with_capacity(CHUNK, file), &mut hasher)?,
    };
    Ok(hasher.finalize())
}

fn same_bytes(a: &Path, b: &Path) -> io::Result<bool> {
    let mut left = io::BufReader::with_capacity(CHUNK, File::open(a)?);
    let mut right = io::BufReader::with_capacity(CHUNK, File::open(b)?);
    let mut left_buf = vec![0u8; CHUNK];
    let mut right_buf = vec![0u8; CHUNK];
    loop {
        let read = left.read(&mut left_buf)?;
        if read == 0 {
            return Ok(right.read(&mut right_buf[..1])? == 0);
        }
        match right.read_exact(&mut right_buf[..read]) {
            Ok(()) if left_buf[..read] == right_buf[..read] => {}
            Ok(()) => return Ok(false),
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(false),
            Err(error) => return Err(error),
        }
    }
}

fn in_parallel<T: Sync, R: Send>(
    items: &[T],
    workers: usize,
    work: impl Fn(&T) -> R + Sync,
) -> Vec<R> {
    if items.is_empty() {
        return Vec::new();
    }
    let next = AtomicUsize::new(0);
    let mut results: Vec<(usize, R)> = thread::scope(|scope| {
        let handles: Vec<_> = (0..workers.clamp(1, items.len()))
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(item) = items.get(index) else {
                            break done;
                        };
                        done.push((index, work(item)));
                    }
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("hashing thread panicked"))
            .collect()
    });
    results.sort_unstable_by_key(|(index, _)| *index);
    results.into_iter().map(|(_, result)| result).collect()
}

fn regroup(
    groups: Vec<Vec<Candidate>>,
    workers: usize,
    limit: Option<u64>,
    issues: &mut Vec<Issue>,
    on_stage: &mut dyn FnMut(Stage),
) -> Vec<(blake3::Hash, Vec<Candidate>)> {
    let flat: Vec<Candidate> = groups.into_iter().flatten().collect();
    on_stage(Stage::Hashing { files: flat.len() });
    let hashes = in_parallel(&flat, workers, |candidate| {
        let limit = limit.filter(|&limit| candidate.size > limit);
        hash_file(&candidate.member.path, limit)
    });
    let mut keyed: HashMap<(u64, blake3::Hash), Vec<Candidate>> = HashMap::new();
    for (candidate, hash) in flat.into_iter().zip(hashes) {
        match hash {
            Ok(hash) => keyed
                .entry((candidate.size, hash))
                .or_default()
                .push(candidate),
            Err(error) => issues.push(Issue::from_io(candidate.member.path, &error)),
        }
    }
    keyed
        .into_iter()
        .filter(|(_, members)| members.len() > 1)
        .map(|((_, hash), members)| (hash, members))
        .collect()
}

fn split_by_bytes(members: Vec<Candidate>, issues: &mut Vec<Issue>) -> Vec<Vec<Candidate>> {
    let mut clusters: Vec<Vec<Candidate>> = Vec::new();
    'members: for member in members {
        for cluster in &mut clusters {
            match same_bytes(&cluster[0].member.path, &member.member.path) {
                Ok(true) => {
                    cluster.push(member);
                    continue 'members;
                }
                Ok(false) => {}
                Err(error) => {
                    issues.push(Issue::from_io(member.member.path, &error));
                    continue 'members;
                }
            }
        }
        clusters.push(vec![member]);
    }
    clusters
        .into_iter()
        .filter(|cluster| cluster.len() > 1)
        .collect()
}

/// @name find
/// @description Finds files with identical contents under `roots`. Files are grouped by size,
/// then by a BLAKE3 hash of their first 16 KiB, then by a full hash, and with `verify` by a byte
/// comparison. Hard links to one inode count as a single member. Empty files, symlinks,
/// generated artifact directories and browser or credential data are never considered.
///
/// @example
/// let found = dupes::find(&config, &roots, Options::from_config(&config), |_| {})?;
/// println!("{} reclaimable", found.reclaimable());
pub fn find(
    config: &Config,
    roots: &[PathBuf],
    options: Options,
    mut on_stage: impl FnMut(Stage),
) -> Result<Duplicates, ConfigError> {
    let started = Instant::now();
    let mut walk = Walk {
        config,
        detector: config.detector()?,
        excludes: config.excludes()?,
        protected: Protected::for_user(),
        min_size: options.min_size,
        files: Vec::new(),
        issues: Vec::new(),
        counted: 0,
        on_stage: &mut on_stage,
    };
    for root in roots {
        walk.root(root);
    }
    let Walk {
        files,
        mut issues,
        counted,
        ..
    } = walk;

    let mut by_size: HashMap<u64, Vec<Candidate>> = HashMap::new();
    for candidate in merge_links(files) {
        by_size.entry(candidate.size).or_default().push(candidate);
    }
    let sized: Vec<Vec<Candidate>> = by_size
        .into_values()
        .filter(|group| group.len() > 1)
        .collect();

    let workers = options.workers();
    let partial = regroup(sized, workers, Some(PARTIAL), &mut issues, &mut on_stage);
    let (small, large): (Vec<_>, Vec<_>) = partial
        .into_iter()
        .partition(|(_, members)| members[0].size <= PARTIAL);
    let full = regroup(
        large.into_iter().map(|(_, members)| members).collect(),
        workers,
        None,
        &mut issues,
        &mut on_stage,
    );

    let mut hashed: Vec<(blake3::Hash, Vec<Candidate>)> = small.into_iter().chain(full).collect();
    if options.verify {
        on_stage(Stage::Verifying {
            groups: hashed.len(),
        });
        hashed = hashed
            .into_iter()
            .flat_map(|(hash, members)| {
                split_by_bytes(members, &mut issues)
                    .into_iter()
                    .map(move |cluster| (hash, cluster))
                    .collect::<Vec<_>>()
            })
            .collect();
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
    Ok(Duplicates {
        groups,
        issues,
        files: counted,
        duration: started.elapsed(),
    })
}
