use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use crate::config::{Config, ConfigError};
use crate::platform::{self, MountTable};
use crate::protect::Protected;
use crate::scan::walk::{Flow, Visitor, Walker};
use crate::scan::{self, Cancel, Issue, Scan};
use crate::toolchains::PinFinder;

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

pub struct Files {
    min_size: u64,
    seen: Vec<Seen>,
    issues: Vec<Issue>,
    counted: u64,
    started: Instant,
}

impl Files {
    pub fn new(options: Options) -> Self {
        Self {
            min_size: options.min_size,
            seen: Vec::new(),
            issues: Vec::new(),
            counted: 0,
            started: Instant::now(),
        }
    }
}

impl Visitor for Files {
    fn issue(&mut self, issue: Issue) -> Flow {
        self.issues.push(issue);
        Flow::Continue
    }

    fn counted(&mut self, file_type: fs::FileType) {
        if file_type.is_file() {
            self.counted += 1;
        }
    }

    fn file(&mut self, path: &Path, file_type: fs::FileType) -> Flow {
        if !file_type.is_file() {
            return Flow::Continue;
        }
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) => return self.issue(Issue::from_io(path.to_path_buf(), &error)),
        };
        let size = metadata.len();
        if size == 0 || size < self.min_size {
            return Flow::Continue;
        }
        self.seen.push(Seen {
            path: path.to_path_buf(),
            size,
            allocated: platform::allocated(&metadata),
            identity: platform::identity(&metadata),
            modified: metadata.modified().ok(),
        });
        Flow::Continue
    }
}

struct Reporting<'a> {
    files: Files,
    on_stage: &'a mut dyn FnMut(Stage),
}

impl Visitor for Reporting<'_> {
    fn issue(&mut self, issue: Issue) -> Flow {
        self.files.issue(issue)
    }

    fn counted(&mut self, file_type: fs::FileType) {
        self.files.counted(file_type);
        if file_type.is_file() && self.files.counted.is_multiple_of(4096) {
            (self.on_stage)(Stage::Walking {
                files: self.files.counted,
            });
        }
    }

    fn file(&mut self, path: &Path, file_type: fs::FileType) -> Flow {
        self.files.file(path, file_type)
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
            files: Files::new(options),
            on_stage: &mut on_stage,
        },
    };
    for root in roots {
        walker.walk_root(root);
    }
    let files = walker.visitor.files;
    Ok(group(files, options, on_stage))
}

/// @name start_scan
/// @description Starts an artifact scan that also collects files for duplicate detection during
/// the same walk, like the dashboard does. Pass `None` to skip duplicates. The returned receiver
/// yields the collected files once the walk ends; hand them to `group`.
///
/// @example
/// let (scan, files) = dupes::start_scan(&config, &roots, Cancel::new(), Some(options))?;
/// let collected = scan::collect(&scan, |_| {});
/// let found = files.recv().ok().flatten().map(|files| dupes::group(files, options, |_| {}));
pub fn start_scan(
    config: &Config,
    roots: &[PathBuf],
    cancel: Cancel,
    options: Option<Options>,
) -> Result<(Scan, crossbeam_channel::Receiver<Option<Files>>), ConfigError> {
    scan::start_with(config, roots, cancel, options.map(Files::new))
}

/// What a walk collected next to the artifacts: files for duplicate detection and toolchain pins.
pub type Collected = (Option<Files>, Option<PinFinder>);

/// @name start_scan_with_pins
/// @description Like `start_scan`, and with `pins` set also records toolchain pin files
/// (`.nvmrc`, `rust-toolchain.toml` and the like) during the same walk. The receiver yields the
/// collected files and pins once the walk ends.
///
/// @example
/// let (scan, found) = dupes::start_scan_with_pins(&config, &roots, Cancel::new(), None, true)?;
/// let (_, pins) = found.recv()?;
pub fn start_scan_with_pins(
    config: &Config,
    roots: &[PathBuf],
    cancel: Cancel,
    options: Option<Options>,
    pins: bool,
) -> Result<(Scan, crossbeam_channel::Receiver<Collected>), ConfigError> {
    scan::start_with(
        config,
        roots,
        cancel,
        (options.map(Files::new), pins.then(PinFinder::new)),
    )
}

/// @name group
/// @description Groups files collected during a walk into sets of identical contents, the same
/// way `find` does after its own walk. Lets a caller that already walks the tree, such as the
/// dashboard, collect `Files` alongside that walk instead of walking twice.
///
/// @example
/// let (scan, files) = scan::start_with(&config, &roots, Cancel::new(), Files::new(options))?;
/// let found = dupes::group(files.recv()?, options, |_| {});
pub fn group(files: Files, options: Options, mut on_stage: impl FnMut(Stage)) -> Duplicates {
    let Files {
        seen,
        mut issues,
        counted,
        started,
        ..
    } = files;

    let mut by_size: HashMap<u64, Vec<Candidate>> = HashMap::new();
    for candidate in merge_links(seen) {
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
    Duplicates {
        groups,
        issues,
        files: counted,
        duration: started.elapsed(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::{self, Cancel};

    #[test]
    fn shared_scan_walk_finds_the_same_duplicates() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        for relative in ["a/one.txt", "b/two.txt", "c/three.txt"] {
            let path = root.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"identical contents").unwrap();
        }
        fs::write(root.join("unique.txt"), b"something else").unwrap();
        let config = Config::default();
        let options = Options {
            min_size: 1,
            threads: 2,
            verify: true,
        };
        let roots = vec![root];

        let alone = find(&config, &roots, options, |_| {}).unwrap();
        let (handle, files) =
            scan::start_with(&config, &roots, Cancel::new(), Files::new(options)).unwrap();
        scan::collect(&handle, |_| {});
        let shared = group(files.recv().unwrap(), options, |_| {});

        let members = |found: &Duplicates| -> Vec<Vec<PathBuf>> {
            found
                .groups
                .iter()
                .map(|group| group.members.iter().map(|m| m.path.clone()).collect())
                .collect()
        };
        assert_eq!(members(&alone).len(), 1);
        assert_eq!(members(&alone), members(&shared));
        assert_eq!(alone.files, shared.files);
    }
}
