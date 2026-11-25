use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::delete::Mode;
use crate::scan::Candidate;
use crate::units::ByteSize;

pub mod restore;
mod state;
mod store;
mod stored_path;

pub use state::{
    ItemState, Restored, TrashSnapshot, TrashState, TrashView, freed_bytes, item_state, restorable,
    still_in_trash,
};
pub use store::{
    DEFAULT_MAX_BYTES, PruneOptions, PruneReport, default_path, load, prune, prune_default,
    readable_path_in, write_path,
};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct History {
    pub next_session: u64,
    pub runs: Vec<Run>,
    pub cumulative: Cumulative,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    #[default]
    Delete,
    Restore,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Run {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub action: Action,
    pub started_at: String,
    pub finished_at: String,
    pub command: String,
    #[serde(with = "stored_path::many")]
    pub roots: Vec<PathBuf>,
    pub deleted: Vec<Entry>,
    pub total: Total,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub deleted_at: String,
    #[serde(with = "stored_path")]
    pub path: PathBuf,
    pub rule: String,
    pub category: String,
    pub mode: Mode,
    pub bytes: u64,
    pub size: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restores: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Total {
    pub items: usize,
    pub bytes: u64,
    pub size: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Cumulative {
    pub runs: usize,
    pub items: usize,
    pub bytes: u64,
    pub size: String,
    pub first_run: Option<String>,
    pub last_run: Option<String>,
    pub restored_items: usize,
    pub restored_bytes: u64,
    pub permanent_bytes: u64,
    pub trashed_bytes: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error("no home directory is known for this platform")]
    NoHome,
    #[error("cannot read history {path}: {source}")]
    Read { path: PathBuf, source: io::Error },
    #[error("invalid history {path}: {source}")]
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("cannot write history {path}: {source}")]
    Write { path: PathBuf, source: io::Error },
}

/// @name now
/// @description Returns the current local time as an RFC 3339 string with its UTC offset.
///
/// @example
/// let started_at = history::now();
pub fn now() -> String {
    jiff::Zoned::now()
        .strftime("%Y-%m-%dT%H:%M:%S%:z")
        .to_string()
}

fn size(bytes: u64) -> String {
    ByteSize(bytes).to_string()
}

impl Entry {
    pub fn new(candidate: &Candidate, mode: Mode) -> Self {
        let bytes = candidate.usage.allocated;
        Self {
            deleted_at: now(),
            path: candidate.path.clone(),
            rule: candidate.rule.name.clone(),
            category: candidate.rule.category.as_str().to_owned(),
            mode,
            bytes,
            size: size(bytes),
            restores: None,
        }
    }

    pub fn restored(original: &Entry, id: String) -> Self {
        Self {
            deleted_at: now(),
            restores: Some(id),
            ..original.clone()
        }
    }
}

impl Total {
    fn of(entries: &[Entry]) -> Self {
        Self::sum(entries.iter())
    }

    pub fn sum<'a>(entries: impl Iterator<Item = &'a Entry>) -> Self {
        let (items, bytes) = entries.fold((0, 0), |(items, bytes), entry| {
            (items + 1, bytes + entry.bytes)
        });
        Self {
            items,
            bytes,
            size: size(bytes),
        }
    }
}

impl Run {
    pub fn new(started_at: String, roots: &[PathBuf], deleted: Vec<Entry>) -> Self {
        Self {
            id: 0,
            action: Action::Delete,
            started_at,
            finished_at: now(),
            command: std::env::args_os()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(" "),
            roots: roots.to_vec(),
            total: Total::of(&deleted),
            deleted,
        }
    }

    pub fn restore(started_at: String, restored: Vec<Entry>) -> Self {
        let roots = restored
            .iter()
            .filter_map(|entry| entry.path.parent().map(Path::to_path_buf))
            .collect::<Vec<_>>();
        Self {
            action: Action::Restore,
            ..Self::new(started_at, &roots, restored)
        }
    }

    pub fn session(&self) -> String {
        format!("s{}", self.id)
    }
}

