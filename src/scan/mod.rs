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

pub use activity::annotate;
pub use cancel::Cancel;
pub use size::{Measured, measure, measure_parallel};
pub use types::{Candidate, Counters, Event, Issue, IssueKind, Progress, Summary, Usage};

use crate::config::{Config, ConfigError};
use crate::platform::dir::EntryKind;
use crate::platform::{self, MountTable};
use crate::protect::Protected;
use pool::Queue;
use size::{SizeScratch, SizeTask, Start, run_size_task, start_measure};
use walk::{Companion, Context, Flow, Found, Pending, Scratch, Visitor};

const EVENT_CAPACITY: usize = 1024;
const TICK: Duration = Duration::from_millis(100);
const CURRENT_EVERY: u64 = 32;
const FLUSH_EVERY: u64 = 256;
const MAX_SCAN_THREADS: usize = 16;
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

    fn repository(&mut self, root: &Path) {
        self.companion.repository(root);
    }

    fn artifact(&mut self, found: Found) -> Flow {
        self.found.push(found);
        Flow::Continue
    }
}

pub fn size_threads(config: &Config) -> usize {
    match config.scan.size_threads {
        0 => thread::available_parallelism().map_or(4, |n| n.get().min(8)),
        n => n,
    }
}

/// @name scan_threads
/// @description Returns how many threads walk and size a scan of `roots`. Listing directories
/// waits on the disk more than on the CPU, so the default runs about twice as many threads as
/// cores, and only a couple on a spinning disk where more would add seeks. `scan.size_threads`
/// overrides it.
///
/// @example
/// let threads = scan::scan_threads(&config, &roots);
pub fn scan_threads(config: &Config, roots: &[PathBuf]) -> usize {
    if config.scan.size_threads > 0 {
        return config.scan.size_threads;
    }
    let spinning = roots.first().is_some_and(|root| {
        std::fs::metadata(root)
            .is_ok_and(|metadata| platform::rotational(platform::device(&metadata)))
    });
    if spinning {
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
                        .map(|(index, fork)| {
                            let (shared, context, queue) = (&shared, &context, &queue);
                            thread::Builder::new()
                                .name(format!("kuizer-scan-{index}"))
                                .spawn_scoped(scope, move || work(shared, context, queue, fork))
                                .expect("spawn scan thread")
                        })
                        .collect();
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

pub fn collect_with(
    scan: &Scan,
    mut on_tick: impl FnMut(Progress),
    mut on_found: impl FnMut(&Candidate),
) -> Collected {
    let mut candidates = Vec::new();
    let mut issues = Vec::new();
    let mut ticked = Instant::now();
    loop {
        let event = scan.events.recv_timeout(TICK);
        if ticked.elapsed() >= TICK {
            on_tick(scan.progress());
            ticked = Instant::now();
        }
        match event {
            Ok(Event::Candidate(candidate)) => {
                on_found(&candidate);
                candidates.push(candidate);
            }
            Ok(Event::Issue(issue)) => issues.push(issue),
            Ok(Event::Finished(summary)) => {
                on_tick(summary.progress);
                return Collected {
                    candidates,
                    issues,
                    summary,
                };
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                return Collected {
                    candidates,
                    issues,
                    summary: Summary {
                        progress: scan.progress(),
                        duration: Duration::ZERO,
                        cancelled: true,
                    },
                };
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
    fn parallel_measure_matches_serial_measure() {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        tree(&root);
        let serial = measure(&root, &Cancel::new());
        let parallel = measure_parallel(&root, &Cancel::new(), 4);
        assert_eq!(parallel.usage, serial.usage);
        assert_eq!(parallel.newest, serial.newest);
        assert_eq!(parallel.identity, serial.identity);
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
