use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use crossbeam_channel::Receiver;

use super::{DeleteError, Guard, Lanes, Mode, Removal, Status, Target};
use crate::config::DeleteConfig;
use crate::history::{self, Entry, HistoryError, Recorder, Run};
use crate::scan::Candidate;
use crate::source::Source;

const CANCELLED: &str = "cancelled";

/// Items of one source, checked against the same guard roots.
#[derive(Debug, Clone)]
pub struct Batch {
    pub source: Source,
    pub roots: Vec<PathBuf>,
    pub candidates: Vec<Candidate>,
}

/// One item handed to a running executor.
#[derive(Debug, Clone)]
pub struct Job {
    pub source: Source,
    pub mode: Mode,
    pub candidate: Candidate,
}

/// What a finished `run` did: one removal per item it got to, with the source it came from, in
/// the order the items were given, and how many it never started because it was stopped.
#[derive(Debug, Default)]
pub struct Report {
    pub removals: Vec<(Source, Removal)>,
    pub left: usize,
}

impl Report {
    pub fn failed(&self) -> usize {
        self.removals
            .iter()
            .filter(|(_, removal)| removal.status == Status::Failed)
            .count()
    }
}

enum Log {
    User,
    At(PathBuf),
}

/// Removes a confirmed selection for every frontend: revalidates each item against the guard
/// right before acting, removes it the way its source asks, reports a result per item and
/// records what was removed in the history, one run per mode.
pub struct Executor {
    roots: Vec<PathBuf>,
    started_at: String,
    policy: Option<String>,
    protected: Vec<PathBuf>,
    cross_device: bool,
    engines: Vec<String>,
    log: Log,
    recorders: Mutex<Vec<(Mode, Recorder)>>,
    lanes: Lanes,
    tools: Mutex<()>,
    stop: Arc<AtomicBool>,
}

impl Executor {
    /// @name Executor::new
    /// @description Prepares a delete run over `roots`, which are recorded with the run in the
    /// history, using the protected paths and cross-device setting from `delete`.
    ///
    /// @example
    /// let executor = Executor::new(&roots, &config.delete).with_engines(&config.docker.engines);
    pub fn new(roots: &[PathBuf], delete: &DeleteConfig) -> Self {
        Self {
            roots: roots.to_vec(),
            started_at: history::now(),
            policy: None,
            protected: delete.protected.clone(),
            cross_device: delete.cross_device,
            engines: Vec::new(),
            log: Log::User,
            recorders: Mutex::new(Vec::new()),
            lanes: Lanes::default(),
            tools: Mutex::new(()),
            stop: Arc::new(AtomicBool::new(false)),
        }
    }

    /// @name Executor::with_policy
    /// @description Records the named policy on any history runs opened by this executor.
    ///
    /// @example
    /// let executor = Executor::new(&roots, &config.delete).with_policy(Some("stale-deps".into()));
    pub fn with_policy(mut self, policy: Option<String>) -> Self {
        self.policy = policy;
        self
    }

    /// @name Executor::with_engines
    /// @description Sets the container engines Docker objects may be removed through.
    ///
    /// @example
    /// let executor = Executor::new(&[], &config.delete).with_engines(&config.docker.engines);
    pub fn with_engines(mut self, engines: &[String]) -> Self {
        self.engines = engines.to_vec();
        self
    }

    /// @name Executor::with_stop
    /// @description Shares a stop flag with the caller: once it is set, items that have not
    /// started are skipped.
    ///
    /// @example
    /// let executor = Executor::new(&roots, &config.delete).with_stop(Arc::clone(&stop));
    pub fn with_stop(mut self, stop: Arc<AtomicBool>) -> Self {
        self.stop = stop;
        self
    }

    /// @name Executor::recording_at
    /// @description Records into the history file at `path` instead of the user's.
    ///
    /// @example
    /// let executor = Executor::new(&roots, &delete).recording_at(dir.path().join("history.json"));
    pub fn recording_at(mut self, path: PathBuf) -> Self {
        self.log = Log::At(path);
        self
    }

