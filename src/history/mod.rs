use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::delete::Mode;
use crate::scan::Candidate;
use crate::units::ByteSize;

mod ids;
mod recorder;
pub mod restore;
mod state;
mod store;
mod stored_path;
#[cfg(test)]
mod tests;
mod trash_match;

use ids::parse_id;
use recorder::record_session;
pub use recorder::{Recorder, append, record};
pub use state::{
    ItemState, Restored, TrashSnapshot, TrashState, TrashView, freed_bytes, item_state, restorable,
    still_in_trash,
};
pub use store::{
    DEFAULT_MAX_BYTES, PruneOptions, PruneReport, default_path, load, prune, prune_default,
    readable_path_in, write_path,
};
pub use trash_match::{TrashListing, TrashRecord, match_trash, trashed_paths};

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<String>,
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
    chrono::Local::now()
        .format("%Y-%m-%dT%H:%M:%S%:z")
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
            policy: None,
            roots: roots.to_vec(),
            total: Total::of(&deleted),
            deleted,
        }
    }

    pub fn with_policy(mut self, policy: Option<String>) -> Self {
        self.policy = policy;
        self
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
}
