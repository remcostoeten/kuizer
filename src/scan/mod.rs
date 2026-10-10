mod cancel;
mod size;
mod types;
mod walk;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use crossbeam_channel::{Receiver, bounded};

pub use cancel::Cancel;
pub use size::{Measured, measure};
pub use types::{Candidate, Counters, Event, Issue, IssueKind, Progress, Summary, Usage};

use crate::config::{Config, ConfigError};
use crate::platform::MountTable;
use crate::protect::Protected;
use walk::{Found, Walker};

const FOUND_CAPACITY: usize = 256;
const EVENT_CAPACITY: usize = 1024;

pub struct Scan {
    pub events: Receiver<Event>,
    counters: Arc<Counters>,
    cancel: Cancel,
    coordinator: Option<JoinHandle<()>>,
}

impl Scan {
    pub fn progress(&self) -> Progress {
        self.counters.snapshot()
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

fn size_threads(config: &Config) -> usize {
    match config.scan.size_threads {
        0 => thread::available_parallelism().map_or(4, |n| n.get().min(8)),
        n => n,
    }
}

fn parent_project(path: &Path) -> Option<PathBuf> {
    path.parent().map(Path::to_path_buf)
}

pub fn start(config: &Config, roots: &[PathBuf], cancel: Cancel) -> Result<Scan, ConfigError> {
    let detector = Arc::new(config.detector()?);
    let excludes = Arc::new(config.excludes()?);
    let scan_config = Arc::new(config.scan.clone());
    let roots = normalize_roots(roots);
    let counters = Arc::new(Counters::default());
    let (found_tx, found_rx) = bounded::<Found>(FOUND_CAPACITY);
    let (event_tx, event_rx) = bounded::<Event>(EVENT_CAPACITY);
    let workers = size_threads(config);
    let next_id = Arc::new(AtomicU64::new(1));
    let min_size = config.scan.min_size.0;
    let started = Instant::now();

    let walker = {
        let counters = Arc::clone(&counters);
        let cancel = cancel.clone();
        let events = event_tx.clone();
        thread::Builder::new()
            .name("kuizer-walk".to_owned())
            .spawn(move || {
                let mounts = MountTable::load();
                let protected = Protected::for_user();
                let walker = Walker {
                    scan: &scan_config,
                    detector: &detector,
                    excludes: &excludes,
                    mounts: &mounts,
                    protected: &protected,
                    cancel: &cancel,
                    counters: &counters,
                    found: &found_tx,
                    events: &events,
                };
                for root in &roots {
                    if cancel.is_cancelled() {
                        break;
                    }
                    walker.walk_root(root);
                }
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

    Ok(Scan {
        events: event_rx,
        counters,
        cancel,
        coordinator: Some(coordinator),
    })
}

pub struct Collected {
    pub candidates: Vec<Candidate>,
    pub issues: Vec<Issue>,
    pub summary: Summary,
}

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
    loop {
        match scan
            .events
            .recv_timeout(std::time::Duration::from_millis(100))
        {
            Ok(Event::Candidate(candidate)) => {
                on_found(&candidate);
                candidates.push(candidate);
            }
            Ok(Event::Issue(issue)) => issues.push(issue),
            Ok(Event::Finished(summary)) => {
                return Collected {
                    candidates,
                    issues,
                    summary,
                };
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => on_tick(scan.progress()),
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                return Collected {
                    candidates,
                    issues,
                    summary: Summary {
                        progress: scan.progress(),
                        duration: std::time::Duration::ZERO,
                        cancelled: true,
                    },
                };
            }
        }
    }
}