    /// @name Executor::guard
    /// @description Builds the guard that items under `roots` are revalidated against, with the
    /// configured protected paths and cross-device setting.
    ///
    /// @example
    /// let guard = executor.guard(&roots);
    pub fn guard(&self, roots: &[PathBuf]) -> Guard {
        Guard::new(roots)
            .with_protected(&self.protected)
            .with_cross_device(self.cross_device)
    }

    /// @name Executor::stop
    /// @description Skips every item that has not started yet. Items already being removed
    /// finish.
    ///
    /// @example
    /// executor.stop();
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    /// @name Executor::remove
    /// @description Revalidates and removes one item, records it in the history when it is
    /// gone, and returns its result. A stopped executor removes nothing and reports the item
    /// as cancelled.
    ///
    /// @example
    /// let removal = executor.remove(&guard, Source::Artifact, candidate, Mode::Trash);
    pub fn remove(
        &self,
        guard: &Guard,
        source: Source,
        candidate: Candidate,
        mode: Mode,
    ) -> Removal {
        self.attempt(guard, source, candidate, mode)
            .unwrap_or_else(|candidate| Removal {
                candidate: *candidate,
                status: Status::Failed,
                error: Some(CANCELLED.to_owned()),
            })
    }

    fn attempt(
        &self,
        guard: &Guard,
        source: Source,
        candidate: Candidate,
        mode: Mode,
    ) -> Result<Removal, Box<Candidate>> {
        if self.stopped() {
            return Err(Box::new(candidate));
        }
        let mode = source.mode(mode);
        let result = {
            let _lane = self.lanes.claim(&candidate.path);
            let _tool = source
                .permanent()
                .then(|| self.tools.lock().unwrap_or_else(PoisonError::into_inner));
            self.execute(guard, source, &candidate, mode)
        };
        Ok(match result {
            Ok(()) if mode == Mode::DryRun => Removal {
                candidate,
                status: Status::WouldRemove,
                error: None,
            },
            Ok(()) => {
                self.record(Entry::new(&candidate, mode), mode);
                Removal {
                    candidate,
                    status: Status::Removed,
                    error: None,
                }
            }
            Err(error) => Removal {
                candidate,
                status: Status::Failed,
                error: Some(error.to_string()),
            },
        })
    }

    fn execute(
        &self,
        guard: &Guard,
        source: Source,
        candidate: &Candidate,
        mode: Mode,
    ) -> Result<(), DeleteError> {
        match source {
            Source::Artifact | Source::Cache => {
                super::delete(guard, &Target::from(candidate), mode)
            }
            Source::Toolchain => crate::toolchains::remove(guard, candidate, mode),
            Source::Docker => crate::docker::remove(&self.engines, candidate, mode),
        }
    }

    fn record(&self, entry: Entry, mode: Mode) {
        let mut recorders = self
            .recorders
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let index = match recorders.iter().position(|(kept, _)| *kept == mode) {
            Some(index) => index,
            None => {
                recorders.push((mode, self.recorder()));
                recorders.len() - 1
            }
        };
        recorders[index].1.add(entry);
    }

    fn recorder(&self) -> Recorder {
        let recorder = match &self.log {
            Log::User => Recorder::for_user(self.started_at.clone(), &self.roots),
            Log::At(path) => Recorder::at(
                path.clone(),
                path.clone(),
                self.started_at.clone(),
                &self.roots,
            ),
        };
        recorder.with_policy(self.policy.clone())
    }

    /// @name Executor::flush
    /// @description Writes every recorded removal to the history file now.
    ///
    /// @example
    /// executor.flush();
    pub fn flush(&self) {
        for (_, recorder) in self
            .recorders
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
        {
            recorder.flush();
        }
    }

