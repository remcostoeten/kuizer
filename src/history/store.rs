use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use super::{Action, Entry, History, HistoryError, Run, Total, now, stored_path};

pub(super) const STATE_FILE_NAME: &str = "history.jsonl";
pub(super) const LEGACY_KUIZER_FILE: &str = ".kuizer-history.json";
pub(super) const LEGACY_SWEEP_FILE: &str = ".sweep-history.json";

/// Maximum history file size before automatic compaction trims the oldest runs.
pub const DEFAULT_MAX_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum LogRecord {
    /// Preserves the next session id across prunes and compactions so ids are never reused.
    Meta { next_session: u64 },
    /// Opens a session or stores a complete run on one line.
    Run {
        id: u64,
        #[serde(default)]
        action: Action,
        started_at: String,
        finished_at: String,
        command: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        policy: Option<String>,
        #[serde(with = "stored_path::many")]
        roots: Vec<PathBuf>,
        entries: Vec<Entry>,
    },
    /// Appends additional entries to an already-open session without rewriting the file.
    Entries {
        id: u64,
        finished_at: String,
        entries: Vec<Entry>,
    },
}

impl LogRecord {
    fn from_run(run: &Run) -> Self {
        Self::Run {
            id: run.id,
            action: run.action,
            started_at: run.started_at.clone(),
            finished_at: run.finished_at.clone(),
            command: run.command.clone(),
            policy: run.policy.clone(),
            roots: run.roots.clone(),
            entries: run.deleted.clone(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PruneOptions {
    pub keep: Option<usize>,
    pub older_than: Option<Duration>,
    pub max_bytes: Option<u64>,
    pub dry_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PruneReport {
    pub path: PathBuf,
    pub pruned_runs: usize,
    pub kept_runs: usize,
    pub bytes_before: u64,
    pub bytes_after: u64,
    pub dry_run: bool,
}

pub(super) fn state_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        dirs::data_dir().map(|dir| dir.join("kuizer"))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        dirs::state_dir().map(|dir| dir.join("kuizer"))
    }
    #[cfg(not(unix))]
    {
        dirs::data_local_dir().map(|dir| dir.join("kuizer"))
    }
}

pub(super) fn state_path_in(home: &Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        home.join("Library")
            .join("Application Support")
            .join("kuizer")
            .join(STATE_FILE_NAME)
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        home.join(".local")
            .join("state")
            .join("kuizer")
            .join(STATE_FILE_NAME)
    }
    #[cfg(not(unix))]
    {
        home.join("kuizer").join(STATE_FILE_NAME)
    }
}

pub fn write_path() -> Result<PathBuf, HistoryError> {
    state_dir()
        .map(|dir| dir.join(STATE_FILE_NAME))
        .ok_or(HistoryError::NoHome)
}

pub(super) fn readable_path_for(target: &Path, home: &Path) -> PathBuf {
    if target.exists() {
        return target.to_path_buf();
    }
    let legacy_kuizer = home.join(LEGACY_KUIZER_FILE);
    if legacy_kuizer.is_file() {
        return legacy_kuizer;
    }
    let legacy_sweep = home.join(LEGACY_SWEEP_FILE);
    if legacy_sweep.is_file() {
        return legacy_sweep;
    }
    target.to_path_buf()
}

pub fn default_path() -> Result<PathBuf, HistoryError> {
    let target = write_path()?;
    let home = dirs::home_dir().ok_or(HistoryError::NoHome)?;
    Ok(readable_path_for(&target, &home))
}

pub fn readable_path_in(home: &Path) -> PathBuf {
    readable_path_for(&state_path_in(home), home)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Format {
    Empty,
    Jsonl,
    Legacy,
}

pub(super) fn parse_history(path: &Path, text: &str) -> Result<(History, Format), HistoryError> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok((History::default(), Format::Empty));
    }
    let first_line = trimmed.lines().next().unwrap_or(trimmed).trim();
    if serde_json::from_str::<LogRecord>(first_line).is_ok() {
        let mut history = History::default();
        let mut by_id: HashMap<u64, usize> = HashMap::new();
        let mut next_session = 1u64;
        for line in trimmed.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let record: LogRecord =
                serde_json::from_str(line).map_err(|source| HistoryError::Parse {
                    path: path.to_owned(),
                    source,
                })?;
            match record {
                LogRecord::Meta { next_session: next } => {
                    next_session = next_session.max(next);
                }
                LogRecord::Run {
                    id,
                    action,
                    started_at,
                    finished_at,
                    command,
                    policy,
                    roots,
                    entries,
                } => {
                    let total = Total::of(&entries);
                    let run = Run {
                        id,
                        action,
                        started_at,
                        finished_at,
                        command,
                        policy,
                        roots,
                        deleted: entries,
                        total,
                    };
                    let idx = history.runs.len();
                    if id > 0 {
                        by_id.insert(id, idx);
                        next_session = next_session.max(id + 1);
                    }
                    history.runs.push(run);
                }
                LogRecord::Entries {
                    id,
                    finished_at,
                    entries,
                } => {
                    next_session = next_session.max(id + 1);
                    if let Some(&idx) = by_id.get(&id) {
                        let run = &mut history.runs[idx];
                        run.deleted.extend(entries);
                        run.total = Total::of(&run.deleted);
                        run.finished_at = finished_at;
                    }
                }
            }
        }
        history.next_session = history.next_session.max(next_session);
        history.assign_ids();
        history.recompute();
        return Ok((history, Format::Jsonl));
    }