impl History {
    fn recompute(&mut self) {
        let total = |action| {
            self.runs
                .iter()
                .filter(|run| run.action == action)
                .fold((0, 0), |(items, bytes), run| {
                    (items + run.total.items, bytes + run.total.bytes)
                })
        };
        let (items, bytes) = total(Action::Delete);
        let (restored_items, restored_bytes) = total(Action::Restore);
        let bytes = bytes.saturating_sub(restored_bytes);
        let by_mode = |mode| {
            self.runs
                .iter()
                .filter(|run| run.action == Action::Delete)
                .flat_map(|run| &run.deleted)
                .filter(|entry| entry.mode == mode)
                .map(|entry| entry.bytes)
                .sum::<u64>()
        };
        let permanent_bytes = by_mode(Mode::Permanent);
        let trashed_bytes = by_mode(Mode::Trash).saturating_sub(restored_bytes);
        self.cumulative = Cumulative {
            runs: self.runs.len(),
            items: items.saturating_sub(restored_items),
            bytes,
            size: size(bytes),
            first_run: self.runs.first().map(|run| run.started_at.clone()),
            last_run: self.runs.last().map(|run| run.started_at.clone()),
            restored_items,
            restored_bytes,
            permanent_bytes,
            trashed_bytes,
        };
    }

    fn push(&mut self, run: Run) -> usize {
        self.next_session = self.next_session.max(run.id + 1);
        self.runs.push(run);
        self.recompute();
        self.runs.len() - 1
    }

    fn assign_ids(&mut self) {
        let mut next = self
            .runs
            .iter()
            .map(|run| run.id)
            .max()
            .unwrap_or(0)
            .max(self.next_session.saturating_sub(1))
            + 1;
        for run in self.runs.iter_mut().filter(|run| run.id == 0) {
            run.id = next;
            next += 1;
        }
        self.next_session = next;
    }

    /// @name find
    /// @description Resolves a session id such as `s42` or an item id such as `s42.3` to
    /// `(run, entry)` indexes. A session id yields every entry of that run.
    ///
    /// @example
    /// let targets = history.find("s42.3")?;
    pub fn find(&self, id: &str) -> Option<Vec<(usize, usize)>> {
        let (session, item) = parse_id(id)?;
        let run_index = self.runs.iter().position(|run| run.id == session)?;
        let run = &self.runs[run_index];
        match item {
            Some(item) => {
                let entry = item
                    .checked_sub(1)
                    .filter(|&index| index < run.deleted.len())?;
                Some(vec![(run_index, entry)])
            }
            None => Some(
                (0..run.deleted.len())
                    .map(|entry| (run_index, entry))
                    .collect(),
            ),
        }
    }

    pub fn item_id(&self, run: usize, entry: usize) -> String {
        format!("s{}.{}", self.runs[run].id, entry + 1)
    }

    /// @name restored_ids
    /// @description Returns the item ids that a later restore run put back.
    ///
    /// @example
    /// let restored = history.restored_ids();
    /// assert!(restored.contains("s4.2"));
    pub fn restored_ids(&self) -> HashSet<&str> {
        self.runs
            .iter()
            .filter(|run| run.action == Action::Restore)
            .flat_map(|run| &run.deleted)
            .filter_map(|entry| entry.restores.as_deref())
            .collect()
    }
}

fn parse_id(id: &str) -> Option<(u64, Option<usize>)> {
    let rest = id.strip_prefix('s')?;
    match rest.split_once('.') {
        Some((session, item)) => Some((session.parse().ok()?, Some(item.parse().ok()?))),
        None => Some((rest.parse().ok()?, None)),
    }
}

/// @name trashed_paths
/// @description Lists the original locations of everything currently in the system trash, or
/// `None` where the platform cannot list the trash.
///
/// @example
/// let trashed = history::trashed_paths().unwrap_or_default();
#[cfg(any(
    target_os = "windows",
    all(
        unix,
        not(target_os = "macos"),
        not(target_os = "ios"),
        not(target_os = "android")
    )
))]
pub fn trashed_paths() -> Option<Vec<PathBuf>> {
    let items = trash::os_limited::list().ok()?;
    Some(items.iter().map(trash::TrashItem::original_path).collect())
}

