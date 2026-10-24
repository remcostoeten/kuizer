use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

use crate::delete::Mode;
use crate::scan::Candidate;
use crate::units::ByteSize;

pub mod restore;

const FILE_NAME: &str = ".kuizer-history.json";
const LEGACY_FILE_NAME: &str = ".sweep-history.json";

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
    pub roots: Vec<PathBuf>,
    pub deleted: Vec<Entry>,
    pub total: Total,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub deleted_at: String,
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

/// @name freed_bytes
/// @description Sums the space that past runs actually freed: everything deleted permanently, plus
/// trashed entries that are no longer in the trash. Restored entries never count. With `kept`
/// unknown, because the trash cannot be read, trashed entries are left out.
///
/// @example
/// let kept = history::still_in_trash(&history, &trashed);
/// let freed = history::freed_bytes(&history, Some(&kept));
pub fn freed_bytes(history: &History, kept: Option<&HashSet<(usize, usize)>>) -> u64 {
    let restored = restored_items(history);
    history
        .runs
        .iter()
        .enumerate()
        .filter(|(_, run)| run.action == Action::Delete)
        .flat_map(|(run_index, run)| {
            run.deleted
                .iter()
                .enumerate()
                .map(move |(entry_index, entry)| (run_index, run.id, entry_index, entry))
        })
        .filter(|&(_, session, entry, _)| !restored.contains(&(session, entry + 1)))
        .filter(|&(run, _, entry, item)| match item.mode {
            Mode::Permanent => true,
            Mode::Trash => kept.is_some_and(|kept| !kept.contains(&(run, entry))),
            Mode::DryRun => false,
        })
        .map(|(_, _, _, entry)| entry.bytes)
        .sum()
}

fn restored_items(history: &History) -> HashSet<(u64, usize)> {
    history
        .restored_ids()
        .into_iter()
        .filter_map(|id| match parse_id(id)? {
            (session, Some(item)) => Some((session, item)),
            (_, None) => None,
        })
        .collect()
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

/// @name still_in_trash
/// @description Returns the `(run, entry)` indexes of trashed entries whose path is still in the
/// trash. Each trashed path is matched once, newest entries first, so a folder trashed twice and
/// emptied once counts once.
///
/// @example
/// let kept = history::still_in_trash(&history, &history::trashed_paths().unwrap_or_default());
pub fn still_in_trash(history: &History, trashed: &[PathBuf]) -> HashSet<(usize, usize)> {
    let mut remaining: HashMap<&Path, usize> = HashMap::new();
    for path in trashed {
        *remaining.entry(path.as_path()).or_default() += 1;
    }
    let mut found = HashSet::new();
    for (run_index, run) in history
        .runs
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, run)| run.action == Action::Delete)
    {
        for (entry_index, entry) in run.deleted.iter().enumerate().rev() {
            if entry.mode != Mode::Trash {
                continue;
            }
            if let Some(count) = remaining.get_mut(entry.path.as_path())
                && *count > 0
            {
                *count -= 1;
                found.insert((run_index, entry_index));
            }
        }
    }
    found
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemState {
    RestoredHere,
    Restored,
    Deleted,
    DryRun,
    Trashed,
    InTrash,
    LeftTrash,
}

impl ItemState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RestoredHere => "restored here",
            Self::Restored => "restored",
            Self::Deleted => "deleted",
            Self::DryRun => "dry run",
            Self::Trashed => "trashed",
            Self::InTrash => "in trash",
            Self::LeftTrash => "left trash",
        }
    }
}

/// @name item_state
/// @description Says what became of one history entry: restored, deleted permanently, a dry run,
/// still in the trash or gone from it. With `kept` unknown, because the trash cannot be read,
/// trashed entries are just `Trashed`.
///
/// @example
/// let state = history::item_state(&history, &history.restored_ids(), Some(&kept), 0, 2);
pub fn item_state(
    history: &History,
    restored: &HashSet<&str>,
    kept: Option<&HashSet<(usize, usize)>>,
    run: usize,
    entry: usize,
) -> ItemState {
    let is_restored = history.runs[run].action != Action::Restore
        && restored.contains(history.item_id(run, entry).as_str());
    state_of(history, is_restored, kept, run, entry)
}

