use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Read};
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use crate::config::{Config, ConfigError};
use crate::platform::dir::EntryKind;
use crate::platform::{self, MountTable};
use crate::protect::Protected;
use crate::scan::walk::{Companion, Flow, Visitor, Walker};
use crate::scan::{self, Cancel, Issue, Scan};
use crate::toolchains::PinFinder;
use crate::{caches, toolchains};

const SAMPLE: u64 = 16 * 1024;
const SAMPLED: u64 = 3 * SAMPLE;
const CHUNK: usize = 1024 * 1024;
const COMPARE_CHUNK: usize = 64 * 1024;

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

#[derive(Debug, Clone)]
struct Skip {
    protected: Protected,
    tool_roots: Vec<PathBuf>,
    lifted: Vec<PathBuf>,
    last: Option<(PathBuf, bool)>,
}

impl Skip {
    fn for_user(roots: &[PathBuf]) -> Self {
        let mut tool_roots = Vec::new();
        if let Some(locations) = caches::Locations::for_user() {
            tool_roots.extend(caches::roots(&locations));
        }
        if let Some(locations) = toolchains::Locations::for_user() {
            tool_roots.extend(locations.roots());
        }
        Self::new(Protected::for_user(), tool_roots, roots)
    }

    fn new(protected: Protected, tool_roots: Vec<PathBuf>, roots: &[PathBuf]) -> Self {
        let mut skip = Self {
            protected,
            tool_roots,
            lifted: Vec::new(),
            last: None,
        };
        skip.lifted = roots
            .iter()
            .filter(|root| skip.hidden(root))
            .cloned()
            .collect();
        skip
    }

    fn hidden(&self, dir: &Path) -> bool {
        self.protected.in_hidden_home_dir(dir)
            || self.tool_roots.iter().any(|root| dir.starts_with(root))
    }

    fn skips(&mut self, path: &Path) -> bool {
        let Some(dir) = path.parent() else {
            return false;
        };
        if let Some((last, skipped)) = &self.last
            && last == dir
        {
            return *skipped;
        }
        let skipped = self.hidden(dir) && !self.lifted.iter().any(|root| dir.starts_with(root));
        self.last = Some((dir.to_path_buf(), skipped));
        skipped
    }
}

pub struct Files {
    min_size: u64,
    paths: Vec<PathBuf>,
    skip: Option<Skip>,
    issues: Vec<Issue>,
    counted: u64,
    started: Instant,
}

impl Files {
    pub fn new(options: Options) -> Self {
        Self::for_roots(options, &[])
    }

    /// @name for_roots
    /// @description Like `new`, and keeps looking inside a scan root that is itself in a hidden or
    /// tool-managed folder, even when `include_hidden` is off.
    ///
    /// @example
    /// let files = Files::for_roots(options, &[PathBuf::from("/home/me/.config/app")]);
    pub fn for_roots(options: Options, roots: &[PathBuf]) -> Self {
        Self::with_skip(
            options,
            (!options.include_hidden).then(|| Skip::for_user(roots)),
        )
    }

    fn with_skip(options: Options, skip: Option<Skip>) -> Self {
        Self {
            min_size: options.min_size,
            paths: Vec::new(),
            skip,
            issues: Vec::new(),
            counted: 0,
            started: Instant::now(),
        }
    }
}

impl Visitor for Files {
    fn wants_files(&self) -> bool {
        true
    }

    fn issue(&mut self, issue: Issue) -> Flow {
        self.issues.push(issue);
        Flow::Continue
    }

    fn counted(&mut self, kind: EntryKind) {
        if kind == EntryKind::File {
            self.counted += 1;
        }
    }

    fn file(&mut self, path: &Path, kind: EntryKind) -> Flow {
        if kind != EntryKind::File {
            return Flow::Continue;
        }
        if let Some(skip) = &mut self.skip
            && skip.skips(path)
        {
            return Flow::Continue;
        }
        self.paths.push(path.to_path_buf());
        Flow::Continue
    }
}

