use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, OnceLock};

use crate::platform::{self, MountTable};
use crate::protect::Protected;

const PROTECTED: &[&str] = &[
    "/bin",
    "/boot",
    "/dev",
    "/efi",
    "/etc",
    "/lib",
    "/lib32",
    "/lib64",
    "/nix",
    "/opt",
    "/proc",
    "/root",
    "/run",
    "/sbin",
    "/snap",
    "/srv",
    "/sys",
    "/usr",
    "/var/lib",
    "/var/log",
    "/Applications",
    "/Library",
    "/System",
    "/private/etc",
    "/private/var/db",
];

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    #[error("path is not absolute or contains `.` or `..`")]
    NotNormal,
    #[error("path is too close to the filesystem root")]
    TooShallow,
    #[error("path is outside every scanned root")]
    OutsideRoots,
    #[error("path is a scan root, the home directory or one of their parents")]
    Anchor,
    #[error("path contains the current working directory")]
    ContainsCwd,
    #[error("path is inside a protected system directory")]
    Protected,
    #[error("path holds browser data, credentials or installed tools and extensions")]
    AppData,
    #[error("path is a symlink now")]
    Symlink,
    #[error("path changed since the scan (different device or inode)")]
    Changed,
    #[error("path is a mount point")]
    MountPoint,
    #[error("path no longer exists")]
    Missing,
    #[error("cannot inspect path: {0}")]
    Inspect(String),
}

#[derive(Clone, Default)]
struct Mounts(Arc<OnceLock<MountTable>>);

impl Mounts {
    fn is_mount_point(&self, path: &Path) -> bool {
        self.0.get_or_init(MountTable::load).is_mount_point(path)
    }
}

impl fmt::Debug for Mounts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Mounts")
    }
}

#[derive(Debug, Clone)]
pub struct Guard {
    roots: Vec<PathBuf>,
    anchors: Vec<PathBuf>,
    cwd: Option<PathBuf>,
    protected: Protected,
    mounts: Mounts,
}

impl Guard {
    pub fn new(roots: &[PathBuf]) -> Self {
        let mut anchors: Vec<PathBuf> = roots.to_vec();
        anchors.extend(dirs::home_dir());
        anchors.extend(platform::home_dir());
        anchors.push(PathBuf::from("/"));
        Self {
            roots: roots.to_vec(),
            anchors,
            cwd: std::env::current_dir().ok(),
            protected: Protected::for_user(),
            mounts: Mounts::default(),
        }
    }

    pub fn with_cwd(mut self, cwd: Option<PathBuf>) -> Self {
        self.cwd = cwd;
        self
    }

    fn check_path(&self, path: &Path) -> Result<(), Refusal> {
        let normal = path.is_absolute()
            && path
                .components()
                .all(|c| !matches!(c, Component::CurDir | Component::ParentDir));
        if !normal {
            return Err(Refusal::NotNormal);
        }
        if path.components().count() < 3 {
            return Err(Refusal::TooShallow);
        }
        if PROTECTED.iter().any(|prefix| path.starts_with(prefix)) {
            return Err(Refusal::Protected);
        }
        if self.anchors.iter().any(|anchor| anchor.starts_with(path)) {
            return Err(Refusal::Anchor);
        }
        if !self.roots.iter().any(|root| path.starts_with(root)) {
            return Err(Refusal::OutsideRoots);
        }
        if self.cwd.as_ref().is_some_and(|cwd| cwd.starts_with(path)) {
            return Err(Refusal::ContainsCwd);
        }
        Ok(())
    }

    pub fn with_protected(mut self, protected: Protected) -> Self {
        self.protected = protected;
        self
    }

    pub fn check(&self, path: &Path, identity: Option<(u64, u64)>) -> Result<(), Refusal> {
        self.check_path(path)?;
        if self.protected.covers(path) {
            return Err(Refusal::AppData);
        }
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(Refusal::Missing);
            }
            Err(error) => return Err(Refusal::Inspect(error.to_string())),
        };
        if metadata.file_type().is_symlink() {
            return Err(Refusal::Symlink);
        }
        if identity.is_some() && platform::identity(&metadata) != identity {
            return Err(Refusal::Changed);
        }
        if self.mounts.is_mount_point(path) {
            return Err(Refusal::MountPoint);
        }
        if metadata.is_dir() {
            let parent = path.parent().ok_or(Refusal::TooShallow)?;
            let parent_meta =
                fs::symlink_metadata(parent).map_err(|e| Refusal::Inspect(e.to_string()))?;
            if platform::device(&parent_meta) != platform::device(&metadata) {
                return Err(Refusal::MountPoint);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_a_bind_mount_on_the_same_device() {
        let tmp = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(tmp.path()).unwrap();
        let mounted = base.join("app/node_modules");
        let plain = base.join("app/dist");
        fs::create_dir_all(&mounted).unwrap();
        fs::create_dir_all(&plain).unwrap();
        let guard = Guard::new(std::slice::from_ref(&base)).with_cwd(None);
        let table = MountTable::from_mounts(vec![platform::Mount {
            point: mounted.clone(),
            fs_type: "ext4".to_owned(),
        }]);
        guard.mounts.0.set(table).ok().unwrap();

        assert_eq!(guard.check(&mounted, None), Err(Refusal::MountPoint));
        assert_eq!(guard.check(&plain, None), Ok(()));
    }

    fn guard() -> Guard {
        Guard::new(&[PathBuf::from("/home/me/projects")]).with_cwd(Some(PathBuf::from("/tmp/x")))
    }

    #[test]
    fn refuses_unsafe_shapes() {
        let guard = guard();
        assert_eq!(guard.check_path(Path::new("/")), Err(Refusal::TooShallow));
        assert_eq!(
            guard.check_path(Path::new("relative/x")),
            Err(Refusal::NotNormal)
        );
        assert_eq!(
            guard.check_path(Path::new("/home/me/projects/a/../b")),
            Err(Refusal::NotNormal)
        );
        assert_eq!(
            guard.check_path(Path::new("/usr/lib/node_modules")),
            Err(Refusal::Protected)
        );
        assert_eq!(
            guard.check_path(Path::new("/home/me/projects")),
            Err(Refusal::Anchor)
        );
        assert_eq!(
            guard.check_path(Path::new("/home/me/other/x")),
            Err(Refusal::OutsideRoots)
        );
        assert!(
            guard
                .check_path(Path::new("/home/me/projects/web/node_modules"))
                .is_ok()
        );
    }

    #[test]
    fn refuses_system_folders_of_other_platforms() {
        let guard = Guard::new(&[PathBuf::from("/")]).with_cwd(None);
        for path in [
            "/nix/store/abc-pkg/lib/node_modules",
            "/snap/code/current/node_modules",
            "/efi/EFI/cache",
            "/System/Library/Caches",
            "/Library/Caches/com.example",
            "/Applications/Editor.app/Contents/node_modules",
            "/private/var/db/cache",
        ] {
            assert_eq!(
                guard.check_path(Path::new(path)),
                Err(Refusal::Protected),
                "{path}"
            );
        }
    }

    #[test]
    fn refuses_parent_of_cwd() {
        let guard = Guard::new(&[PathBuf::from("/home/me/projects")]).with_cwd(Some(
            PathBuf::from("/home/me/projects/web/node_modules/pkg"),
        ));
        assert_eq!(
            guard.check_path(Path::new("/home/me/projects/web/node_modules")),
            Err(Refusal::ContainsCwd)
        );
    }
}
