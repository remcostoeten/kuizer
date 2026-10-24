pub mod dir;
#[cfg(target_os = "linux")]
mod linux;

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    pub point: PathBuf,
    pub fs_type: String,
}

pub struct MountTable {
    mounts: Vec<Mount>,
    points: HashSet<PathBuf>,
    names: HashSet<OsString>,
}

const SPLIT_DEVICE_FS_TYPES: &[&str] = &["btrfs", "bcachefs"];

impl MountTable {
    pub fn load() -> Self {
        #[cfg(target_os = "linux")]
        let mounts = linux::mounts();
        #[cfg(not(target_os = "linux"))]
        let mounts = Vec::new();
        Self::from_mounts(mounts)
    }

    pub fn from_mounts(mut mounts: Vec<Mount>) -> Self {
        mounts.sort_by_key(|mount| std::cmp::Reverse(mount.point.components().count()));
        let points: HashSet<PathBuf> = mounts.iter().map(|mount| mount.point.clone()).collect();
        let names = points
            .iter()
            .filter_map(|point| point.file_name().map(OsString::from))
            .collect();
        Self {
            mounts,
            points,
            names,
        }
    }

    /// @name MountTable::is_mount_point
    /// @description Returns whether something is mounted exactly at `path`, bind mounts
    /// included.
    ///
    /// @example
    /// if mounts.is_mount_point(&child) { check_device(&child); }
    pub fn is_mount_point(&self, path: &Path) -> bool {
        path.file_name()
            .is_some_and(|name| self.names.contains(name))
            && self.points.contains(path)
    }

    /// @name MountTable::device_may_change_below
    /// @description Returns whether a directory under `path` can sit on another device without
    /// being a mount point, as btrfs and bcachefs subvolumes do, or whether the table is too
    /// empty to tell. Walkers then compare devices on every directory instead of only at mount
    /// points.
    ///
    /// @example
    /// let check_every_dir = mounts.device_may_change_below(&root);
    pub fn device_may_change_below(&self, path: &Path) -> bool {
        self.mounts.is_empty()
            || self
                .fs_type(path)
                .is_some_and(|fs_type| SPLIT_DEVICE_FS_TYPES.contains(&fs_type))
    }

    pub fn fs_type(&self, path: &Path) -> Option<&str> {
        self.mounts
            .iter()
            .find(|mount| path.starts_with(&mount.point))
            .map(|mount| mount.fs_type.as_str())
    }
}

/// @name home_dir
/// @description Returns the home directory with symlinks resolved, so it compares equal to the
/// canonicalized scan roots. Falls back to the raw path when it cannot be resolved.
///
/// @example
/// let protected = Protected::new(platform::home_dir().as_deref());
pub fn home_dir() -> Option<PathBuf> {
    static HOME: OnceLock<Option<PathBuf>> = OnceLock::new();
    HOME.get_or_init(|| dirs::home_dir().map(|home| std::fs::canonicalize(&home).unwrap_or(home)))
        .clone()
}

/// @name rotational
/// @description Returns whether `device` is a spinning disk, read from sysfs on Linux, so a
/// scan can stay sequential where parallel reads would only add seeks. Anonymous devices, such
/// as btrfs subvolumes, and other platforms count as not rotational; [`rotational_at`] resolves
/// the former through the mount source.
///
/// @example
/// let threads = if platform::rotational(device) { 2 } else { 16 };
#[cfg(target_os = "linux")]
pub fn rotational(device: u64) -> bool {
    linux::rotational(device)
}

#[cfg(not(target_os = "linux"))]
pub fn rotational(_device: u64) -> bool {
    false
}

/// @name rotational_at
/// @description Returns whether the filesystem holding `path` sits on a spinning disk. Where the
/// file reports an anonymous device, as on btrfs and bcachefs, the disk behind the mount is
/// looked up through the mount source instead. Unknown setups, ZFS datasets included, count as
/// not rotational.
///
/// @example
/// let threads = if platform::rotational_at(&root) { 2 } else { 16 };
#[cfg(target_os = "linux")]
pub fn rotational_at(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    let dev = device(&metadata);
    if rustix::fs::major(dev) != 0 {
        return linux::rotational(dev);
    }
    linux::rotational_mount(path)
}

#[cfg(not(target_os = "linux"))]
pub fn rotational_at(_path: &Path) -> bool {
    false
}

pub fn device(metadata: &std::fs::Metadata) -> u64 {
    std::os::unix::fs::MetadataExt::dev(metadata)
}

pub fn allocated(metadata: &std::fs::Metadata) -> u64 {
    std::os::unix::fs::MetadataExt::blocks(metadata) * 512
}

pub fn identity(metadata: &std::fs::Metadata) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    Some((metadata.dev(), metadata.ino()))
}

pub fn shared_inode(metadata: &std::fs::Metadata) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    (metadata.nlink() > 1 && !metadata.is_dir()).then(|| (metadata.dev(), metadata.ino()))
}

pub fn available_space(path: &Path) -> Option<u64> {
    let stats = rustix::fs::statvfs(path).ok()?;
    Some(stats.f_bavail.saturating_mul(stats.f_frsize))
}

pub fn total_space(path: &Path) -> Option<u64> {
    let stats = rustix::fs::statvfs(path).ok()?;
    Some(stats.f_blocks.saturating_mul(stats.f_frsize))
}

/// @name open_in_browser
/// @description Opens a file with the desktop's default handler without waiting for it to exit.
/// Returns an error when the opener program is missing or cannot be started.
///
/// @example
/// platform::open_in_browser(Path::new("/tmp/report.html"))?;
pub fn open_in_browser(path: &Path) -> std::io::Result<()> {
    use std::process::{Command, Stdio};

    #[cfg(target_os = "macos")]
    let mut command = Command::new("open");
    #[cfg(not(target_os = "macos"))]
    let mut command = Command::new("xdg-open");

    command
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn longest_mount_wins() {
        let table = MountTable::from_mounts(vec![
            Mount {
                point: PathBuf::from("/"),
                fs_type: "ext4".to_owned(),
            },
            Mount {
                point: PathBuf::from("/proc"),
                fs_type: "proc".to_owned(),
            },
        ]);
        assert_eq!(table.fs_type(Path::new("/proc/1/fd")), Some("proc"));
        assert_eq!(table.fs_type(Path::new("/processes")), Some("ext4"));
    }

    #[test]
    fn reports_available_space_for_existing_paths() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(available_space(dir.path()).is_some_and(|bytes| bytes > 0));
        assert_eq!(available_space(&dir.path().join("missing")), None);
    }
}
