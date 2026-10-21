use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs;
use std::hash::{BuildHasherDefault, Hasher};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::cancel::Cancel;
use super::pool::{Local, Queue};
use super::types::Usage;
use crate::platform;
use crate::platform::dir::{Buffer, Dir, Stat, stat_path};

pub struct Measured {
    pub usage: Usage,
    pub newest: Option<SystemTime>,
    pub identity: Option<(u64, u64)>,
}

fn newer(current: Option<SystemTime>, metadata: &fs::Metadata) -> Option<SystemTime> {
    match (current, metadata.modified().ok()) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

pub fn measure(path: &Path, cancel: &Cancel) -> Measured {
    let mut usage = Usage::default();
    let Ok(root) = fs::symlink_metadata(path) else {
        usage.partial = true;
        return Measured {
            usage,
            newest: None,
            identity: None,
        };
    };
    let device = platform::device(&root);
    let identity = platform::identity(&root);
    let mut newest = newer(None, &root);
    let mut seen: HashSet<(u64, u64)> = HashSet::new();
    usage.allocated = platform::allocated(&root);
    usage.apparent = root.len();
    if !root.is_dir() {
        usage.entries = 1;
        return Measured {
            usage,
            newest,
            identity,
        };
    }

    let mut stack: Vec<PathBuf> = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if cancel.is_cancelled() {
            usage.partial = true;
            break;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            usage.partial = true;
            continue;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                usage.partial = true;
                continue;
            };
            let Ok(metadata) = entry.metadata() else {
                usage.partial = true;
                continue;
            };
            if metadata.is_dir() && platform::device(&metadata) != device {
                usage.partial = true;
                continue;
            }
            usage.entries += 1;
            newest = newer(newest, &metadata);
            if platform::shared_inode(&metadata).is_some_and(|key| !seen.insert(key)) {
                continue;
            }
            usage.allocated += platform::allocated(&metadata);
            usage.apparent += metadata.len();
            if metadata.is_dir() {
                stack.push(entry.path());
            }
        }
    }
    Measured {
        usage,
        newest,
        identity,
    }
}

const CHUNK: usize = 1024;
const LINK_SHARDS: usize = 16;

#[derive(Default)]
struct Fx(u64);

impl Hasher for Fx {
    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.write_u64(u64::from(*byte));
        }
    }

    fn write_u64(&mut self, value: u64) {
        self.0 = (self.0.rotate_left(5) ^ value).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

type LinkSet = HashSet<(u64, u64), BuildHasherDefault<Fx>>;

fn encode(time: SystemTime) -> u64 {
    let nanos = match time.duration_since(UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_nanos()).unwrap_or(i64::MAX),
        Err(before) => i64::try_from(before.duration().as_nanos()).map_or(i64::MIN, |n| -n),
    };
    (nanos as u64 ^ (1 << 63)).max(1)
}

fn decode(value: u64) -> Option<SystemTime> {
    if value == 0 {
        return None;
    }
    let nanos = (value ^ (1 << 63)) as i64;
    if nanos >= 0 {
        UNIX_EPOCH.checked_add(Duration::from_nanos(nanos.unsigned_abs()))
    } else {
        UNIX_EPOCH.checked_sub(Duration::from_nanos(nanos.unsigned_abs()))
    }
}

/// The running total of one item measured by many threads at once. Every task that adds to it
/// holds one count in `outstanding`; the task that drops it to zero owns the finished result.
pub(crate) struct Total<T> {
    pub item: T,
    device: u64,
    identity: Option<(u64, u64)>,
    allocated: AtomicU64,
    apparent: AtomicU64,
    entries: AtomicU64,
    newest: AtomicU64,
    partial: AtomicBool,
    outstanding: AtomicUsize,
    links: OnceLock<Box<[Mutex<LinkSet>]>>,
}

impl<T> Total<T> {
    fn new(item: T, root: &Stat) -> Self {
        Self {
            item,
            device: root.device,
            identity: Some(root.identity()),
            allocated: AtomicU64::new(root.allocated),
            apparent: AtomicU64::new(root.len),
            entries: AtomicU64::new(0),
            newest: AtomicU64::new(root.modified.map_or(0, encode)),
            partial: AtomicBool::new(false),
            outstanding: AtomicUsize::new(1),
            links: OnceLock::new(),
        }
    }

