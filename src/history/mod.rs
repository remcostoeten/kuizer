use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::delete::Mode;
use crate::scan::Candidate;
use crate::units::ByteSize;

const FILE_NAME: &str = ".kuizer-history.json";
const LEGACY_FILE_NAME: &str = ".sweep-history.json";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct History {
    pub runs: Vec<Run>,
    pub cumulative: Cumulative,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Run {
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
            started_at,
            finished_at: now(),
            command: std::env::args().collect::<Vec<_>>().join(" "),
            roots: roots.to_vec(),
            total: Total::of(&deleted),
            deleted,
        }
    }
}

impl History {
    fn recompute(&mut self) {
        let bytes = self.runs.iter().map(|run| run.total.bytes).sum();
        self.cumulative = Cumulative {
            runs: self.runs.len(),
            items: self.runs.iter().map(|run| run.total.items).sum(),
            bytes,
            size: size(bytes),
            first_run: self.runs.first().map(|run| run.started_at.clone()),
            last_run: self.runs.last().map(|run| run.started_at.clone()),
        };
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
    for (run_index, run) in history.runs.iter().enumerate().rev() {
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
    serde_json::from_str(&text).map_err(|source| HistoryError::Parse {
        path: path.to_owned(),
        source,
    })
}

/// @name append
/// @description Reads the history at `source`, adds a run, recomputes the cumulative totals and
/// writes the result atomically to `target` through a temporary sibling. `source` is never
/// modified when it differs from `target`.
///
/// @example
/// history::append(&source, &target, Run::new(started_at, &roots, entries))?;
pub fn append(source: &Path, path: &Path, run: Run) -> Result<(), HistoryError> {
    let mut history = load(source)?;
    history.runs.push(run);
    history.recompute();
    let write_error = |source| HistoryError::Write {
        path: path.to_owned(),
        source,
    };
    let text = serde_json::to_string_pretty(&history)
        .map_err(io::Error::other)
        .map_err(write_error)?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, text + "\n").map_err(write_error)?;
    fs::rename(&temporary, path).map_err(write_error)
}

/// @name save
/// @description Appends one run with its deleted entries to the default history file. A run that
/// deleted nothing is not recorded.
///
/// @example
/// history::save(started_at, &roots, deleted)?;
pub fn save(
    started_at: String,
    roots: &[PathBuf],
    deleted: Vec<Entry>,
) -> Result<(), HistoryError> {
    if deleted.is_empty() {
        return Ok(());
    }
    let home = dirs::home_dir().ok_or(HistoryError::NoHome)?;
    append(
        &readable_path_in(&home),
        &home.join(FILE_NAME),
        Run::new(started_at, roots, deleted),
    )
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
    fn matches_each_trashed_path_once_newest_first() {
        let mut permanent = entry(5);
        permanent.mode = Mode::Permanent;
        let history = History {
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
}
