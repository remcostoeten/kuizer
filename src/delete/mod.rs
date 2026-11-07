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

#[derive(Debug, thiserror::Error)]
pub enum DeleteError {
    #[error("refused: {0}")]
    Refused(#[from] Refusal),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("trash: {0}")]
    Trash(String),
    #[error("{0}")]
    Permission(String),
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

impl From<&crate::scan::Candidate> for Target {
    fn from(candidate: &crate::scan::Candidate) -> Self {
        Self {
            path: candidate.path.clone(),
            identity: candidate.identity,
        }
    }
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
fn remove(path: &Path, identity: Option<(u64, u64)>) -> std::io::Result<()> {
    tree::remove(path, identity, workers())
}

#[cfg(not(target_os = "linux"))]
fn remove(path: &Path, _identity: Option<(u64, u64)>) -> std::io::Result<()> {
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
/// removes it, or does nothing in dry-run mode. `Mode::Trash` only remains for runs recorded by
/// older versions; no frontend asks for it.
///
/// @example
/// let guard = Guard::new(&roots);
/// delete(&guard, &target, Mode::Trash)?;
pub fn delete(guard: &Guard, target: &Target, mode: Mode) -> Result<(), DeleteError> {
    guard.check(&target.path, target.identity)?;
    if mode != Mode::DryRun {
        writable(&target.path, mode)?;
    }
    match mode {
        Mode::DryRun => Ok(()),
        Mode::Permanent => Ok(remove(&target.path, target.identity)?),
        Mode::Trash => trash::delete(&target.path).map_err(|e| match e {
            trash::Error::FileSystem { source, .. } => DeleteError::Io(source),
            other => DeleteError::Trash(other.to_string()),
        }),
    }
}

/// Removing an entry needs write access to its folder, and emptying or (on Linux) moving a
/// folder needs write access to the folder itself. Checking up front turns a raw `EACCES` deep
/// inside the trash or the tree walk into a reason the user can act on.
#[cfg(unix)]
fn writable(path: &Path, mode: Mode) -> Result<(), DeleteError> {
    use std::os::unix::fs::MetadataExt;

    use rustix::fs::{Access, access};

    let blocked = |at: &Path| access(at, Access::WRITE_OK).is_err();
    let metadata = std::fs::symlink_metadata(path)?;
    let inner = metadata.is_dir() && (mode == Mode::Permanent || cfg!(target_os = "linux"));
    let culprit = if inner && blocked(path) {
        Some((path, metadata.uid()))
    } else {
        path.parent()
            .filter(|parent| blocked(parent))
            .and_then(|parent| Some((parent, std::fs::metadata(parent).ok()?.uid())))
    };
    let Some((at, uid)) = culprit else {
        return Ok(());
    };
    let place = if at == path {
        "it"
    } else {
        "its parent folder"
    };
    Err(DeleteError::Permission(
        if uid == rustix::process::getuid().as_raw() {
            format!("{place} is read-only. Make it writable and try again.")
        } else {
            format!(
                "{place} belongs to {}, so your user cannot remove it. Remove it with sudo.",
                owner(uid)
            )
        },
    ))
}

#[cfg(not(unix))]
fn writable(_path: &Path, _mode: Mode) -> Result<(), DeleteError> {
    Ok(())
}

#[cfg(unix)]
fn owner(uid: u32) -> String {
    let name = std::fs::read_to_string("/etc/passwd")
        .ok()
        .and_then(|passwd| {
            passwd.lines().find_map(|line| {
                let mut fields = line.split(':');
                let name = fields.next()?;
                (fields.nth(1)?.parse::<u32>().ok()? == uid).then(|| name.to_owned())
            })
        });
    name.unwrap_or_else(|| {
        if uid == 0 {
            "root".into()
        } else {
            format!("user {uid}")
        }
    })
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

    #[cfg(unix)]
    #[test]
    fn a_folder_the_user_cannot_write_is_refused_before_anything_is_removed() {
        use std::os::unix::fs::PermissionsExt;

        if rustix::process::getuid().is_root() {
            return;
        }
        let base = tempfile::tempdir().unwrap();
        let locked = base.path().join("node_modules");
        std::fs::create_dir_all(locked.join("pkg")).unwrap();
        std::fs::write(locked.join("pkg/index.js"), "x").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
        let guard = Guard::new(&[base.path().to_path_buf()]).with_cwd(None);
        let target = Target {
            path: locked.clone(),
            identity: None,
        };

        let outcome = delete(&guard, &target, Mode::Permanent);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();

        assert!(matches!(outcome, Err(DeleteError::Permission(_))));
        assert!(locked.join("pkg/index.js").exists());
    }
}