#[cfg(not(any(
    target_os = "windows",
    all(
        unix,
        not(target_os = "macos"),
        not(target_os = "ios"),
        not(target_os = "android")
    )
)))]
pub fn trashed_paths() -> Option<Vec<PathBuf>> {
    None
}

/// @name TrashListing
/// @description Runs `trashed_paths` on its own thread so a frontend can start listing the trash
/// at launch and pick the result up once it needs it, instead of waiting for it while loading.
///
/// @example
/// let listing = history::TrashListing::start();
/// let trashed = listing.wait();
pub struct TrashListing(Option<thread::JoinHandle<Option<Vec<PathBuf>>>>);

impl TrashListing {
    pub fn start() -> Self {
        Self(
            thread::Builder::new()
                .name("kuizer-trash".to_owned())
                .spawn(trashed_paths)
                .ok(),
        )
    }

    pub fn wait(self) -> Option<Vec<PathBuf>> {
        match self.0 {
            Some(handle) => handle.join().ok().flatten(),
            None => trashed_paths(),
        }
    }
}

const RESTORE_WINDOW_SECONDS: i64 = 300;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashRecord {
    pub path: PathBuf,
    pub deleted_at: Option<jiff::civil::DateTime>,
}

impl TrashRecord {
    /// @name TrashRecord::new
    /// @description Builds a record from a trash item's original path and the Unix time the
    /// trash library reports. Freedesktop `.trashinfo` files store local wall-clock time without
    /// an offset and the library reads it in today's time zone, so the time is turned back into
    /// that wall-clock time, which stays comparable after a time zone or DST change.
    ///
    /// @example
    /// let record = TrashRecord::new(item.original_path(), item.time_deleted);
    pub fn new(path: PathBuf, time_deleted: i64) -> Self {
        let deleted_at = (time_deleted >= 0)
            .then(|| jiff::Timestamp::from_second(time_deleted).ok())
            .flatten()
            .map(|time| time.to_zoned(jiff::tz::TimeZone::system()).datetime());
        Self { path, deleted_at }
    }
}

/// @name match_trash
/// @description Pairs each requested trashed entry with an item in the trash: the same original
/// path, deleted closest to the entry's local wall-clock time and within five minutes of it,
/// so a time zone or DST change between delete and restore does not matter. Items without a
/// deletion time match only when no timed item does. Every trash item is used at most once.
///
/// @example
/// let matched = history::match_trash(&history, &targets, &records);
/// if let Some(item) = matched[0] { restore(&items[item]) }
pub fn match_trash(
    history: &History,
    targets: &[(usize, usize)],
    trash: &[TrashRecord],
) -> Vec<Option<usize>> {
    let mut used = vec![false; trash.len()];
    let mut by_path: HashMap<&Path, Vec<usize>> = HashMap::new();
    for (index, record) in trash.iter().enumerate() {
        by_path
            .entry(record.path.as_path())
            .or_default()
            .push(index);
    }
    targets
        .iter()
        .map(|&(run, entry)| {
            let entry = &history.runs[run].deleted[entry];
            if entry.mode != Mode::Trash {
                return None;
            }
            let deleted = entry.deleted_at.parse::<jiff::civil::DateTime>().ok();
            let same_path = by_path.get(entry.path.as_path())?;
            let timed = same_path
                .iter()
                .copied()
                .filter(|&index| !used[index])
                .filter_map(|index| {
                    let gap = trash[index]
                        .deleted_at?
                        .duration_since(deleted?)
                        .as_secs()
                        .abs();
                    (gap <= RESTORE_WINDOW_SECONDS).then_some((gap, index))
                })
                .min()
                .map(|(_, index)| index);
            let found = timed.or_else(|| {
                same_path
                    .iter()
                    .copied()
                    .find(|&index| !used[index] && trash[index].deleted_at.is_none())
            });
            if let Some(index) = found {
                used[index] = true;
            }
            found
        })
        .collect()
}

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

