mod activity;
mod cancel;
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
pub use size::{Measured, measure};
pub use types::{Candidate, Counters, Event, Issue, IssueKind, Progress, Summary, Usage};

use crate::config::{Config, ConfigError};
use crate::platform::MountTable;
use crate::protect::Protected;
use walk::{Flow, Found, Visitor, Walker};

const FOUND_CAPACITY: usize = 256;
const EVENT_CAPACITY: usize = 1024;
const TICK: Duration = Duration::from_millis(100);
const CURRENT_EVERY: u64 = 32;

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

struct Sink<'a, C> {
    cancel: &'a Cancel,
    counters: &'a Counters,
    current: &'a Mutex<Option<PathBuf>>,
    found: &'a Sender<Found>,
    events: &'a Sender<Event>,
    companion: C,
}

impl<C: Visitor> Visitor for Sink<'_, C> {
    fn cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    fn issue(&mut self, issue: Issue) -> Flow {
        Counters::add(&self.counters.issues, 1);
        match self.events.send(Event::Issue(issue)) {
            Ok(()) => Flow::Continue,
            Err(_) => Flow::Stop,
        }
    }

    fn entered(&mut self, path: &Path) {
        let entered = self.counters.dirs.fetch_add(1, Ordering::Relaxed);
        if entered.is_multiple_of(CURRENT_EVERY)
            && let Ok(mut current) = self.current.lock()
        {
            *current = Some(path.to_path_buf());
        }
    }

    fn counted(&mut self, file_type: std::fs::FileType) {
        Counters::add(&self.counters.files, 1);
        self.companion.counted(file_type);
    }

    fn file(&mut self, path: &Path, file_type: std::fs::FileType) -> Flow {
        self.companion.file(path, file_type)
    }

    fn repository(&mut self, root: &Path) {
        self.companion.repository(root);
    }

    fn artifact(&mut self, found: Found) -> Flow {
        match self.found.send(found) {
            Ok(()) => Flow::Continue,
            Err(_) => Flow::Stop,
        }
    }
}

pub fn size_threads(config: &Config) -> usize {
    match config.scan.size_threads {
        0 => thread::available_parallelism().map_or(4, |n| n.get().min(8)),
        n => n,
    }
}

fn parent_project(path: &Path) -> Option<PathBuf> {
    path.parent().map(Path::to_path_buf)
}

pub fn start(config: &Config, roots: &[PathBuf], cancel: Cancel) -> Result<Scan, ConfigError> {
    start_with(config, roots, cancel, ()).map(|(scan, _)| scan)
}

pub(crate) fn start_with<C: Visitor + Send + 'static>(
    config: &Config,
    roots: &[PathBuf],
    cancel: Cancel,
    companion: C,
) -> Result<(Scan, Receiver<C>), ConfigError> {
    let detector = Arc::new(config.detector()?);
    let excludes = Arc::new(config.excludes()?);
    let scan_config = Arc::new(config.scan.clone());
    let roots = normalize_roots(roots);
    let counters = Arc::new(Counters::default());
    let current = Arc::new(Mutex::new(None));
    let (found_tx, found_rx) = bounded::<Found>(FOUND_CAPACITY);
    let (event_tx, event_rx) = bounded::<Event>(EVENT_CAPACITY);
    let workers = size_threads(config);
    let next_id = Arc::new(AtomicU64::new(1));
    let min_size = config.scan.min_size.0;
    let started = Instant::now();
    let (companion_tx, companion_rx) = bounded::<C>(1);

    let walker = {
        let counters = Arc::clone(&counters);
        let current = Arc::clone(&current);
        let cancel = cancel.clone();
        let events = event_tx.clone();
        thread::Builder::new()
            .name("kuizer-walk".to_owned())
            .spawn(move || {
                let mounts = MountTable::load();
                let protected = Protected::for_user();
                let mut walker = Walker {
                    scan: &scan_config,
                    detector: &detector,
                    excludes: &excludes,
                    mounts: &mounts,
                    protected: &protected,
                    visitor: Sink {
                        cancel: &cancel,
                        counters: &counters,
                        current: &current,
                        found: &found_tx,
                        events: &events,
                        companion,
                    },
                };
                for root in &roots {
                    if cancel.is_cancelled() {
                        break;
                    }
                    walker.walk_root(root);
                }
                let _ = companion_tx.send(walker.visitor.companion);
            })
            .expect("spawn walker thread")
    };

    let sizers: Vec<JoinHandle<()>> = (0..workers)
        .map(|index| {
            let found_rx = found_rx.clone();
            let events = event_tx.clone();
            let counters = Arc::clone(&counters);
            let cancel = cancel.clone();
            let next_id = Arc::clone(&next_id);
            thread::Builder::new()
                .name(format!("kuizer-size-{index}"))
                .spawn(move || {
                    for found in found_rx {
                        if cancel.is_cancelled() {
                            continue;
                        }
                        let Measured {
                            usage,
                            newest,
                            identity,
                        } = measure(&found.path, &cancel);
                        if usage.allocated < min_size {
                            continue;
                        }
                        Counters::add(&counters.candidates, 1);
                        Counters::add(&counters.bytes, usage.allocated);
                        let candidate = Candidate {
                            id: next_id.fetch_add(1, Ordering::Relaxed),
                            project: parent_project(&found.path),
                            path: found.path,
                            kind: found.kind,
                            rule: found.rule,
                            usage,
                            newest,
                            identity,
                            project_last_active: None,
                        };
                        if events.send(Event::Candidate(candidate)).is_err() {
                            cancel.cancel();
                        }
                    }
                })
                .expect("spawn sizing thread")
        })
        .collect();
    drop(found_rx);

    let coordinator = {
        let counters = Arc::clone(&counters);
        let cancel = cancel.clone();
        thread::Builder::new()
            .name("kuizer-scan".to_owned())
            .spawn(move || {
                let _ = walker.join();
                for sizer in sizers {
                    let _ = sizer.join();
                }
                let summary = Summary {
                    progress: counters.snapshot(),
                    duration: started.elapsed(),
                    cancelled: cancel.is_cancelled(),
                };
                let _ = event_tx.send(Event::Finished(summary));
            })
            .expect("spawn coordinator thread")
    };

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
