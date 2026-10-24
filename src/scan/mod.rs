mod activity;
mod cancel;
mod pool;
mod size;
mod types;
pub(crate) mod walk;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, bounded};

pub use activity::{ActivityScope, annotate, annotate_in};
pub use cancel::Cancel;
pub use size::{Measured, measure, measure_many, measure_parallel};
pub use types::{Candidate, Counters, Event, Issue, IssueKind, Progress, Summary, Usage};

use crate::config::{Config, ConfigError};
use crate::platform::dir::EntryKind;
use crate::platform::{self, MountTable};
use crate::protect::Protected;
use pool::Queue;
use size::{SizeScratch, SizeTask, Start, run_size_task, start_measure};
use walk::{Companion, Context, FileEntry, Flow, Found, Pending, Scratch, Visitor};

const EVENT_CAPACITY: usize = 1024;
const TICK: Duration = Duration::from_millis(100);
const CURRENT_EVERY: u64 = 32;
const FLUSH_EVERY: u64 = 256;
const MAX_SCAN_THREADS: usize = 16;
const MAX_CONFIGURED_THREADS: usize = MAX_SCAN_THREADS * 4;
const ROTATIONAL_THREADS: usize = 2;

pub struct Scan {
    pub events: Receiver<Event>,
    counters: Arc<Counters>,
    current: Arc<Mutex<Option<PathBuf>>>,
    cancel: Cancel,
    coordinator: Option<JoinHandle<()>>,
}

impl Scan {
    pub fn progress(&self) -> Progress {
        self.counters.snapshot()
    }

    /// @name Scan::current
    /// @description Returns a folder the walker entered recently, refreshed every few dozen
    /// folders, so a frontend can show where the scan is without paying for a path per folder.
    /// `None` until the walker entered its first folder.
    ///
    /// @example
    /// if let Some(folder) = handle.current() { println!("in {}", folder.display()); }
    pub fn current(&self) -> Option<PathBuf> {
        self.current.lock().ok().and_then(|current| current.clone())
    }

    pub fn cancel(&self) {
        self.cancel.cancel();
    }
}

impl Drop for Scan {
    fn drop(&mut self) {
        self.cancel.cancel();
        while self.events.try_recv().is_ok() {}
        if let Some(handle) = self.coordinator.take() {
            drop(std::mem::replace(
                &mut self.events,
                crossbeam_channel::never(),
            ));
            let _ = handle.join();
        }
    }
}

pub fn normalize_roots(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut resolved: Vec<PathBuf> = roots
        .iter()
        .map(|root| std::fs::canonicalize(root).unwrap_or_else(|_| root.clone()))
        .collect();
    resolved.sort();
    resolved.dedup();
    let mut kept: Vec<PathBuf> = Vec::with_capacity(resolved.len());
    for root in resolved {
        if !kept.iter().any(|parent| root.starts_with(parent)) {
            kept.push(root);
        }
    }
    kept
}

struct Shared {
    cancel: Cancel,
    counters: Arc<Counters>,
    current: Arc<Mutex<Option<PathBuf>>>,
    events: Sender<Event>,
    next_id: AtomicU64,
    min_size: u64,
}

impl Shared {
    fn deliver(&self, found: &Found, measured: Measured) -> Flow {
        if self.cancel.is_cancelled() || measured.usage.allocated < self.min_size {
            return Flow::Continue;
        }
        Counters::add(&self.counters.candidates, 1);
        Counters::add(&self.counters.bytes, measured.usage.allocated);
        let candidate = Candidate {
            id: self.next_id.fetch_add(1, Ordering::Relaxed),
            project: parent_project(&found.path),
            path: found.path.clone(),
            kind: found.kind,
            rule: Arc::clone(&found.rule),
            usage: measured.usage,
            newest: measured.newest,
            identity: measured.identity,
            project_last_active: None,
        };
        match self.events.send(Event::Candidate(candidate)) {
            Ok(()) => Flow::Continue,
            Err(_) => {
                self.cancel.cancel();
                Flow::Stop
            }
        }
    }
}