fn state_of(
    history: &History,
    is_restored: bool,
    kept: Option<&HashSet<(usize, usize)>>,
    run: usize,
    entry: usize,
) -> ItemState {
    let item = &history.runs[run].deleted[entry];
    if history.runs[run].action == Action::Restore {
        return ItemState::RestoredHere;
    }
    if is_restored {
        return ItemState::Restored;
    }
    match (item.mode, kept) {
        (Mode::Permanent, _) => ItemState::Deleted,
        (Mode::DryRun, _) => ItemState::DryRun,
        (Mode::Trash, None) => ItemState::Trashed,
        (Mode::Trash, Some(kept)) if kept.contains(&(run, entry)) => ItemState::InTrash,
        (Mode::Trash, Some(_)) => ItemState::LeftTrash,
    }
}

/// @name restorable
/// @description Lists the entries of one run that can still be restored: trashed, not restored
/// yet and still in the trash according to `kept` from `still_in_trash`.
///
/// @example
/// let targets = history::restorable(&history, &kept, run_index);
/// let restore = history::restore::restore(&history, &targets, false)?;
pub fn restorable(
    history: &History,
    kept: &HashSet<(usize, usize)>,
    run: usize,
) -> Vec<(usize, usize)> {
    let restored = restored_items(history);
    let session = history.runs.get(run).map_or(0, |run| run.id);
    (0..history.runs.get(run).map_or(0, |run| run.deleted.len()))
        .filter(|&entry| {
            let is_restored = restored.contains(&(session, entry + 1));
            state_of(history, is_restored, Some(kept), run, entry) == ItemState::InTrash
        })
        .map(|entry| (run, entry))
        .collect()
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

/// @name default_path
/// @description Returns the history file to read: `~/.kuizer-history.json`, or the legacy
/// `~/.sweep-history.json` while only that one exists.
///
/// @example
/// let path = history::default_path()?;
pub fn default_path() -> Result<PathBuf, HistoryError> {
    dirs::home_dir()
        .map(|home| readable_path_in(&home))
        .ok_or(HistoryError::NoHome)
}

/// @name readable_path_in
/// @description Picks the history file to read inside `home`, falling back to the legacy
/// `.sweep-history.json` when `.kuizer-history.json` does not exist yet.
///
/// @example
/// let path = history::readable_path_in(&home);
pub fn readable_path_in(home: &Path) -> PathBuf {
    let current = home.join(FILE_NAME);
    let legacy = home.join(LEGACY_FILE_NAME);
    if !current.exists() && legacy.is_file() {
        legacy
    } else {
        current
    }
}

/// @name load
/// @description Reads the history file, returning an empty history when it does not exist yet.
///
/// @example
/// let history = history::load(&history::default_path()?)?;
pub fn load(path: &Path) -> Result<History, HistoryError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(History::default()),
        Err(source) => {
            return Err(HistoryError::Read {
                path: path.to_owned(),
                source,
            });
        }
    };
    let mut history: History =
        serde_json::from_str(&text).map_err(|source| HistoryError::Parse {
            path: path.to_owned(),
            source,
        })?;
    history.assign_ids();
    Ok(history)
}

/// @name append
/// @description Reads the history at `source`, adds a run, recomputes the cumulative totals and
/// writes the result atomically to `target` through a temporary sibling, holding a lock so two
/// kuizer processes never drop each other's runs. `source` is never modified when it differs
/// from `target`.
///
/// @example
/// history::append(&source, &target, Run::new(started_at, &roots, entries))?;
pub fn append(source: &Path, path: &Path, run: Run) -> Result<u64, HistoryError> {
    update(source, path, |history| {
        let mut run = run;
        run.id = history.next_session;
        history.next_session += 1;
        history.runs.push(run);
        history.next_session - 1
    })
}

fn update<T>(
    source: &Path,
    path: &Path,
    change: impl FnOnce(&mut History) -> T,
) -> Result<T, HistoryError> {
    let write_error = |source| HistoryError::Write {
        path: path.to_owned(),
        source,
    };
    let _lock = Lock::acquire(path).map_err(write_error)?;
    let mut history = load_for_update(source, path)?;
    history.assign_ids();
    let result = change(&mut history);
    history.recompute();
    write(path, &history).map_err(write_error)?;
    Ok(result)
}

fn load_for_update(source: &Path, target: &Path) -> Result<History, HistoryError> {
    match load(source) {
        Err(error @ HistoryError::Parse { .. }) => {
            if source == target && !set_aside(source) {
                return Err(error);
            }
            Ok(History::default())
        }
        other => other,
    }
}