fn record_session(run: Run) -> Result<u64, HistoryError> {
    let target = store::write_path()?;
    let home = dirs::home_dir().ok_or(HistoryError::NoHome)?;
    let source = store::readable_path_for(&target, &home);
    store::append_with_cap(&source, &target, run, DEFAULT_MAX_BYTES, true)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    const FILE_NAME: &str = store::STATE_FILE_NAME;

    fn entry(bytes: u64) -> Entry {
        Entry {
            deleted_at: now(),
            path: PathBuf::from("/tmp/a/node_modules"),
            rule: "node-modules".to_owned(),
            category: "dependencies".to_owned(),
            mode: Mode::Trash,
            bytes,
            size: size(bytes),
            restores: None,
        }
    }

    fn entry_at(path: &str, deleted_at: &str) -> Entry {
        Entry {
            path: PathBuf::from(path),
            deleted_at: deleted_at.to_owned(),
            ..entry(1)
        }
    }

    fn record(path: &str, deleted_at: Option<&str>) -> TrashRecord {
        TrashRecord {
            path: PathBuf::from(path),
            deleted_at: deleted_at.map(|time| time.parse().unwrap()),
        }
    }

    #[test]
    fn append_accumulates_totals_across_runs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);
        append(
            &path,
            &path,
            Run::new(now(), &[], vec![entry(1 << 20), entry(1 << 20)]),
        )
        .expect("first");
        append(&path, &path, Run::new(now(), &[], vec![entry(1 << 30)])).expect("second");

        let history = load(&path).expect("load");
        assert_eq!(history.runs.len(), 2);
        assert_eq!(history.runs[0].total.items, 2);
        assert_eq!(history.runs[0].total.size, "2.0 MiB");
        assert_eq!(history.cumulative.runs, 2);
        assert_eq!(history.cumulative.items, 3);
        assert_eq!(history.cumulative.bytes, (1 << 30) + (2 << 20));

        let text = fs::read_to_string(&path).expect("read");
        assert_eq!(text.lines().count(), 2);
        for line in text.lines() {
            let value: serde_json::Value = serde_json::from_str(line).expect("jsonl line");
            assert_eq!(value["type"], "run");
        }
    }

    #[test]
    fn recorder_writes_each_entry_into_one_run() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);
        let mut fresh = Recorder::at(path.clone(), path.clone(), now(), &[]);
        fresh.add(entry(7));
        fresh.add(entry(0));
        fresh.flush();
        assert_eq!(load(&path).expect("fresh").runs[0].total.items, 2);
        let mut recorder = Recorder::at(path.clone(), path.clone(), now(), &[]);

        recorder.add(entry(1));
        let partial = load(&path).expect("after one");
        assert_eq!(partial.runs.len(), 2);
        assert_eq!(partial.runs[1].total.items, 1);

        recorder.add(entry(2));
        assert_eq!(recorder.session(), Some(2));
        recorder.finish().expect("no errors");
        let history = load(&path).expect("after two");
        assert_eq!(history.runs.len(), 2);
        assert_eq!(history.runs[1].session(), "s2");
        assert_eq!(history.runs[1].total.bytes, 3);
        assert_eq!(history.cumulative.items, 4);
    }

    #[test]
    fn an_unreadable_history_is_set_aside_and_recording_continues() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);
        fs::write(&path, "{\"runs\": [").expect("garbage");

        append(&path, &path, Run::new(now(), &[], vec![entry(1)])).expect("append");

        assert_eq!(load(&path).expect("fresh").runs.len(), 1);
        let aside: Vec<_> = fs::read_dir(dir.path())
            .expect("list")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".corrupt-"))
            .collect();
        assert_eq!(aside.len(), 1);
        assert_eq!(
            fs::read_to_string(aside[0].path()).expect("kept"),
            "{\"runs\": ["
        );

        let mut recorder = Recorder::at(path.clone(), path.clone(), now(), &[]);
        recorder.add(entry(2));
        recorder.finish().expect("no errors");
        assert_eq!(load(&path).expect("reload").runs.len(), 2);
    }

    #[test]
    fn recorder_without_entries_writes_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);
        let recorder = Recorder::at(path.clone(), path.clone(), now(), &[]);
        assert_eq!(recorder.session(), None);
        recorder.finish().expect("no errors");
        assert!(!path.exists());
    }

    fn recorded_items(path: &Path) -> usize {
        load(path).map_or(0, |history| history.cumulative.items)
    }

    #[test]
    fn queued_entries_reach_the_file_without_finish() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);
        let mut recorder = Recorder::at(path.clone(), path.clone(), now(), &[]);
        recorder.add(entry(1));
        assert_eq!(recorded_items(&path), 1);
        recorder.add(entry(2));
        recorder.add(entry(3));
        drop(recorder);
        assert_eq!(recorded_items(&path), 3);

        let other = dir.path().join("interrupted.json");
        let mut abandoned = Recorder::at(other.clone(), other.clone(), now(), &[]);
        abandoned.add(entry(1));
        abandoned.add(entry(2));
        abandoned.add(entry(3));
        std::mem::forget(abandoned);
        let deadline = Instant::now() + Duration::from_secs(10);
        while recorded_items(&other) < 3 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(recorded_items(&other), 3);
    }

    #[test]
    fn many_queued_entries_land_in_one_run_and_keep_other_runs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);
        let mut recorder = Recorder::at(path.clone(), path.clone(), now(), &[]);
        recorder.add(entry(1));
        append(&path, &path, Run::new(now(), &[], vec![entry(10)])).expect("other process");
        for _ in 0..128 {
            recorder.add(entry(1));
        }
        recorder.finish().expect("no errors");
        let history = load(&path).expect("load");
        assert_eq!(history.runs.len(), 2);
        assert_eq!(history.runs[0].deleted.len(), 129);
        assert_eq!(history.runs[0].total.items, 129);
        assert_eq!(history.cumulative.items, 130);
    }

    #[test]
    fn concurrent_appends_keep_every_run() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    for _ in 0..5 {
                        append(&path, &path, Run::new(now(), &[], vec![entry(1)])).expect("append");
                    }
                });
            }
        });
        let history = load(&path).expect("load");
        assert_eq!(history.runs.len(), 40);
        let ids: HashSet<u64> = history.runs.iter().map(|run| run.id).collect();
        assert_eq!(ids.len(), 40);
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .expect("list")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn missing_file_is_empty_history() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(
            load(&dir.path().join("none.json")).expect("load"),
            History::default()
        );
    }

    #[test]
    fn reads_legacy_history_and_writes_the_new_file() {
        let home = tempfile::tempdir().expect("tempdir");
        let legacy_sweep = home.path().join(store::LEGACY_SWEEP_FILE);
        let legacy_kuizer = home.path().join(store::LEGACY_KUIZER_FILE);
        let current = store::state_path_in(home.path());
        assert_eq!(readable_path_in(home.path()), current);

        append(
            &legacy_sweep,
            &legacy_sweep,
            Run::new(now(), &[], vec![entry(1)]),
        )
        .expect("legacy sweep");
        assert_eq!(readable_path_in(home.path()), legacy_sweep);

        append(
            &legacy_kuizer,
            &legacy_kuizer,
            Run::new(now(), &[], vec![entry(2)]),
        )
        .expect("legacy kuizer");
        assert_eq!(readable_path_in(home.path()), legacy_kuizer);

        store::append_with_cap(
            &readable_path_in(home.path()),
            &current,
            Run::new(now(), &[], vec![entry(3)]),
            DEFAULT_MAX_BYTES,
            true,
        )
        .expect("migrate");
        assert_eq!(readable_path_in(home.path()), current);
        assert_eq!(load(&current).expect("current").runs.len(), 2);
        assert_eq!(load(&legacy_kuizer).expect("legacy kept").runs.len(), 1);
    }

    #[test]
    fn old_files_get_ids_in_order_and_keep_them() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);
        let old = r#"{"runs":[
            {"started_at":"a","finished_at":"a","command":"kuizer clean","roots":[],"deleted":[],"total":{"items":0,"bytes":0,"size":"0 B"}},
            {"started_at":"b","finished_at":"b","command":"kuizer clean","roots":[],"deleted":[],"total":{"items":0,"bytes":0,"size":"0 B"}}
        ]}"#;
        fs::write(&path, old).expect("write");

        let first = load(&path).expect("load");
        assert_eq!(
            first.runs.iter().map(Run::session).collect::<Vec<_>>(),
            ["s1", "s2"]
        );
        assert_eq!(first.next_session, 3);

        append(&path, &path, Run::new(now(), &[], vec![entry(1)])).expect("append");
        let second = load(&path).expect("reload");
        assert_eq!(
            second.runs.iter().map(Run::session).collect::<Vec<_>>(),
            ["s1", "s2", "s3"]
        );
        assert_eq!(load(&path).expect("again"), second);
    }

    #[test]
    fn finds_sessions_and_items_by_id() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);
        append(&path, &path, Run::new(now(), &[], vec![entry(1), entry(2)])).expect("append");
        let history = load(&path).expect("load");

        assert_eq!(history.find("s1"), Some(vec![(0, 0), (0, 1)]));
        assert_eq!(history.find("s1.2"), Some(vec![(0, 1)]));
        assert_eq!(history.item_id(0, 1), "s1.2");
        for missing in ["s1.3", "s1.0", "s2", "1", "s", "sx.1"] {
            assert_eq!(history.find(missing), None, "{missing}");
        }
    }

    #[test]
    fn a_failed_write_keeps_its_entries_in_one_session() {
        let dir = tempfile::tempdir().expect("tempdir");
        let folder = dir.path().join("later");
        let path = folder.join(FILE_NAME);
        let mut recorder = Recorder::at(path.clone(), path.clone(), now(), &[]);
        recorder.add(entry(1));
        assert!(recorder.session().is_none());
        fs::create_dir(&folder).expect("folder");
        recorder.add(entry(2));
        recorder.finish().expect("recovers");
        let history = load(&path).expect("load");
        assert_eq!(history.runs.len(), 1);
        assert_eq!(history.runs[0].deleted.len(), 2);
    }

    #[test]
    fn a_non_utf8_path_survives_the_history_file() {
        use std::os::unix::ffi::OsStringExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);
        let odd = PathBuf::from(std::ffi::OsString::from_vec(
            b"/tmp/caf\xE9/target".to_vec(),
        ));
        let mut item = entry(1);
        item.path = odd.clone();
        item.mode = Mode::Trash;
        let mut recorder = Recorder::at(
            path.clone(),
            path.clone(),
            now(),
            std::slice::from_ref(&odd),
        );
        recorder.add(item);
        recorder.finish().expect("writes");
        let history = load(&path).expect("load");
        assert_eq!(history.runs[0].roots, std::slice::from_ref(&odd));
        assert_eq!(history.runs[0].deleted[0].path, odd);
        assert_eq!(still_in_trash(&history, &[odd]).len(), 1);
    }

    #[test]
    fn restores_subtract_from_the_totals() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);
        append(
            &path,
            &path,
            Run::new(now(), &[], vec![entry(100), entry(50)]),
        )
        .expect("delete");
        let deleted = load(&path).expect("load").runs[0].deleted[1].clone();
        append(
            &path,
            &path,
            Run::restore(now(), vec![Entry::restored(&deleted, "s1.2".to_owned())]),
        )
        .expect("restore");

        let history = load(&path).expect("reload");
        assert_eq!(history.cumulative.bytes, 100);
        assert_eq!(history.cumulative.items, 1);
        assert_eq!(history.cumulative.restored_bytes, 50);
        assert_eq!(history.restored_ids(), HashSet::from(["s1.2"]));
        let kept = still_in_trash(&history, &[PathBuf::from("/tmp/a/node_modules")]);
        assert_eq!(kept, HashSet::from([(0, 1)]));
    }

    #[test]
    fn counts_trashed_bytes_as_freed_only_once_they_leave_the_trash() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);
        let mut permanent = entry(1000);
        permanent.mode = Mode::Permanent;
        append(
            &path,
            &path,
            Run::new(now(), &[], vec![entry(100), entry(10), permanent]),
        )
        .expect("delete");
        let restored = load(&path).expect("load").runs[0].deleted[1].clone();
        append(
            &path,
            &path,
            Run::restore(now(), vec![Entry::restored(&restored, "s1.2".to_owned())]),
        )
        .expect("restore");
        let history = load(&path).expect("reload");

        assert_eq!(history.cumulative.bytes, 1100);
        assert_eq!(history.cumulative.permanent_bytes, 1000);
        assert_eq!(history.cumulative.trashed_bytes, 100);
        let trash = |kept| TrashSnapshot::new(&history, kept);
        assert_eq!(freed_bytes(&history, &trash(None)), 1000);
        assert_eq!(
            freed_bytes(&history, &trash(Some(HashSet::from([(0, 0)])))),
            1000
        );
        assert_eq!(freed_bytes(&history, &trash(Some(HashSet::new()))), 1100);
    }

    #[test]
    fn matches_the_trash_item_closest_in_time() {
        let history = History {
            next_session: 2,
            runs: vec![Run::new(
                now(),
                &[],
                vec![
                    entry_at("/w/a", "2026-10-15T12:00:00+00:00"),
                    entry_at("/w/a", "2026-10-15T14:00:00+00:00"),
                    entry_at("/w/b", "2026-10-15T12:00:00+00:00"),
                    entry_at("/w/c", "2026-10-15T12:00:00+00:00"),
                ],
            )],
            cumulative: Cumulative::default(),
        };
        let trash = [
            record("/w/a", Some("2026-10-15T14:00:03")),
            record("/w/a", Some("2026-10-15T12:00:01")),
            record("/w/b", Some("2026-10-15T13:00:00")),
            record("/w/c", None),
        ];
        let targets = [(0, 0), (0, 1), (0, 2), (0, 3)];

        let matched = match_trash(&history, &targets, &trash);

        assert_eq!(matched, [Some(1), Some(0), None, Some(3)]);
    }

    #[test]
    fn matches_by_wall_clock_time_after_a_time_zone_change() {
        let history = History {
            next_session: 2,
            runs: vec![Run::new(
                now(),
                &[],
                vec![
                    entry_at("/w/a", "2026-10-15T12:00:00+02:00"),
                    entry_at("/w/b", "2026-03-29T01:59:00+01:00"),
                ],
            )],
            cumulative: Cumulative::default(),
        };
        let trash = [
            record("/w/a", Some("2026-10-15T12:00:02")),
            record("/w/b", Some("2026-03-29T01:59:00")),
        ];

        let matched = match_trash(&history, &[(0, 0), (0, 1)], &trash);

        assert_eq!(matched, [Some(0), Some(1)]);
    }

    #[test]
    fn trash_record_keeps_the_wall_clock_time() {
        let zone = jiff::tz::TimeZone::system();
        let wall: jiff::civil::DateTime = "2026-10-15T12:00:00".parse().unwrap();
        let unix = wall.to_zoned(zone).unwrap().timestamp().as_second();

        let record = TrashRecord::new(PathBuf::from("/w/a"), unix);

        assert_eq!(record.deleted_at, Some(wall));
        assert_eq!(TrashRecord::new(PathBuf::from("/w/a"), -1).deleted_at, None);
    }

    #[test]
    fn append_only_keeps_file_inode_and_appends_records() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);
        append(&path, &path, Run::new(now(), &[], vec![entry(1)])).expect("first");
        #[cfg(unix)]
        let inode_before = std::os::unix::fs::MetadataExt::ino(&fs::metadata(&path).expect("meta"));
        let len_before = fs::metadata(&path).expect("meta").len();

        let mut recorder = Recorder::at(path.clone(), path.clone(), now(), &[]);
        recorder.add(entry(2));
        recorder.add(entry(3));
        recorder.flush();
        recorder.add(entry(4));
        recorder.finish().expect("finish");

        #[cfg(unix)]
        let inode_after = std::os::unix::fs::MetadataExt::ino(&fs::metadata(&path).expect("meta"));
        let len_after = fs::metadata(&path).expect("meta").len();
        #[cfg(unix)]
        assert_eq!(inode_before, inode_after);
        assert!(len_after > len_before);

        let text = fs::read_to_string(&path).expect("read");
        assert!(text.lines().count() >= 3);
        let history = load(&path).expect("load");
        assert_eq!(history.runs.len(), 2);
        assert_eq!(history.runs[1].deleted.len(), 3);
        assert_eq!(history.runs[1].total.bytes, 9);
    }

    #[test]
    fn automatic_compaction_caps_file_size_and_keeps_newest_run() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);
        let cap = 900;
        for idx in 1..=6u64 {
            let mut item = entry(idx * 10);
            item.path = PathBuf::from(format!("/tmp/project-{idx}/node_modules"));
            store::append_with_cap(&path, &path, Run::new(now(), &[], vec![item]), cap, false)
                .expect("append");
        }
        let len = fs::metadata(&path).expect("meta").len();
        assert!(len <= cap, "expected compacted file <= {cap}, got {len}");
        let history = load(&path).expect("load");
        assert!(!history.runs.is_empty());
        assert!(history.runs.len() < 6);
        assert_eq!(history.runs.last().expect("newest").id, 6);
        assert_eq!(history.next_session, 7);
    }

    #[test]
    fn prune_trims_by_keep_age_and_size_without_reusing_ids() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);

        let mut old_run = Run::new(
            "2020-01-01T00:00:00Z".to_owned(),
            &[],
            vec![entry_at("/tmp/old", "2020-01-01T00:00:00Z")],
        );
        old_run.finished_at = "2020-01-01T00:00:05Z".to_owned();
        append(&path, &path, old_run).expect("r1");
        append(&path, &path, Run::new(now(), &[], vec![entry(20)])).expect("r2");
        append(&path, &path, Run::new(now(), &[], vec![entry(30)])).expect("r3");

        let dry = prune(
            &path,
            &path,
            &PruneOptions {
                older_than: Some(Duration::from_secs(30 * 86_400)),
                dry_run: true,
                ..PruneOptions::default()
            },
        )
        .expect("dry prune");
        assert!(dry.dry_run);
        assert_eq!(dry.pruned_runs, 1);
        assert_eq!(dry.kept_runs, 2);
        assert_eq!(load(&path).expect("unchanged").runs.len(), 3);

        let report = prune(
            &path,
            &path,
            &PruneOptions {
                older_than: Some(Duration::from_secs(30 * 86_400)),
                keep: Some(1),
                ..PruneOptions::default()
            },
        )
        .expect("prune");
        assert_eq!(report.pruned_runs, 2);
        assert_eq!(report.kept_runs, 1);
        let kept = load(&path).expect("load");
        assert_eq!(kept.runs.len(), 1);
        assert_eq!(kept.runs[0].id, 3);
        assert_eq!(kept.next_session, 4);

        let empty = prune(
            &path,
            &path,
            &PruneOptions {
                keep: Some(0),
                ..PruneOptions::default()
            },
        )
        .expect("prune all");
        assert_eq!(empty.pruned_runs, 1);
        assert_eq!(empty.kept_runs, 0);
        let after = load(&path).expect("load empty");
        assert!(after.runs.is_empty());
        assert_eq!(after.next_session, 4);

        append(&path, &path, Run::new(now(), &[], vec![entry(40)])).expect("r4");
        let resumed = load(&path).expect("resumed");
        assert_eq!(resumed.runs.len(), 1);
        assert_eq!(resumed.runs[0].session(), "s4");
    }
}
