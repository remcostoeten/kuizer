#[cfg(target_os = "linux")]
mod linux;

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    pub point: PathBuf,
    pub fs_type: String,
}

pub struct MountTable {
    mounts: Vec<Mount>,
}

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
        Self { mounts }
    }

    pub fn fs_type(&self, path: &Path) -> Option<&str> {
        self.mounts
            .iter()
            .find(|mount| path.starts_with(&mount.point))
            .map(|mount| mount.fs_type.as_str())
    }
}

#[cfg(unix)]
pub fn device(metadata: &std::fs::Metadata) -> u64 {
    std::os::unix::fs::MetadataExt::dev(metadata)
}

#[cfg(not(unix))]
pub fn device(_metadata: &std::fs::Metadata) -> u64 {
    0
}

#[cfg(unix)]
pub fn allocated(metadata: &std::fs::Metadata) -> u64 {
    std::os::unix::fs::MetadataExt::blocks(metadata) * 512
}

#[cfg(not(unix))]
pub fn allocated(metadata: &std::fs::Metadata) -> u64 {
    metadata.len()
}

#[cfg(unix)]
pub fn identity(metadata: &std::fs::Metadata) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    Some((metadata.dev(), metadata.ino()))
}

#[cfg(not(unix))]
pub fn identity(_metadata: &std::fs::Metadata) -> Option<(u64, u64)> {
    None
}

#[cfg(unix)]
pub fn shared_inode(metadata: &std::fs::Metadata) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    (metadata.nlink() > 1 && !metadata.is_dir()).then(|| (metadata.dev(), metadata.ino()))
}

#[cfg(not(unix))]
pub fn shared_inode(_metadata: &std::fs::Metadata) -> Option<(u64, u64)> {
    None
}

#[cfg(unix)]
pub fn available_space(path: &Path) -> Option<u64> {
    let stats = rustix::fs::statvfs(path).ok()?;
    Some(stats.f_bavail.saturating_mul(stats.f_frsize))
}

#[cfg(not(unix))]
pub fn available_space(_path: &Path) -> Option<u64> {
    None
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

    #[cfg(unix)]
    #[test]
    fn reports_available_space_for_existing_paths() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(available_space(dir.path()).is_some_and(|bytes| bytes > 0));
        assert_eq!(available_space(&dir.path().join("missing")), None);
    }
}