struct Sink<'a, C> {
    shared: &'a Shared,
    found: Vec<Found>,
    entered: u64,
    dirs: u64,
    files: u64,
    companion: C,
}

impl<'a, C> Sink<'a, C> {
    fn new(shared: &'a Shared, companion: C) -> Self {
        Self {
            shared,
            found: Vec::new(),
            entered: 0,
            dirs: 0,
            files: 0,
            companion,
        }
    }

    fn flush(&mut self) {
        Counters::add(&self.shared.counters.dirs, std::mem::take(&mut self.dirs));
        Counters::add(&self.shared.counters.files, std::mem::take(&mut self.files));
    }
}

impl<C: Visitor> Visitor for Sink<'_, C> {
    fn cancelled(&self) -> bool {
        self.shared.cancel.is_cancelled()
    }

    fn wants_files(&self) -> bool {
        self.companion.wants_files()
    }

    fn issue(&mut self, issue: Issue) -> Flow {
        Counters::add(&self.shared.counters.issues, 1);
        match self.shared.events.send(Event::Issue(issue)) {
            Ok(()) => Flow::Continue,
            Err(_) => Flow::Stop,
        }
    }

    fn entered(&mut self, path: &Path) {
        if self.entered.is_multiple_of(CURRENT_EVERY)
            && let Ok(mut current) = self.shared.current.lock()
        {
            *current = Some(path.to_path_buf());
        }
        self.entered += 1;
        self.dirs += 1;
        if self.dirs >= FLUSH_EVERY {
            self.flush();
        }
    }

    fn counted(&mut self, kind: EntryKind) {
        self.files += 1;
        self.companion.counted(kind);
    }

    fn file(&mut self, path: &Path, kind: EntryKind) -> Flow {
        self.companion.file(path, kind)
    }

    fn entry(&mut self, entry: &FileEntry<'_>) -> Flow {
        self.companion.entry(entry)
    }

    fn repository(&mut self, root: &Path) {
        self.companion.repository(root);
    }

    fn artifact(&mut self, found: Found) -> Flow {
        self.found.push(found);
        Flow::Continue
    }
}

/// @name size_threads
/// @description Returns how many threads size caches, toolchains and other single roots.
/// `scan.size_threads` overrides the default and is capped at 64.
///
/// @example
/// let threads = scan::size_threads(&config);
pub fn size_threads(config: &Config) -> usize {
    match config.scan.size_threads {
        0 => thread::available_parallelism().map_or(4, |n| n.get().min(8)),
        n => n.min(MAX_CONFIGURED_THREADS),
    }
}

/// @name scan_threads
/// @description Returns how many threads walk and size a scan of `roots`. Listing directories
/// waits on the disk more than on the CPU, so the default runs about twice as many threads as
/// cores, and only a couple when any root is on a spinning disk where more would add seeks.
/// `scan.size_threads` overrides it and is capped at 64.
///
/// @example
/// let threads = scan::scan_threads(&config, &roots);
pub fn scan_threads(config: &Config, roots: &[PathBuf]) -> usize {
    if config.scan.size_threads > 0 {
        return config.scan.size_threads.min(MAX_CONFIGURED_THREADS);
    }
    if roots.iter().any(|root| platform::rotational_at(root)) {
        return ROTATIONAL_THREADS;
    }
    thread::available_parallelism().map_or(8, |n| (n.get() * 2).min(MAX_SCAN_THREADS))
}

fn parent_project(path: &Path) -> Option<PathBuf> {
    path.parent().map(Path::to_path_buf)
}

pub fn start(config: &Config, roots: &[PathBuf], cancel: Cancel) -> Result<Scan, ConfigError> {
    start_with(config, roots, cancel, ()).map(|(scan, _)| scan)
}