    fn first_link(&self, key: (u64, u64)) -> bool {
        let shards = self.links.get_or_init(|| {
            (0..LINK_SHARDS)
                .map(|_| Mutex::new(LinkSet::default()))
                .collect()
        });
        shards[(key.1 as usize) % LINK_SHARDS]
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .insert(key)
    }

    pub fn measured(&self) -> Measured {
        Measured {
            usage: Usage {
                allocated: self.allocated.load(Ordering::Acquire),
                apparent: self.apparent.load(Ordering::Acquire),
                entries: self.entries.load(Ordering::Acquire),
                partial: self.partial.load(Ordering::Acquire),
            },
            newest: decode(self.newest.load(Ordering::Acquire)),
            identity: self.identity,
        }
    }
}

/// How a measurement starts: the root's own stat, and whether there is a tree below it.
pub(crate) enum Start<T> {
    Done(T, Measured),
    Tree(Arc<Total<T>>, PathBuf),
}

/// @name start_measure
/// @description Stats the root of an item the way [`measure`] does. A file or an unreadable
/// root is measured right away; a directory comes back as a total to fill with
/// [`SizeTask`]s.
///
/// @example
/// match start_measure(found, &path) { Start::Done(item, measured) => emit(item, measured), Start::Tree(total, path) => local.push(SizeTask::dir(total, path)) }
pub(crate) fn start_measure<T>(item: T, path: &Path) -> Start<T> {
    match stat_path(path) {
        Ok(root) if root.kind.is_dir() => {
            Start::Tree(Arc::new(Total::new(item, &root)), path.to_path_buf())
        }
        Ok(root) => Start::Done(
            item,
            Measured {
                usage: Usage {
                    allocated: root.allocated,
                    apparent: root.len,
                    entries: 1,
                    partial: false,
                },
                newest: root.modified,
                identity: Some(root.identity()),
            },
        ),
        Err(_) => Start::Done(
            item,
            Measured {
                usage: Usage {
                    partial: true,
                    ..Usage::default()
                },
                newest: None,
                identity: None,
            },
        ),
    }
}

pub(crate) struct SizeTask<T> {
    total: Arc<Total<T>>,
    dir: Arc<Path>,
    names: Option<Box<[u8]>>,
}

impl<T> SizeTask<T> {
    pub fn dir(total: Arc<Total<T>>, path: PathBuf) -> Self {
        Self {
            total,
            dir: Arc::from(path),
            names: None,
        }
    }
}

/// Per-thread buffers for sizing.
#[derive(Default)]
pub(crate) struct SizeScratch {
    buffer: Buffer,
    names: Vec<u8>,
    entries: Vec<(u64, usize, usize)>,
}

#[derive(Default)]
struct Sum {
    allocated: u64,
    apparent: u64,
    entries: u64,
    newest: u64,
    partial: bool,
}

impl Sum {
    fn add<T>(
        &mut self,
        total: &Total<T>,
        stat: &Stat,
        dir: &Path,
        name: &OsStr,
    ) -> Option<PathBuf> {
        if stat.kind.is_dir() && stat.device != total.device {
            self.partial = true;
            return None;
        }
        self.entries += 1;
        self.newest = self.newest.max(stat.modified.map_or(0, encode));
        if stat
            .shared_inode()
            .is_some_and(|key| !total.first_link(key))
        {
            return None;
        }
        self.allocated += stat.allocated;
        self.apparent += stat.len;
        stat.kind.is_dir().then(|| dir.join(name))
    }

    fn flush<T>(self, total: &Total<T>) {
        total.allocated.fetch_add(self.allocated, Ordering::Relaxed);
        total.apparent.fetch_add(self.apparent, Ordering::Relaxed);
        total.entries.fetch_add(self.entries, Ordering::Relaxed);
        total.newest.fetch_max(self.newest, Ordering::Relaxed);
        if self.partial {
            total.partial.store(true, Ordering::Relaxed);
        }
    }
}

fn split_names(names: &[u8]) -> impl Iterator<Item = &OsStr> {
    names
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
        .map(OsStr::from_bytes)
}