    let mut history: History =
        serde_json::from_str(trimmed).map_err(|source| HistoryError::Parse {
            path: path.to_owned(),
            source,
        })?;
    history.assign_ids();
    history.recompute();
    Ok((history, Format::Legacy))
}

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
    parse_history(path, &text).map(|(history, _)| history)
}

pub(super) fn load_with_format(path: &Path) -> Result<(History, Format), HistoryError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok((History::default(), Format::Empty));
        }
        Err(source) => {
            return Err(HistoryError::Read {
                path: path.to_owned(),
                source,
            });
        }
    };
    parse_history(path, &text)
}

pub(super) fn load_for_update(
    source: &Path,
    target: &Path,
) -> Result<(History, Format), HistoryError> {
    match load_with_format(source) {
        Err(error @ HistoryError::Parse { .. }) => {
            if source == target && !set_aside(source) {
                return Err(error);
            }
            Ok((History::default(), Format::Empty))
        }
        other => other,
    }
}

fn set_aside(path: &Path) -> bool {
    let seconds = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    fs::rename(path, path.with_extension(format!("corrupt-{seconds}"))).is_ok()
}

pub(super) fn serialize_jsonl(history: &History) -> io::Result<String> {
    let mut out = String::new();
    let max_id = history.runs.iter().map(|run| run.id).max().unwrap_or(0);
    if history.next_session > max_id + 1 {
        let meta = LogRecord::Meta {
            next_session: history.next_session,
        };
        out.push_str(&serde_json::to_string(&meta).map_err(io::Error::other)?);
        out.push('\n');
    }
    for run in &history.runs {
        let record = LogRecord::from_run(run);
        out.push_str(&serde_json::to_string(&record).map_err(io::Error::other)?);
        out.push('\n');
    }
    Ok(out)
}

fn write_compacted(path: &Path, history: &History) -> io::Result<()> {
    let text = serialize_jsonl(history)?;
    crate::platform::write_atomic(path, text.as_bytes())
}

fn append_record(path: &Path, record: &LogRecord) -> io::Result<()> {
    let mut line = serde_json::to_string(record).map_err(io::Error::other)?;
    line.push('\n');
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(line.as_bytes())?;
    file.sync_data()
}

pub(super) struct Lock {
    _file: fs::File,
}