enum Task {
    Walk(Pending),
    Size(SizeTask<Found>),
}

fn work<C: Companion>(
    shared: &Shared,
    context: &Context<'_>,
    queue: &Queue<Task>,
    companion: C,
) -> C {
    let mut local = queue.local();
    let mut sink = Sink::new(shared, companion);
    let mut scratch = Scratch::default();
    let mut sizing = SizeScratch::default();
    let mut children = Vec::new();
    while let Some(task) = local.next() {
        if shared.cancel.is_cancelled() {
            queue.stop();
            break;
        }
        let flow = match task {
            Task::Walk(pending) => {
                let flow = context.visit(&mut sink, pending, &mut scratch, &mut children);
                for child in children.drain(..) {
                    local.push(Task::Walk(child));
                }
                let mut flow = flow;
                for found in std::mem::take(&mut sink.found) {
                    let path = found.path.clone();
                    match start_measure(found, &path) {
                        Start::Done(found, measured) => {
                            if let Flow::Stop = shared.deliver(&found, measured) {
                                flow = Flow::Stop;
                            }
                        }
                        Start::Tree(total, path) => {
                            local.push(Task::Size(SizeTask::dir(total, path)));
                        }
                    }
                }
                flow
            }
            Task::Size(task) => {
                match run_size_task(task, &mut sizing, &shared.cancel, &mut local, Task::Size) {
                    Some(total) => shared.deliver(&total.item, total.measured()),
                    None => Flow::Continue,
                }
            }
        };
        local.done();
        if let Flow::Stop = flow {
            queue.stop();
            break;
        }
    }
    sink.flush();
    sink.companion
}

pub(crate) fn start_with<C: Companion>(
    config: &Config,
    roots: &[PathBuf],
    cancel: Cancel,
    companion: C,
) -> Result<(Scan, Receiver<C>), ConfigError> {
    let detector = config.detector()?;
    let excludes = config.excludes()?;
    let scan_config = config.scan.clone();
    let roots = normalize_roots(roots);
    let threads = scan_threads(config, &roots);
    let counters = Arc::new(Counters::default());
    let current = Arc::new(Mutex::new(None));
    let (event_tx, event_rx) = bounded::<Event>(EVENT_CAPACITY);
    let started = Instant::now();
    let (companion_tx, companion_rx) = bounded::<C>(1);
    let shared = Shared {
        cancel: cancel.clone(),
        counters: Arc::clone(&counters),
        current: Arc::clone(&current),
        events: event_tx,
        next_id: AtomicU64::new(1),
        min_size: config.scan.min_size.0,
    };

    let coordinator = thread::Builder::new()
        .name("kuizer-scan".to_owned())
        .spawn(move || {
            let mounts = MountTable::load();
            let protected = Protected::for_user();
            let context = Context::new(&scan_config, &detector, &excludes, &mounts, &protected);
            let mut companion = companion;
            let mut seeds = Vec::new();
            {
                let mut sink = Sink::new(&shared, ());
                for root in &roots {
                    seeds.extend(context.root(&mut sink, root).map(Task::Walk));
                }
            }
            if !seeds.is_empty() && !shared.cancel.is_cancelled() {
                let queue = Queue::new(seeds);
                let forks: Vec<C> = (0..threads).map(|_| companion.fork()).collect();
                let done: Vec<C> = thread::scope(|scope| {
                    let workers: Vec<_> = forks
                        .into_iter()
                        .enumerate()
                        .filter_map(|(index, fork)| {
                            let (shared, context, queue) = (&shared, &context, &queue);
                            thread::Builder::new()
                                .name(format!("kuizer-scan-{index}"))
                                .spawn_scoped(scope, move || work(shared, context, queue, fork))
                                .ok()
                        })
                        .collect();
                    if workers.is_empty() {
                        return vec![work(&shared, &context, &queue, companion.fork())];
                    }
                    workers
                        .into_iter()
                        .filter_map(|worker| worker.join().ok())
                        .collect()
                });
                for fork in done {
                    companion.merge(fork);
                }
            }
            let _ = companion_tx.send(companion);
            let summary = Summary {
                progress: shared.counters.snapshot(),
                duration: started.elapsed(),
                cancelled: shared.cancel.is_cancelled(),
            };
            let _ = shared.events.send(Event::Finished(summary));
        })
        .expect("spawn scan thread");

    Ok((
        Scan {
            events: event_rx,
            counters,
            current,
            cancel,
            coordinator: Some(coordinator),
        },
        companion_rx,
    ))
}