    /// @name Executor::run
    /// @description Removes every item of `batches` in `mode` on up to `workers` threads, each
    /// batch against a guard over its own roots. Calls `on_removal` as each item finishes and
    /// returns the removals in the order the items were given. Nested paths never run at the
    /// same time, and toolchains and Docker objects go one at a time.
    ///
    /// @example
    /// let report = executor.run(batches, Mode::Trash, delete::workers(), |_, removal| show(removal));
    pub fn run(
        &self,
        batches: Vec<Batch>,
        mode: Mode,
        workers: usize,
        on_removal: impl Fn(Source, &Removal) + Sync,
    ) -> Report {
        let guards: Vec<Guard> = batches
            .iter()
            .map(|batch| self.guard(&batch.roots))
            .collect();
        let items: Vec<(usize, usize, Source, Candidate)> = batches
            .into_iter()
            .enumerate()
            .flat_map(
                |(
                    batch,
                    Batch {
                        source, candidates, ..
                    },
                )| {
                    candidates
                        .into_iter()
                        .map(move |candidate| (batch, source, candidate))
                },
            )
            .enumerate()
            .map(|(index, (batch, source, candidate))| (index, batch, source, candidate))
            .collect();
        let total = items.len();
        let queue = Mutex::new(items.into_iter());
        let slots: Mutex<Vec<Option<(Source, Removal)>>> =
            Mutex::new((0..total).map(|_| None).collect());
        let work = || {
            loop {
                let next = queue.lock().unwrap_or_else(PoisonError::into_inner).next();
                let Some((index, batch, source, candidate)) = next else {
                    break;
                };
                if let Ok(removal) = self.attempt(&guards[batch], source, candidate, mode) {
                    on_removal(source, &removal);
                    slots.lock().unwrap_or_else(PoisonError::into_inner)[index] =
                        Some((source, removal));
                }
            }
        };
        thread::scope(|scope| {
            for _ in 1..workers.min(total) {
                if thread::Builder::new().spawn_scoped(scope, work).is_err() {
                    break;
                }
            }
            work();
        });
        let removals: Vec<(Source, Removal)> = slots
            .into_inner()
            .unwrap_or_else(PoisonError::into_inner)
            .into_iter()
            .flatten()
            .collect();
        Report {
            left: total - removals.len(),
            removals,
        }
    }

    /// @name Executor::serve
    /// @description Removes jobs as they arrive on `jobs`, on up to `workers` threads, until the
    /// channel closes or `on_done` returns false. Each item is checked against `guard`, and
    /// `on_done` gets the mode it was removed in with its result. The history is written
    /// whenever the queue runs empty.
    ///
    /// @example
    /// executor.serve(&guard, &jobs, delete::workers(), |mode, removal| results.send((mode, removal)).is_ok());
    pub fn serve(
        &self,
        guard: &Guard,
        jobs: &Receiver<Job>,
        workers: usize,
        on_done: impl Fn(Mode, Removal) -> bool + Sync,
    ) {
        let work = || {
            for job in jobs {
                let mode = job.source.mode(job.mode);
                let removal = self.remove(guard, job.source, job.candidate, job.mode);
                if jobs.is_empty() {
                    self.flush();
                }
                if !on_done(mode, removal) {
                    break;
                }
            }
        };
        thread::scope(|scope| {
            for _ in 1..workers {
                if thread::Builder::new().spawn_scoped(scope, work).is_err() {
                    break;
                }
            }
            work();
        });
    }