fn set_aside(path: &Path) -> bool {
    let seconds = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    fs::rename(path, path.with_extension(format!("json.corrupt-{seconds}"))).is_ok()
}

fn write(path: &Path, history: &History) -> io::Result<()> {
    let mut text = serde_json::to_string(history).map_err(io::Error::other)?;
    text.push('\n');
    let temporary = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let saved = fs::File::create(&temporary)
        .and_then(|mut file| {
            file.write_all(text.as_bytes())?;
            file.sync_all()
        })
        .and_then(|()| fs::rename(&temporary, path));
    if saved.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    saved
}

struct Lock {
    _file: fs::File,
}

impl Lock {
    fn acquire(path: &Path) -> io::Result<Self> {
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path.with_extension("json.lock"))?;
        #[cfg(unix)]
        rustix::fs::flock(&file, rustix::fs::FlockOperation::LockExclusive)?;
        Ok(Self { _file: file })
    }
}

const FLUSH_AFTER: Duration = Duration::from_millis(100);
const FLUSH_AFTER_MAX: Duration = Duration::from_secs(5);
const WRITE_SHARE: u32 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    identity: u64,
    length: u64,
    modified: Option<SystemTime>,
}

impl Stamp {
    fn of(path: &Path) -> Option<Self> {
        let metadata = fs::metadata(path).ok()?;
        #[cfg(unix)]
        let identity = std::os::unix::fs::MetadataExt::ino(&metadata);
        #[cfg(not(unix))]
        let identity = 0;
        Some(Self {
            identity,
            length: metadata.len(),
            modified: metadata.modified().ok(),
        })
    }
}

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

struct Store {
    source: PathBuf,
    target: PathBuf,
    started_at: String,
    roots: Vec<PathBuf>,
    written: Option<(History, Stamp)>,
}

impl Store {
    fn write(&mut self, entries: Vec<Entry>, session: Option<u64>) -> Result<u64, HistoryError> {
        let write_error = |source| HistoryError::Write {
            path: self.target.clone(),
            source,
        };
        let _lock = Lock::acquire(&self.target).map_err(write_error)?;
        let current = Stamp::of(&self.target);
        let reusable = self
            .written
            .take()
            .filter(|(_, stamp)| Some(*stamp) == current)
            .map(|(history, _)| history);
        let mut history = match reusable {
            Some(history) => history,
            None => {
                let source = if session.is_some() {
                    &self.target
                } else {
                    &self.source
                };
                let mut history = load_for_update(source, &self.target)?;
                history.assign_ids();
                history
            }
        };
        let open =
            session.and_then(|session| history.runs.iter_mut().rev().find(|run| run.id == session));
        let session = match open {
            Some(run) => {
                run.deleted.extend(entries);
                run.total = Total::of(&run.deleted);
                run.finished_at = now();
                run.id
            }
            None => {
                let mut run = Run::new(self.started_at.clone(), &self.roots, entries);
                run.id = history.next_session;
                history.next_session += 1;
                history.runs.push(run);
                history.next_session - 1
            }
        };
        history.recompute();
        write(&self.target, &history).map_err(write_error)?;
        self.written = Stamp::of(&self.target).map(|stamp| (history, stamp));
        Ok(session)
    }
}

struct Shared {
    queue: Mutex<Queue>,
    store: Mutex<Store>,
    wake: Condvar,
}