pub struct Collected {
    pub candidates: Vec<Candidate>,
    pub issues: Vec<Issue>,
    pub summary: Summary,
}

/// @name collect
/// @description Drains a scan until it finishes and returns everything it found. `on_tick`
/// gets the counters about every 100ms, also while results keep streaming in, and once more
/// with the final counters when the scan is done.
///
/// @example
/// let collected = scan::collect(&handle, |progress| output::print_progress(progress));
pub fn collect(scan: &Scan, on_tick: impl FnMut(Progress)) -> Collected {
    collect_with(scan, on_tick, |_| {})
}

/// @name collect_with
/// @description Like [`collect`], and lends every candidate to `on_found` as it arrives.
///
/// @example
/// let collected = scan::collect_with(&handle, |_| {}, |candidate| show(candidate));
pub fn collect_with(
    scan: &Scan,
    on_tick: impl FnMut(Progress),
    mut on_found: impl FnMut(&Candidate),
) -> Collected {
    let mut candidates = Vec::new();
    let (issues, summary) = collect_streaming(scan, on_tick, |candidate| {
        on_found(&candidate);
        candidates.push(candidate);
    });
    Collected {
        candidates,
        issues,
        summary,
    }
}

/// @name collect_streaming
/// @description Drains a scan until it finishes and hands every candidate to `on_found` by
/// value, keeping none. Returns the issues and the summary. For a frontend that stores the
/// candidates itself and would otherwise clone each one out of [`collect_with`].
///
/// @example
/// let (issues, summary) = scan::collect_streaming(&handle, |_| {}, |candidate| rows.push(candidate));
pub fn collect_streaming(
    scan: &Scan,
    mut on_tick: impl FnMut(Progress),
    mut on_found: impl FnMut(Candidate),
) -> (Vec<Issue>, Summary) {
    let mut issues = Vec::new();
    let mut ticked = Instant::now();
    loop {
        let event = scan.events.recv_timeout(TICK);
        if ticked.elapsed() >= TICK {
            on_tick(scan.progress());
            ticked = Instant::now();
        }
        match event {
            Ok(Event::Candidate(candidate)) => on_found(candidate),
            Ok(Event::Issue(issue)) => issues.push(issue),
            Ok(Event::Finished(summary)) => {
                on_tick(summary.progress);
                return (issues, summary);
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                return (
                    issues,
                    Summary {
                        progress: scan.progress(),
                        duration: Duration::ZERO,
                        cancelled: true,
                    },
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;

    use super::*;
    use crate::detect::Kind;
    use walk::Walker;

    struct Serial(Vec<Found>);

    impl Visitor for Serial {
        fn issue(&mut self, _issue: Issue) -> Flow {
            Flow::Continue
        }

        fn artifact(&mut self, found: Found) -> Flow {
            self.0.push(found);
            Flow::Continue
        }
    }

    struct ByPath(Vec<PathBuf>);

    impl Visitor for ByPath {
        fn wants_files(&self) -> bool {
            true
        }

        fn issue(&mut self, _issue: Issue) -> Flow {
            Flow::Continue
        }

        fn file(&mut self, path: &Path, _kind: EntryKind) -> Flow {
            self.0.push(path.to_path_buf());
            Flow::Continue
        }
    }

    struct ByEntry(Vec<PathBuf>);

    impl Visitor for ByEntry {
        fn wants_files(&self) -> bool {
            true
        }

        fn issue(&mut self, _issue: Issue) -> Flow {
            Flow::Continue
        }

        fn entry(&mut self, entry: &FileEntry<'_>) -> Flow {
            self.0.push(entry.path().into_owned());
            Flow::Continue
        }
    }

    fn files_seen<V: Visitor>(config: &Config, root: &Path, visitor: V) -> V {
        let detector = config.detector().unwrap();
        let excludes = config.excludes().unwrap();
        let mounts = MountTable::load();
        let protected = Protected::for_user();
        let mut walker = Walker {
            scan: &config.scan,
            detector: &detector,
            excludes: &excludes,
            mounts: &mounts,
            protected: &protected,
            visitor,
        };
        walker.walk_root(root);
        walker.visitor
    }

    type Row = (PathBuf, String, Kind, Usage, Option<std::time::SystemTime>);

    fn serial(config: &Config, root: &Path) -> Vec<Row> {
        let detector = config.detector().unwrap();
        let excludes = config.excludes().unwrap();
        let mounts = MountTable::load();
        let protected = Protected::for_user();
        let mut walker = Walker {
            scan: &config.scan,
            detector: &detector,
            excludes: &excludes,
            mounts: &mounts,
            protected: &protected,
            visitor: Serial(Vec::new()),
        };
        walker.walk_root(root);
        let mut rows: Vec<Row> = walker
            .visitor
            .0
            .into_iter()
            .map(|found| {
                let measured = measure(&found.path, &Cancel::new());
                (
                    found.path,
                    found.rule.name.clone(),
                    found.kind,
                    measured.usage,
                    measured.newest,
                )
            })
            .collect();
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        rows
    }

    fn parallel(config: &Config, root: &Path) -> Vec<Row> {
        let handle = start(config, &[root.to_path_buf()], Cancel::new()).unwrap();
        let mut rows: Vec<Row> = collect(&handle, |_| {})
            .candidates
            .into_iter()
            .map(|c| (c.path, c.rule.name.clone(), c.kind, c.usage, c.newest))
            .collect();
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        rows
    }

    fn write(path: &Path, bytes: usize) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, vec![b'x'; bytes]).unwrap();
    }

    fn tree(root: &Path) {
        write(&root.join("web/package.json"), 2);
        write(&root.join("web/node_modules/a/index.js"), 300);
        for index in 0..2500 {
            write(
                &root.join(format!("web/node_modules/flat/{index}.js")),
                index % 7,
            );
        }
        fs::hard_link(
            root.join("web/node_modules/a/index.js"),
            root.join("web/node_modules/flat/linked.js"),
        )
        .unwrap();
        symlink(root.join("web"), root.join("web/node_modules/loop")).unwrap();
        write(&root.join("web/packages/ui/package.json"), 2);
        write(&root.join("web/packages/ui/node_modules/b/index.js"), 40);
        write(&root.join("web/tsconfig.json"), 2);
        write(&root.join("web/tsconfig.tsbuildinfo"), 9);
        write(&root.join("rust/Cargo.toml"), 2);
        write(&root.join("rust/target/debug/app"), 1000);
        write(&root.join("rust/target/.rustc_info.json"), 2);
        write(&root.join("skipped/package.json"), 2);
        write(&root.join("skipped/node_modules/c.js"), 5);
        write(&root.join("quiet/.kuizerignore"), 0);
        write(&root.join("quiet/app/package.json"), 2);
        write(&root.join("quiet/app/node_modules/d.js"), 5);
        for index in 0..60 {
            write(&root.join(format!("deep/{index}/package.json")), 2);
            write(
                &root.join(format!("deep/{index}/node_modules/e/{index}.js")),
                index,
            );
        }
    }

    #[test]
    fn parallel_scan_matches_a_serial_walk() {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        tree(&root);
        for one_file_system in [true, false] {
            let mut config = Config::default();
            config.scan.size_threads = 4;
            config.scan.one_file_system = one_file_system;
            config.scan.exclude = vec!["skipped".to_owned()];
            let expected = serial(&config, &root);
            assert_eq!(expected.len(), 64, "{expected:?}");
            assert_eq!(parallel(&config, &root), expected);
        }
    }

    #[test]
    fn file_hooks_see_the_same_files() {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        tree(&root);
        let mut config = Config::default();
        config.scan.exclude = vec!["skipped".to_owned()];
        let mut by_path = files_seen(&config, &root, ByPath(Vec::new())).0;
        let mut by_entry = files_seen(&config, &root, ByEntry(Vec::new())).0;
        by_path.sort();
        by_entry.sort();
        assert!(by_path.contains(&root.join("web/package.json")));
        assert!(
            !by_path
                .iter()
                .any(|path| path.starts_with(root.join("skipped")))
        );
        assert_eq!(by_entry, by_path);
    }

    fn by_std_fs(path: &Path) -> (u64, u64, u64) {
        use std::collections::HashSet;
        use std::os::unix::fs::MetadataExt;

        let root = fs::symlink_metadata(path).unwrap();
        let mut seen = HashSet::new();
        let (mut entries, mut apparent, mut allocated) = (0, root.len(), root.blocks() * 512);
        let mut stack = vec![path.to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in fs::read_dir(&dir).unwrap() {
                let entry = entry.unwrap();
                let metadata = entry.metadata().unwrap();
                entries += 1;
                if metadata.nlink() > 1
                    && !metadata.is_dir()
                    && !seen.insert((metadata.dev(), metadata.ino()))
                {
                    continue;
                }
                apparent += metadata.len();
                allocated += metadata.blocks() * 512;
                if metadata.is_dir() {
                    stack.push(entry.path());
                }
            }
        }
        (entries, apparent, allocated)
    }

    #[test]
    fn measure_many_matches_measuring_each_item_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        tree(&root);
        let paths = [
            root.join("web"),
            root.join("rust"),
            root.join("web/package.json"),
            root.join("missing"),
        ];
        let items: Vec<(usize, PathBuf)> = paths.iter().cloned().enumerate().collect();
        let done = Mutex::new(Vec::new());
        measure_many(items, &Cancel::new(), 4, |index, measured| {
            done.lock().unwrap().push((index, measured));
        });
        let mut done = done.into_inner().unwrap();
        done.sort_by_key(|(index, _)| *index);
        assert_eq!(done.len(), paths.len());
        for (index, measured) in done {
            if index < 2 {
                let usage = measured.usage;
                assert_eq!(
                    (usage.entries, usage.apparent, usage.allocated),
                    by_std_fs(&paths[index])
                );
            }
            let alone = measure(&paths[index], &Cancel::new());
            assert_eq!(measured.usage, alone.usage, "{}", paths[index].display());
            assert_eq!(measured.newest, alone.newest);
            assert_eq!(measured.identity, alone.identity);
        }
        assert!(measure(&paths[3], &Cancel::new()).usage.partial);
    }

    #[test]
    fn oversized_thread_counts_are_capped() {
        let mut config = Config::default();
        config.scan.size_threads = 1_000_000;
        assert_eq!(size_threads(&config), MAX_CONFIGURED_THREADS);
        assert_eq!(scan_threads(&config, &[]), MAX_CONFIGURED_THREADS);
    }

    #[test]
    fn cancelled_scan_finishes() {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        tree(&root);
        let mut config = Config::default();
        config.scan.size_threads = 4;
        let handle = start(&config, &[root], Cancel::new()).unwrap();
        handle.cancel();
        let collected = collect(&handle, |_| {});
        assert!(collected.summary.cancelled);
    }
}