impl Lock {
    pub(super) fn acquire(path: &Path, create_parent: bool) -> io::Result<Self> {
        if create_parent
            && let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path.with_extension("lock"))?;
        #[cfg(unix)]
        rustix::fs::flock(&file, rustix::fs::FlockOperation::LockExclusive)?;
        Ok(Self { _file: file })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Stamp {
    identity: u64,
    length: u64,
    modified: Option<SystemTime>,
}

impl Stamp {
    pub(super) fn of(path: &Path) -> Option<Self> {
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

    pub(super) fn same_file(self, other: Self) -> bool {
        #[cfg(unix)]
        {
            self.identity == other.identity && other.length >= self.length
        }
        #[cfg(not(unix))]
        {
            other.length >= self.length
        }
    }
}

fn compact_to_cap(history: &mut History, max_bytes: u64, keep_newest: bool) -> io::Result<String> {
    let min_keep = usize::from(keep_newest && !history.runs.is_empty());
    loop {
        let text = serialize_jsonl(history)?;
        if text.len() as u64 <= max_bytes || history.runs.len() <= min_keep {
            return Ok(text);
        }
        history.runs.remove(0);
        history.recompute();
    }
}

fn maybe_compact(path: &Path, max_bytes: u64) -> Result<(), HistoryError> {
    let len = fs::metadata(path).map_or(0, |meta| meta.len());
    if len <= max_bytes {
        return Ok(());
    }
    let write_error = |source| HistoryError::Write {
        path: path.to_owned(),
        source,
    };
    let mut history = load(path)?;
    let text = compact_to_cap(&mut history, max_bytes, true).map_err(write_error)?;
    crate::platform::write_atomic(path, text.as_bytes()).map_err(write_error)
}

fn prepare_target(source: &Path, target: &Path) -> Result<(History, bool), HistoryError> {
    let write_error = |source| HistoryError::Write {
        path: target.to_owned(),
        source,
    };
    let exists = target.exists();
    let read_from = if exists { target } else { source };
    let (mut history, format) = load_for_update(read_from, target)?;
    history.assign_ids();
    let needs_rewrite = (!exists && source != target && !history.runs.is_empty())
        || (exists && format == Format::Legacy);
    if needs_rewrite {
        write_compacted(target, &history).map_err(write_error)?;
    }
    Ok((history, needs_rewrite))
}

pub(super) fn append_with_cap(
    source: &Path,
    path: &Path,
    mut run: Run,
    max_bytes: u64,
    create_parent: bool,
) -> Result<u64, HistoryError> {
    let write_error = |source| HistoryError::Write {
        path: path.to_owned(),
        source,
    };
    let _lock = Lock::acquire(path, create_parent).map_err(write_error)?;
    let (history, _) = prepare_target(source, path)?;
    run.id = history.next_session;
    let id = run.id;
    let record = LogRecord::from_run(&run);
    append_record(path, &record).map_err(write_error)?;
    maybe_compact(path, max_bytes)?;
    Ok(id)
}

pub(super) struct Store {
    pub(super) source: PathBuf,
    pub(super) target: PathBuf,
    pub(super) started_at: String,
    pub(super) policy: Option<String>,
    pub(super) roots: Vec<PathBuf>,
    pub(super) max_bytes: u64,
    pub(super) create_parent: bool,
    pub(super) written_stamp: Option<Stamp>,
    pub(super) active_run: Option<Run>,
}

impl Store {
    pub(super) fn write(
        &mut self,
        entries: &[Entry],
        session: Option<u64>,
    ) -> Result<u64, HistoryError> {
        let write_error = |source| HistoryError::Write {
            path: self.target.clone(),
            source,
        };
        let _lock = Lock::acquire(&self.target, self.create_parent).map_err(write_error)?;
        let current_stamp = Stamp::of(&self.target);

        // Fast path: append entries to an already-open session in the same JSONL file without
        // re-reading or rewriting existing history.
        if let (Some(id), Some(prev), Some(curr)) = (session, self.written_stamp, current_stamp)
            && prev.same_file(curr)
        {
            let finished_at = now();
            let record = LogRecord::Entries {
                id,
                finished_at: finished_at.clone(),
                entries: entries.to_vec(),
            };
            append_record(&self.target, &record).map_err(write_error)?;
            if let Some(run) = &mut self.active_run {
                run.deleted.extend_from_slice(entries);
                run.total = Total::of(&run.deleted);
                run.finished_at = finished_at;
            }
            maybe_compact(&self.target, self.max_bytes)?;
            self.written_stamp = Stamp::of(&self.target);
            return Ok(id);
        }

        let read_source = if session.is_some() {
            &self.target
        } else {
            &self.source
        };
        let (history, _) = prepare_target(read_source, &self.target)?;
        let existing = session.filter(|id| history.runs.iter().any(|run| run.id == *id));
        let id = match existing {
            Some(id) => {
                let finished_at = now();
                let record = LogRecord::Entries {
                    id,
                    finished_at: finished_at.clone(),
                    entries: entries.to_vec(),
                };
                append_record(&self.target, &record).map_err(write_error)?;
                if let Some(run) = &mut self.active_run {
                    run.deleted.extend_from_slice(entries);
                    run.total = Total::of(&run.deleted);
                    run.finished_at = finished_at;
                }
                id
            }
            None => {
                let mut run = Run::new(self.started_at.clone(), &self.roots, entries.to_vec())
                    .with_policy(self.policy.clone());
                run.id = history.next_session;
                let id = run.id;
                let record = LogRecord::from_run(&run);
                append_record(&self.target, &record).map_err(write_error)?;
                self.active_run = Some(run);
                id
            }
        };
        maybe_compact(&self.target, self.max_bytes)?;
        self.written_stamp = Stamp::of(&self.target);
        Ok(id)
    }
}

fn run_timestamp(run: &Run) -> Option<jiff::Timestamp> {
    run.finished_at
        .parse::<jiff::Timestamp>()
        .ok()
        .or_else(|| run.started_at.parse::<jiff::Timestamp>().ok())
}

pub fn prune(
    source: &Path,
    target: &Path,
    options: &PruneOptions,
) -> Result<PruneReport, HistoryError> {
    prune_inner(source, target, options, false)
}

pub fn prune_default(options: &PruneOptions) -> Result<PruneReport, HistoryError> {
    let target = write_path()?;
    let home = dirs::home_dir().ok_or(HistoryError::NoHome)?;
    let source = readable_path_for(&target, &home);
    prune_inner(&source, &target, options, true)
}

fn prune_inner(
    source: &Path,
    target: &Path,
    options: &PruneOptions,
    create_parent: bool,
) -> Result<PruneReport, HistoryError> {
    let write_error = |source| HistoryError::Write {
        path: target.to_owned(),
        source,
    };
    let _lock = if options.dry_run {
        None
    } else {
        Some(Lock::acquire(target, create_parent).map_err(write_error)?)
    };
    let read_from = if target.exists() { target } else { source };
    let bytes_before = fs::metadata(read_from).map_or(0, |meta| meta.len());
    let mut history = load(read_from)?;
    let before_runs = history.runs.len();

    if let Some(age) = options.older_than {
        let age = jiff::SignedDuration::try_from(age).unwrap_or(jiff::SignedDuration::MAX);
        if let Ok(cutoff) = jiff::Timestamp::now().checked_sub(age) {
            history
                .runs
                .retain(|run| run_timestamp(run).is_none_or(|ts| ts >= cutoff));
        }
    }

    if let Some(keep) = options.keep
        && history.runs.len() > keep
    {
        let drop_count = history.runs.len() - keep;
        history.runs.drain(..drop_count);
    }

    history.recompute();

    let cap = options.max_bytes.or_else(|| {
        (options.keep.is_none() && options.older_than.is_none()).then_some(DEFAULT_MAX_BYTES)
    });
    let serialized = match cap {
        Some(cap) => compact_to_cap(&mut history, cap, false).map_err(write_error)?,
        None => serialize_jsonl(&history).map_err(write_error)?,
    };

    let kept_runs = history.runs.len();
    let pruned_runs = before_runs.saturating_sub(kept_runs);
    let bytes_after = serialized.len() as u64;

    if !options.dry_run
        && (pruned_runs > 0
            || read_from != target
            || bytes_after != bytes_before
            || (!target.exists() && before_runs > 0))
    {
        crate::platform::write_atomic(target, serialized.as_bytes()).map_err(write_error)?;
    }

    Ok(PruneReport {
        path: target.to_owned(),
        pruned_runs,
        kept_runs,
        bytes_before,
        bytes_after,
        dry_run: options.dry_run,
    })
}