    /// @name Executor::finish
    /// @description Writes the history for every mode this executor removed in and returns the
    /// runs it recorded, oldest first and exactly as they stand in the history file, so a
    /// frontend can add them to the history it holds. Returns the first error writing it hit
    /// instead.
    ///
    /// @example
    /// let runs = executor.finish()?;
    pub fn finish(self) -> Result<Vec<Run>, HistoryError> {
        let recorders = self
            .recorders
            .into_inner()
            .unwrap_or_else(PoisonError::into_inner);
        let mut runs = Vec::new();
        let mut first = None;
        for (_, recorder) in recorders {
            match recorder.finish() {
                Ok(run) => runs.extend(run),
                Err(error) => {
                    first.get_or_insert(error);
                }
            }
        }
        runs.sort_by_key(|run| run.id);
        first.map_or(Ok(runs), Err)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::detect::{Category, Kind, RuleInfo, Safety};
    use crate::scan::Usage;

    fn candidate(id: u64, path: &Path) -> Candidate {
        std::fs::create_dir_all(path.join("pkg")).unwrap();
        std::fs::write(path.join("pkg/index.js"), "x").unwrap();
        Candidate {
            id,
            path: path.to_path_buf(),
            kind: Kind::Dir,
            rule: Arc::new(RuleInfo {
                name: "node-modules".to_owned(),
                category: Category::Dependencies,
                safety: Safety::Safe,
                ecosystem: "node".to_owned(),
            }),
            project: None,
            usage: Usage::default(),
            newest: None,
            identity: crate::platform::identity(&std::fs::symlink_metadata(path).unwrap()),
            project_last_active: None,
        }
    }

    fn setup() -> (tempfile::TempDir, PathBuf, Vec<Candidate>) {
        let tmp = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(tmp.path()).unwrap();
        let root = base.join("work");
        let inside = candidate(1, &root.join("a/node_modules"));
        let outside = candidate(2, &base.join("elsewhere/node_modules"));
        (tmp, root, vec![inside, outside])
    }

    #[test]
    fn run_reports_each_item_in_order_and_records_only_what_it_removed() {
        let (tmp, root, candidates) = setup();
        let history = tmp.path().join("history.json");
        let executor = Executor::new(std::slice::from_ref(&root), &DeleteConfig::default())
            .recording_at(history.clone());
        let batch = Batch {
            source: Source::Artifact,
            roots: vec![root.clone()],
            candidates: candidates.clone(),
        };

        let report = executor.run(vec![batch], Mode::Permanent, 2, |_, _| {});
        executor.finish().unwrap();

        let statuses: Vec<Status> = report
            .removals
            .iter()
            .map(|(_, removal)| removal.status)
            .collect();
        assert_eq!(statuses, [Status::Removed, Status::Failed]);
        assert_eq!(report.left, 0);
        assert!(!candidates[0].path.exists());
        assert!(candidates[1].path.join("pkg/index.js").exists());
        let runs = history::load(&history).unwrap().runs;
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].deleted.len(), 1);
        assert_eq!(runs[0].deleted[0].path, candidates[0].path);
    }

    #[test]
    fn finish_returns_the_runs_exactly_as_the_history_file_holds_them() {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap().join("work");
        let history = tmp.path().join("history.json");
        let clean = |candidates: Vec<Candidate>| {
            let executor = Executor::new(std::slice::from_ref(&root), &DeleteConfig::default())
                .recording_at(history.clone());
            let batch = Batch {
                source: Source::Artifact,
                roots: vec![root.clone()],
                candidates,
            };
            executor.run(vec![batch], Mode::Permanent, 1, |_, _| {});
            executor.finish().unwrap()
        };

        let first = clean(vec![candidate(1, &root.join("a/node_modules"))]);
        let second = clean(vec![
            candidate(2, &root.join("b/node_modules")),
            candidate(3, &root.join("c/node_modules")),
        ]);
        let nothing = clean(Vec::new());

        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].deleted.len(), 2);
        assert!(nothing.is_empty());
        let on_disk = history::load(&history).unwrap().runs;
        assert_eq!(on_disk, [first, second].concat());
    }

    #[test]
    fn a_stopped_executor_touches_nothing_and_counts_what_it_skipped() {
        let (tmp, root, candidates) = setup();
        let executor = Executor::new(std::slice::from_ref(&root), &DeleteConfig::default())
            .recording_at(tmp.path().join("history.json"));
        executor.stop();
        let batch = Batch {
            source: Source::Artifact,
            roots: vec![root.clone()],
            candidates: candidates.clone(),
        };

        let report = executor.run(vec![batch], Mode::Permanent, 2, |_, _| {});
        let single = executor.remove(
            &executor.guard(&[root]),
            Source::Artifact,
            candidates[0].clone(),
            Mode::Permanent,
        );

        assert!(report.removals.is_empty());
        assert_eq!(report.left, 2);
        assert_eq!(single.status, Status::Failed);
        assert!(candidates.iter().all(|candidate| candidate.path.exists()));
        assert!(!tmp.path().join("history.json").exists());
    }
}