/// @name run_size_task
/// @description Measures one directory, or one chunk of names in a directory, of a total.
/// Subdirectories and the chunks of a very large directory become new tasks on `local`, so
/// other threads take part. Returns the total when this task was the last one it waited for.
///
/// @example
/// if let Some(total) = run_size_task(task, &mut scratch, cancel, &mut local, Task::Size) { emit(total); }
pub(crate) fn run_size_task<T, Q>(
    task: SizeTask<T>,
    scratch: &mut SizeScratch,
    cancel: &Cancel,
    local: &mut Local<'_, Q>,
    wrap: impl Fn(SizeTask<T>) -> Q,
) -> Option<Arc<Total<T>>> {
    let SizeTask { total, dir, names } = task;
    let mut sum = Sum::default();
    let spawn = |path: PathBuf, local: &mut Local<'_, Q>| {
        total.outstanding.fetch_add(1, Ordering::Relaxed);
        local.push(wrap(SizeTask::dir(Arc::clone(&total), path)));
    };
    if cancel.is_cancelled() {
        sum.partial = true;
    } else {
        match Dir::open(&dir, false) {
            Err(_) => sum.partial = true,
            Ok(open) => match names {
                Some(names) => {
                    for name in split_names(&names) {
                        match open.stat_at(name) {
                            Ok(stat) => {
                                if let Some(path) = sum.add(&total, &stat, &dir, name) {
                                    spawn(path, local);
                                }
                            }
                            Err(_) => sum.partial = true,
                        }
                    }
                }
                None => {
                    let SizeScratch {
                        buffer,
                        names,
                        entries,
                    } = scratch;
                    names.clear();
                    entries.clear();
                    if open
                        .read(buffer, |name, _, inode| {
                            let start = names.len();
                            names.extend_from_slice(name.as_bytes());
                            entries.push((inode, start, names.len()));
                            names.push(0);
                        })
                        .is_err()
                    {
                        sum.partial = true;
                    }
                    entries.sort_unstable_by_key(|(inode, _, _)| *inode);
                    let mut chunks = entries.chunks(CHUNK);
                    let first = chunks.next().unwrap_or(&[]);
                    for chunk in chunks {
                        let mut bytes = Vec::with_capacity(chunk.len() * 16);
                        for (_, start, end) in chunk {
                            bytes.extend_from_slice(&names[*start..=*end]);
                        }
                        total.outstanding.fetch_add(1, Ordering::Relaxed);
                        local.push(wrap(SizeTask {
                            total: Arc::clone(&total),
                            dir: Arc::clone(&dir),
                            names: Some(bytes.into_boxed_slice()),
                        }));
                    }
                    for (_, start, end) in first {
                        let name = OsStr::from_bytes(&names[*start..*end]);
                        match open.stat_at(name) {
                            Ok(stat) => {
                                if let Some(path) = sum.add(&total, &stat, &dir, name) {
                                    spawn(path, local);
                                }
                            }
                            Err(_) => sum.partial = true,
                        }
                    }
                }
            },
        }
    }
    sum.flush(&total);
    (total.outstanding.fetch_sub(1, Ordering::AcqRel) == 1).then_some(total)
}

/// @name measure_parallel
/// @description Measures `path` like [`measure`], with the same totals, but spreads the
/// directories and the entries of very large directories over `threads` threads. Meant for a
/// single large root such as a cache or a toolchain folder.
///
/// @example
/// let measured = scan::measure_parallel(&cache, &Cancel::new(), 8);
pub fn measure_parallel(path: &Path, cancel: &Cancel, threads: usize) -> Measured {
    let (total, root) = match start_measure((), path) {
        Start::Done((), measured) => return measured,
        Start::Tree(total, root) => (total, root),
    };
    let queue = Queue::new(vec![SizeTask::dir(Arc::clone(&total), root)]);
    thread::scope(|scope| {
        for _ in 0..threads.max(1) {
            scope.spawn(|| {
                let mut local = queue.local();
                let mut scratch = SizeScratch::default();
                while let Some(task) = local.next() {
                    run_size_task(task, &mut scratch, cancel, &mut local, |task| task);
                    local.done();
                }
            });
        }
    });
    if cancel.is_cancelled() {
        total.partial.store(true, Ordering::Relaxed);
    }
    total.measured()
}
