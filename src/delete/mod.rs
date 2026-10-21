mod guard;
#[cfg(target_os = "linux")]
mod tree;

#[cfg(not(target_os = "linux"))]
use std::fs;
use std::num::NonZero;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, PoisonError};
use std::thread;

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
    #[error("{0}")]
    Tool(String),
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

/// @name workers
/// @description Returns how many threads removal work should use: the number of cores, at most
/// four.
///
/// @example
/// let threads = delete::workers();
pub fn workers() -> usize {
    thread::available_parallelism()
        .map_or(1, NonZero::get)
        .min(4)
}

#[cfg(target_os = "linux")]
fn remove(path: &Path) -> std::io::Result<()> {
    tree::remove(path, workers())
}

#[cfg(not(target_os = "linux"))]
fn remove(path: &Path) -> std::io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

/// Serializes removals of paths that contain one another, so two workers never fight over a
/// folder and something inside it.
#[derive(Default)]
pub struct Lanes {
    active: Mutex<Vec<PathBuf>>,
    freed: Condvar,
}

pub struct Lane<'a> {
    lanes: &'a Lanes,
    path: PathBuf,
}

impl Lanes {
    /// @name Lanes::claim
    /// @description Waits until no running removal is the same as, inside or around `path`, then
    /// claims it until the returned lane is dropped.
    ///
    /// @example
    /// let _lane = lanes.claim(&target.path);
    /// delete(&guard, &target, mode)?;
    pub fn claim(&self, path: &Path) -> Lane<'_> {
        let mut active = self.active.lock().unwrap_or_else(PoisonError::into_inner);
        while active
            .iter()
            .any(|other| other.starts_with(path) || path.starts_with(other))
        {
            active = self
                .freed
                .wait(active)
                .unwrap_or_else(PoisonError::into_inner);
        }
        active.push(path.to_path_buf());
        Lane {
            lanes: self,
            path: path.to_path_buf(),
        }
    }
}

impl Drop for Lane<'_> {
    fn drop(&mut self) {
        let mut active = self
            .lanes
            .active
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(index) = active.iter().position(|path| *path == self.path) {
            active.swap_remove(index);
        }
        self.lanes.freed.notify_all();
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

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    #[test]
    fn nested_paths_wait_for_each_other_but_siblings_do_not() {
        let lanes = Lanes::default();
        let parent = lanes.claim(Path::new("/p/app"));
        let sibling = lanes.claim(Path::new("/p/other"));
        let (sender, receiver) = mpsc::channel();
        thread::scope(|scope| {
            scope.spawn(|| {
                let _inner = lanes.claim(Path::new("/p/app/node_modules"));
                sender.send(()).unwrap();
            });
            assert!(receiver.recv_timeout(Duration::from_millis(150)).is_err());
            drop(sibling);
            assert!(receiver.recv_timeout(Duration::from_millis(50)).is_err());
            drop(parent);
            assert!(receiver.recv_timeout(Duration::from_secs(10)).is_ok());
        });
    }
}
