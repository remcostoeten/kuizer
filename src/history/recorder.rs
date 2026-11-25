use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use super::{DEFAULT_MAX_BYTES, Entry, HistoryError, Run, load, store};

/// @name append
/// @description Reads the history at `source` if migrating, allocates the next session id, and
/// appends one JSONL run record to `path`, holding a lock so two kuizer processes never drop each
/// other's runs. `source` is never modified when it differs from `path`.
///
/// @example
/// history::append(&source, &target, Run::new(started_at, &roots, entries))?;
pub fn append(source: &Path, path: &Path, run: Run) -> Result<u64, HistoryError> {
    store::append_with_cap(source, path, run, DEFAULT_MAX_BYTES, false)
}

const FLUSH_AFTER: Duration = Duration::from_millis(100);
const FLUSH_AFTER_MAX: Duration = Duration::from_secs(5);
const WRITE_SHARE: u32 = 20;

struct Queue {
    pending: Vec<Entry>,
    oldest: Option<Instant>,
    session: Option<u64>,
    error: Option<HistoryError>,
    write_cost: Duration,
    closed: bool,
}

impl Queue {
    fn flush_delay(&self, oldest: Instant) -> Duration {
        (self.write_cost * WRITE_SHARE)
            .clamp(FLUSH_AFTER, FLUSH_AFTER_MAX)
            .saturating_sub(oldest.elapsed())
    }
}

struct Shared {
    queue: Mutex<Queue>,
    store: Mutex<store::Store>,
    wake: Condvar,
}

impl Shared {
    fn queue(&self) -> MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn flush(&self) {
        let mut store = self.store.lock().unwrap_or_else(PoisonError::into_inner);
        let (mut entries, session) = {
            let mut queue = self.queue();
            if queue.pending.is_empty() {
                return;
            }
            queue.oldest = None;
            (std::mem::take(&mut queue.pending), queue.session)
        };
        let started = Instant::now();
        let result = store.write(&entries, session);
        let mut queue = self.queue();
        match result {
            Ok(session) => {
                queue.session = Some(session);
                queue.write_cost = started.elapsed();
                queue.error = None;
            }
            Err(error) => {
                entries.append(&mut queue.pending);
                queue.pending = entries;
                queue.oldest = Some(Instant::now());
                queue.error.get_or_insert(error);
            }
        }
    }

    fn flush_when_due(&self) {
        let mut queue = self.queue();
        loop {
            if queue.closed {
                return;
            }
            let Some(oldest) = queue.oldest else {
                queue = self
                    .wake
                    .wait(queue)
                    .unwrap_or_else(PoisonError::into_inner);
                continue;
            };
            let remaining = queue.flush_delay(oldest);
            if remaining.is_zero() {
                drop(queue);
                self.flush();
                queue = self.queue();
            } else {
                queue = self
                    .wake
                    .wait_timeout(queue, remaining)
                    .unwrap_or_else(PoisonError::into_inner)
                    .0;
            }
        }
    }
}

pub struct Recorder {
    shared: Arc<Shared>,
    flusher: Option<thread::JoinHandle<()>>,
}

impl Recorder {
    /// @name for_user
    /// @description Starts recording a delete run into the default history file. Nothing is
    /// written until the first entry arrives, so a run that removes nothing leaves no trace.
    ///
    /// @example
    /// let mut recorder = Recorder::for_user(history::now(), &roots);
    pub fn for_user(started_at: String, roots: &[PathBuf]) -> Self {
        let paths = store::write_path()
            .ok()
            .zip(dirs::home_dir())
            .map(|(target, home)| (store::readable_path_for(&target, &home), target));
        let (source, target, has_home) = match paths {
            Some((source, target)) => (source, target, true),
            None => (PathBuf::new(), PathBuf::new(), false),
        };
        let recorder = Self::with_options(
            source,
            target,
            started_at,
            roots,
            DEFAULT_MAX_BYTES,
            has_home,
        );
        if !has_home {
            recorder.shared.queue().error = Some(HistoryError::NoHome);
        }
        recorder
    }

    pub fn at(source: PathBuf, target: PathBuf, started_at: String, roots: &[PathBuf]) -> Self {
        Self::with_options(source, target, started_at, roots, DEFAULT_MAX_BYTES, false)
    }