impl Companion for Files {
    fn fork(&self) -> Self {
        Self {
            min_size: self.min_size,
            paths: Vec::new(),
            skip: self.skip.clone(),
            issues: Vec::new(),
            counted: 0,
            started: self.started,
        }
    }

    fn merge(&mut self, other: Self) {
        self.paths.extend(other.paths);
        self.issues.extend(other.issues);
        self.counted += other.counted;
    }
}

struct Reporting<'a> {
    files: Files,
    cancel: &'a Cancel,
    on_stage: &'a mut dyn FnMut(Stage),
}

impl Visitor for Reporting<'_> {
    fn wants_files(&self) -> bool {
        true
    }

    fn cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    fn issue(&mut self, issue: Issue) -> Flow {
        self.files.issue(issue)
    }

    fn counted(&mut self, kind: EntryKind) {
        self.files.counted(kind);
        if kind == EntryKind::File && self.files.counted.is_multiple_of(4096) {
            (self.on_stage)(Stage::Walking {
                files: self.files.counted,
            });
        }
    }

    fn file(&mut self, path: &Path, kind: EntryKind) -> Flow {
        self.files.file(path, kind)
    }
}

fn stat(path: &Path, min_size: u64) -> Result<Option<Seen>, Issue> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| Issue::from_io(path.to_path_buf(), &error))?;
    let size = metadata.len();
    if size == 0 || size < min_size || !metadata.is_file() {
        return Ok(None);
    }
    Ok(Some(Seen {
        path: path.to_path_buf(),
        size,
        allocated: platform::allocated(&metadata),
        identity: platform::shared_inode(&metadata),
        modified: metadata.modified().ok(),
    }))
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

fn interrupted() -> io::Error {
    io::Error::from(io::ErrorKind::Interrupted)
}

fn advise_sequential(file: &File) {
    #[cfg(target_os = "linux")]
    let _ = rustix::fs::fadvise(file, 0, None, rustix::fs::Advice::Sequential);
    #[cfg(not(target_os = "linux"))]
    let _ = file;
}

fn sample_hash(path: &Path, size: u64) -> io::Result<blake3::Hash> {
    let file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0u8; SAMPLE as usize];
    for offset in [0, (size - SAMPLE) / 2, size - SAMPLE] {
        file.read_exact_at(&mut buffer, offset)?;
        hasher.update(&buffer);
    }
    Ok(hasher.finalize())
}

fn full_hash(path: &Path, size: u64, cancel: &Cancel) -> io::Result<blake3::Hash> {
    let mut file = File::open(path)?;
    if size > CHUNK as u64 {
        advise_sequential(&file);
    }
    let mut hasher = blake3::Hasher::new();
    let mut buffer = vec![0u8; CHUNK.min(usize::try_from(size).unwrap_or(CHUNK) + 1)];
    loop {
        if cancel.is_cancelled() {
            return Err(interrupted());
        }
        let read = file.read(&mut buffer)?;
        if read == 0 {
            return Ok(hasher.finalize());
        }
        hasher.update(&buffer[..read]);
    }
}