impl Shared {
    fn queue(&self) -> MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn flush(&self) {
        let mut store = self.store.lock().unwrap_or_else(PoisonError::into_inner);
        let (entries, session) = {
            let mut queue = self.queue();
            if queue.pending.is_empty() {
                return;
            }
            queue.oldest = None;
            (std::mem::take(&mut queue.pending), queue.session)
        };
        let started = Instant::now();
        let result = store.write(entries, session);
        let mut queue = self.queue();
        match result {
            Ok(session) => {
                queue.session = Some(session);
                queue.write_cost = started.elapsed();
            }
            Err(error) => {
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
        let home = dirs::home_dir();
        let recorder = Self::at(
            home.as_deref().map(readable_path_in).unwrap_or_default(),
            home.map(|home| home.join(FILE_NAME)).unwrap_or_default(),
            started_at,
            roots,
        );
        {
            let no_home = recorder
                .shared
                .store
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .target
                .as_os_str()
                .is_empty();
            if no_home {
                recorder.shared.queue().error = Some(HistoryError::NoHome);
            }
        }
        recorder
    }

    pub fn at(source: PathBuf, target: PathBuf, started_at: String, roots: &[PathBuf]) -> Self {
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
                store: Mutex::new(Store {
                    source,
                    target,
                    started_at,
                    roots: roots.to_vec(),
                    written: None,
                }),
                wake: Condvar::new(),
            }),
            flusher: None,
        }
    }

    /// @name add
    /// @description Queues one removed entry for the history file. The first entry opens the run
    /// and is written straight away. Later entries are written together by a background thread
    /// once the oldest has waited 100 ms, or longer when writing the file is slow so that writing
    /// takes at most about a twentieth of the time, and always by `flush`, `finish` and drop, so an
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

    pub fn finish(self) -> Result<(), HistoryError> {
        self.shared.flush();
        let error = self.shared.queue().error.take();
        error.map_or(Ok(()), Err)
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
    let home = dirs::home_dir().ok_or(HistoryError::NoHome)?;
    append(&readable_path_in(&home), &home.join(FILE_NAME), run).map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(text.find("\"runs\"") < text.find("\"cumulative\""));
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
    fn only_trashed_entries_still_in_the_trash_are_restorable() {
        let mut permanent = entry(5);
        permanent.mode = Mode::Permanent;
        let mut history = History {
            next_session: 0,
            runs: vec![Run::new(
                now(),
                &[],
                vec![entry(1), entry(2), entry(3), permanent],
            )],
            cumulative: Cumulative::default(),
        };
        history.assign_ids();
        let back = Entry::restored(&history.runs[0].deleted[1], "s1.2".to_owned());
        history.runs.push(Run::restore(now(), vec![back]));
        history.assign_ids();
        let kept = HashSet::from([(0, 0), (0, 1)]);
        let restored = history.restored_ids();

        assert_eq!(restorable(&history, &kept, 0), vec![(0, 0)]);
        assert!(restorable(&history, &kept, 1).is_empty());
        assert!(restorable(&history, &kept, 9).is_empty());
        let states: Vec<ItemState> = (0..4)
            .map(|entry| item_state(&history, &restored, Some(&kept), 0, entry))
            .collect();
        assert_eq!(
            states,
            vec![
                ItemState::InTrash,
                ItemState::Restored,
                ItemState::LeftTrash,
                ItemState::Deleted
            ]
        );
        assert_eq!(
            item_state(&history, &restored, None, 0, 2),
            ItemState::Trashed
        );
        assert_eq!(
            item_state(&history, &restored, Some(&kept), 1, 0).as_str(),
            "restored here"
        );
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
    fn matches_each_trashed_path_once_newest_first() {
        let mut permanent = entry(5);
        permanent.mode = Mode::Permanent;
        let history = History {
            next_session: 0,
            runs: vec![
                Run::new(now(), &[], vec![entry(1)]),
                Run::new(now(), &[], vec![entry(2), permanent]),
            ],
            cumulative: Cumulative::default(),
        };
        let trashed = [PathBuf::from("/tmp/a/node_modules")];
        let kept = still_in_trash(&history, &trashed);
        assert_eq!(kept, HashSet::from([(1, 0)]));
        assert!(still_in_trash(&history, &[]).is_empty());
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
        let legacy = home.path().join(LEGACY_FILE_NAME);
        let current = home.path().join(FILE_NAME);
        assert_eq!(readable_path_in(home.path()), current);

        append(&legacy, &legacy, Run::new(now(), &[], vec![entry(1)])).expect("legacy");
        assert_eq!(readable_path_in(home.path()), legacy);

        append(
            &readable_path_in(home.path()),
            &current,
            Run::new(now(), &[], vec![entry(2)]),
        )
        .expect("migrate");
        assert_eq!(readable_path_in(home.path()), current);
        assert_eq!(load(&current).expect("current").runs.len(), 2);
        assert_eq!(load(&legacy).expect("legacy kept").runs.len(), 1);
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
        assert_eq!(freed_bytes(&history, None), 1000);
        assert_eq!(freed_bytes(&history, Some(&HashSet::from([(0, 0)]))), 1000);
        assert_eq!(freed_bytes(&history, Some(&HashSet::new())), 1100);
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
}