    pub fn with_policy(self, policy: Option<String>) -> Self {
        self.shared
            .store
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .policy = policy;
        self
    }

    fn with_options(
        source: PathBuf,
        target: PathBuf,
        started_at: String,
        roots: &[PathBuf],
        max_bytes: u64,
        create_parent: bool,
    ) -> Self {
        Self {
            shared: Arc::new(Shared {
                queue: Mutex::new(Queue {
                    pending: Vec::new(),
                    oldest: None,
                    session: None,
                    error: None,
                    write_cost: Duration::ZERO,
                    closed: false,
                }),
                store: Mutex::new(store::Store {
                    source,
                    target,
                    started_at,
                    policy: None,
                    roots: roots.to_vec(),
                    max_bytes,
                    create_parent,
                    written_stamp: None,
                    active_run: None,
                }),
                wake: Condvar::new(),
            }),
            flusher: None,
        }
    }

    /// @name add
    /// @description Queues one removed entry for the history file. The first entry opens the run
    /// and is written straight away. Later entries are appended by a background thread once the
    /// oldest has waited 100 ms, or longer when writing the file is slow so that writing takes at
    /// most about a twentieth of the time, and always by `flush`, `finish` and drop, so an
    /// interrupted run leaves a record that `kuizer restore` can use. Adding never waits for a
    /// write except for the first entry. The first write error is kept for `finish`.
    ///
    /// @example
    /// recorder.add(Entry::new(&candidate, mode));
    pub fn add(&mut self, entry: Entry) {
        let mut queue = self.shared.queue();
        if matches!(queue.error, Some(HistoryError::NoHome)) {
            return;
        }
        queue.pending.push(entry);
        queue.oldest.get_or_insert_with(Instant::now);
        let first = queue.session.is_none();
        drop(queue);
        if first {
            self.shared.flush();
            return;
        }
        self.shared.wake.notify_one();
        if self.flusher.is_none() {
            let shared = Arc::clone(&self.shared);
            self.flusher = thread::Builder::new()
                .name("kuizer-history".to_owned())
                .spawn(move || shared.flush_when_due())
                .ok();
        }
    }

    /// @name flush
    /// @description Writes every queued entry to the history file now. Call it after a batch
    /// of removals so nothing recorded waits on the timer.
    ///
    /// @example
    /// recorder.flush();
    pub fn flush(&self) {
        self.shared.flush();
    }

    /// @name session
    /// @description Returns the session id of the run this recorder opened, or `None` while no
    /// entry was written yet.
    ///
    /// @example
    /// let undo = recorder.session().map(|id| format!("s{id}"));
    pub fn session(&self) -> Option<u64> {
        self.shared.queue().session
    }

    /// @name finish
    /// @description Writes every queued entry and returns the run as it now stands in the history
    /// file, or `None` when nothing was recorded, so a frontend can add it to a history it holds
    /// without reading the file again. Returns the first write error instead when one is left.
    ///
    /// @example
    /// let run = recorder.finish()?;
    pub fn finish(self) -> Result<Option<Run>, HistoryError> {
        self.shared.flush();
        if let Some(error) = self.shared.queue().error.take() {
            return Err(error);
        }
        let Some(session) = self.shared.queue().session else {
            return Ok(None);
        };
        let (active_run, target) = {
            let mut store = self
                .shared
                .store
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            (store.active_run.take(), store.target.clone())
        };
        if let Some(run) = active_run
            && run.id == session
        {
            return Ok(Some(run));
        }
        let history = load(&target)?;
        Ok(history.runs.into_iter().rev().find(|run| run.id == session))
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.shared.queue().closed = true;
        self.shared.wake.notify_all();
        if let Some(flusher) = self.flusher.take() {
            let _ = flusher.join();
        }
        self.shared.flush();
    }
}

/// @name record
/// @description Appends a finished run, delete or restore, to the default history file.
///
/// @example
/// history::record(Run::restore(started_at, restored))?;
pub fn record(run: Run) -> Result<(), HistoryError> {
    record_session(run).map(drop)
}

pub(super) fn record_session(run: Run) -> Result<u64, HistoryError> {
    let target = store::write_path()?;
    let home = dirs::home_dir().ok_or(HistoryError::NoHome)?;
    let source = store::readable_path_for(&target, &home);
    store::append_with_cap(&source, &target, run, DEFAULT_MAX_BYTES, true)
}
