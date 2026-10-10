mod guard;

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use guard::{Guard, Refusal};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    Trash,
    Permanent,
    DryRun,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Trash => "move to trash",
            Self::Permanent => "delete permanently",
            Self::DryRun => "dry run",
        }
    }
}

impl From<crate::config::DeleteMode> for Mode {
    fn from(mode: crate::config::DeleteMode) -> Self {
        match mode {
            crate::config::DeleteMode::Trash => Self::Trash,
            crate::config::DeleteMode::Permanent => Self::Permanent,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DeleteError {
    #[error("refused: {0}")]
    Refused(#[from] Refusal),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("trash: {0}")]
    Trash(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    WouldRemove,
    Removed,
    Failed,
}

#[derive(Debug, Clone)]
pub struct Removal {
    pub candidate: crate::scan::Candidate,
    pub status: Status,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Target {
    pub path: PathBuf,
    pub identity: Option<(u64, u64)>,
}

fn remove(path: &Path) -> std::io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

/// @name delete
/// @description Revalidates a scanned target against the guard right before acting, then
/// moves it to the trash, removes it, or does nothing in dry-run mode.
///
/// @example
/// let guard = Guard::new(&roots);
/// delete(&guard, &target, Mode::Trash)?;
pub fn delete(guard: &Guard, target: &Target, mode: Mode) -> Result<(), DeleteError> {
    guard.check(&target.path, target.identity)?;
    match mode {
        Mode::DryRun => Ok(()),
        Mode::Permanent => Ok(remove(&target.path)?),
        Mode::Trash => trash::delete(&target.path).map_err(|e| DeleteError::Trash(e.to_string())),
    }
}