fn same_bytes(a: &Path, b: &Path, cancel: &Cancel) -> io::Result<bool> {
    let mut left = io::BufReader::with_capacity(COMPARE_CHUNK, File::open(a)?);
    let mut right = io::BufReader::with_capacity(COMPARE_CHUNK, File::open(b)?);
    let mut left_buf = vec![0u8; COMPARE_CHUNK];
    let mut right_buf = vec![0u8; COMPARE_CHUNK];
    loop {
        if cancel.is_cancelled() {
            return Err(interrupted());
        }
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
    cancel: &Cancel,
    work: impl Fn(&T) -> R + Sync,
) -> Option<Vec<R>> {
    if items.is_empty() {
        return Some(Vec::new());
    }
    let next = AtomicUsize::new(0);
    let mut results: Vec<(usize, R)> = thread::scope(|scope| {
        let handles: Vec<_> = (0..workers.clamp(1, items.len()))
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        if cancel.is_cancelled() {
                            break done;
                        }
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
    if cancel.is_cancelled() {
        return None;
    }
    results.sort_unstable_by_key(|(index, _)| *index);
    Some(results.into_iter().map(|(_, result)| result).collect())
}

type Hashed = Vec<(blake3::Hash, Vec<Candidate>)>;

fn bucket(
    candidates: Vec<Candidate>,
    hashes: Vec<io::Result<blake3::Hash>>,
    issues: &mut Vec<Issue>,
) -> Vec<((u64, blake3::Hash), Vec<Candidate>)> {
    let mut keyed: HashMap<(u64, blake3::Hash), Vec<Candidate>> = HashMap::new();
    for (candidate, hash) in candidates.into_iter().zip(hashes) {
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
        .collect()
}

struct Pass<'a> {
    workers: usize,
    cancel: &'a Cancel,
}

impl Pass<'_> {
    fn full(
        &self,
        mut flat: Vec<Candidate>,
        issues: &mut Vec<Issue>,
        on_stage: &mut dyn FnMut(Stage),
    ) -> Option<Hashed> {
        flat.sort_by_key(|candidate| std::cmp::Reverse(candidate.size));
        on_stage(Stage::Hashing { files: flat.len() });
        let hashes = in_parallel(&flat, self.workers, self.cancel, |candidate| {
            full_hash(&candidate.member.path, candidate.size, self.cancel)
        })?;
        Some(
            bucket(flat, hashes, issues)
                .into_iter()
                .map(|((_, hash), members)| (hash, members))
                .collect(),
        )
    }

    fn sampled(
        &self,
        flat: Vec<Candidate>,
        issues: &mut Vec<Issue>,
        on_stage: &mut dyn FnMut(Stage),
    ) -> Option<Vec<Candidate>> {
        on_stage(Stage::Hashing { files: flat.len() });
        let samples = in_parallel(&flat, self.workers, self.cancel, |candidate| {
            sample_hash(&candidate.member.path, candidate.size)
        })?;
        Some(
            bucket(flat, samples, issues)
                .into_iter()
                .flat_map(|(_, members)| members)
                .collect(),
        )
    }
}

fn split_by_bytes(
    members: Vec<Candidate>,
    cancel: &Cancel,
    issues: &mut Vec<Issue>,
) -> Vec<Vec<Candidate>> {
    let mut clusters: Vec<Vec<Candidate>> = Vec::new();
    'members: for member in members {
        for cluster in &mut clusters {
            match same_bytes(&cluster[0].member.path, &member.member.path, cancel) {
                Ok(true) => {
                    cluster.push(member);
                    continue 'members;
                }
                Ok(false) => {}
                Err(error) => {
                    if error.kind() != io::ErrorKind::Interrupted {
                        issues.push(Issue::from_io(member.member.path, &error));
                    }
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
    scan::start_with(
        config,
        roots,
        cancel,
        options.map(|options| Files::for_roots(options, roots)),
    )
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
        (
            options.map(|options| Files::for_roots(options, roots)),
            pins.then(PinFinder::new),
        ),
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
        min_size,
        paths,
        mut issues,
        counted,
        started,
        ..
    } = files;
    let workers = options.workers();
    let cancelled = |issues: Vec<Issue>| Duplicates {
        groups: Vec::new(),
        issues,
        files: counted,
        duration: started.elapsed(),
    };

    let Some(stats) = in_parallel(&paths, workers, cancel, |path| stat(path, min_size)) else {
        return cancelled(issues);
    };
    drop(paths);
    let mut seen = Vec::new();
    for stat in stats {
        match stat {
            Ok(Some(file)) => seen.push(file),
            Ok(None) => {}
            Err(issue) => issues.push(issue),
        }
    }

    seen.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    let mut by_size: HashMap<u64, Vec<Candidate>> = HashMap::new();
    for candidate in merge_links(seen) {
        by_size.entry(candidate.size).or_default().push(candidate);
    }
    let (small, large): (Vec<_>, Vec<_>) = by_size
        .into_values()
        .filter(|group| group.len() > 1)
        .flatten()
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
        hashed = hashed
            .into_iter()
            .flat_map(|(hash, members)| {
                split_by_bytes(members, cancel, &mut issues)
                    .into_iter()
                    .map(move |cluster| (hash, cluster))
                    .collect::<Vec<_>>()
            })
            .collect();
        if cancel.is_cancelled() {
            return cancelled(issues);
        }
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
    use std::fs;

    use super::*;
    use crate::scan::{self, Cancel};

    fn options() -> Options {
        Options {
            min_size: 1,
            threads: 2,
            verify: true,
            include_hidden: true,
        }
    }

    fn collect(root: &Path, options: Options) -> Files {
        let config = Config::default();
        let detector = config.detector().unwrap();
        let excludes = config.excludes().unwrap();
        let mounts = MountTable::load();
        let protected = Protected::default();
        let mut walker = Walker {
            scan: &config.scan,
            detector: &detector,
            excludes: &excludes,
            mounts: &mounts,
            protected: &protected,
            visitor: Files::with_skip(options, None),
        };
        walker.walk_root(root);
        walker.visitor
    }

    fn write(root: &Path, relative: &str, bytes: &[u8]) -> PathBuf {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn shared_scan_walk_finds_the_same_duplicates() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        for relative in ["a/one.txt", "b/two.txt", "c/three.txt"] {
            write(&root, relative, b"identical contents");
        }
        fs::write(root.join("unique.txt"), b"something else").unwrap();
        let config = Config::default();
        let options = Options {
            min_size: 1,
            threads: 2,
            verify: true,
            include_hidden: true,
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

    #[test]
    fn same_samples_with_different_middles_are_not_duplicates() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        let size = 200 * 1024;
        let base = vec![7u8; size];
        let mut changed = base.clone();
        changed[size / 4] = 9;
        write(&root, "a/one.bin", &base);
        write(&root, "b/two.bin", &base);
        write(&root, "c/three.bin", &changed);
        let options = Options {
            verify: false,
            ..options()
        };

        let found = group(collect(&root, options), options, |_| {});

        assert_eq!(found.groups.len(), 1);
        let names: Vec<_> = found.groups[0]
            .members
            .iter()
            .map(|member| member.path.strip_prefix(&root).unwrap().to_owned())
            .collect();
        assert_eq!(
            names,
            [PathBuf::from("a/one.bin"), PathBuf::from("b/two.bin")]
        );
    }

    #[test]
    fn cancelling_returns_no_groups() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        write(&root, "a/one.bin", &[1u8; 100 * 1024]);
        write(&root, "b/two.bin", &[1u8; 100 * 1024]);
        let files = collect(&root, options());
        let cancel = Cancel::new();
        cancel.cancel();

        let found = group_cancellable(files, options(), &cancel, |_| {});

        assert!(found.groups.is_empty());
    }

    #[test]
    fn hidden_home_folders_and_tool_roots_are_skipped_unless_a_root_is_inside() {
        let tmp = tempfile::TempDir::new().unwrap();
        let home = fs::canonicalize(tmp.path()).unwrap();
        let protected = Protected::new(Some(&home));
        let tool = home.join("go/pkg/mod");
        let mut skip = Skip::new(protected.clone(), vec![tool.clone()], &[]);
        assert!(skip.skips(&home.join(".cache/pip/a.whl")));
        assert!(skip.skips(&tool.join("lib/a.go")));
        assert!(!skip.skips(&home.join("dev/app/a.go")));

        let mut lifted = Skip::new(protected, vec![tool], &[home.join(".config/app")]);
        assert!(!lifted.skips(&home.join(".config/app/a.toml")));
        assert!(lifted.skips(&home.join(".config/other/a.toml")));
    }
}
