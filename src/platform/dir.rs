use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::time::SystemTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Dir,
    File,
    Symlink,
    Other,
    Unknown,
}

impl EntryKind {
    pub fn is_dir(self) -> bool {
        self == Self::Dir
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Stat {
    pub kind: EntryKind,
    pub device: u64,
    pub inode: u64,
    pub links: u64,
    pub len: u64,
    pub allocated: u64,
    pub modified: Option<SystemTime>,
}

impl Stat {
    pub fn identity(&self) -> (u64, u64) {
        (self.device, self.inode)
    }

    pub fn shared_inode(&self) -> Option<(u64, u64)> {
        (self.links > 1 && !self.kind.is_dir()).then_some((self.device, self.inode))
    }

    pub fn from_metadata(metadata: &std::fs::Metadata) -> Self {
        use std::os::unix::fs::MetadataExt;
        let file_type = metadata.file_type();
        let kind = if file_type.is_dir() {
            EntryKind::Dir
        } else if file_type.is_file() {
            EntryKind::File
        } else if file_type.is_symlink() {
            EntryKind::Symlink
        } else {
            EntryKind::Other
        };
        Self {
            kind,
            device: metadata.dev(),
            inode: metadata.ino(),
            links: metadata.nlink(),
            len: metadata.len(),
            allocated: metadata.blocks() * 512,
            modified: metadata.modified().ok(),
        }
    }
}

/// An open directory that lists its entries and stats them relative to itself, so a walk
/// pays no path lookup per entry. On Linux it reads with `getdents64` into a caller-owned
/// buffer; elsewhere it reads through `readdir`.
pub struct Dir {
    fd: rustix::fd::OwnedFd,
}

/// Reusable scratch space for [`Dir::read`], one per thread.
pub struct Buffer {
    #[cfg(target_os = "linux")]
    bytes: Vec<u8>,
}

#[cfg(target_os = "linux")]
const BUFFER_BYTES: usize = 128 * 1024;

impl Default for Buffer {
    fn default() -> Self {
        Self {
            #[cfg(target_os = "linux")]
            bytes: Vec::with_capacity(BUFFER_BYTES),
        }
    }
}

mod imp {
    use std::ffi::OsStr;
    use std::io;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use rustix::fs::{AtFlags, CWD, FileType, Mode, OFlags};

    use super::{Buffer, Dir, EntryKind, Stat};

    fn kind(file_type: FileType) -> EntryKind {
        match file_type {
            FileType::Directory => EntryKind::Dir,
            FileType::RegularFile => EntryKind::File,
            FileType::Symlink => EntryKind::Symlink,
            FileType::Unknown => EntryKind::Unknown,
            _ => EntryKind::Other,
        }
    }

    fn time(seconds: i64, nanos: u32) -> Option<SystemTime> {
        let base = if seconds >= 0 {
            UNIX_EPOCH.checked_add(Duration::from_secs(seconds.unsigned_abs()))
        } else {
            UNIX_EPOCH.checked_sub(Duration::from_secs(seconds.unsigned_abs()))
        };
        base?.checked_add(Duration::from_nanos(u64::from(nanos)))
    }

    #[allow(clippy::unnecessary_cast, clippy::cast_sign_loss)]
    fn convert(stat: &rustix::fs::Stat) -> Stat {
        Stat {
            kind: kind(FileType::from_raw_mode(stat.st_mode as _)),
            device: stat.st_dev as u64,
            inode: stat.st_ino as u64,
            links: stat.st_nlink as u64,
            len: stat.st_size as u64,
            allocated: (stat.st_blocks as u64).saturating_mul(512),
            modified: time(stat.st_mtime as i64, stat.st_mtime_nsec as u32),
        }
    }

    impl Dir {
        pub fn open(path: &Path, follow: bool) -> io::Result<Self> {
            let mut flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC;
            if !follow {
                flags |= OFlags::NOFOLLOW;
            }
            Ok(Self {
                fd: rustix::fs::openat(CWD, path, flags, Mode::empty())?,
            })
        }

        pub fn stat(&self) -> io::Result<Stat> {
            Ok(convert(&rustix::fs::fstat(&self.fd)?))
        }

        pub fn stat_at(&self, name: &OsStr) -> io::Result<Stat> {
            Ok(convert(&rustix::fs::statat(
                &self.fd,
                name,
                AtFlags::SYMLINK_NOFOLLOW,
            )?))
        }

        #[cfg(target_os = "linux")]
        pub fn read(
            &self,
            buffer: &mut Buffer,
            mut each: impl FnMut(&OsStr, EntryKind, u64),
        ) -> io::Result<()> {
            buffer.bytes.clear();
            let mut entries = rustix::fs::RawDir::new(&self.fd, buffer.bytes.spare_capacity_mut());
            while let Some(entry) = entries.next() {
                let entry = entry?;
                let name = entry.file_name().to_bytes();
                if name == b"." || name == b".." {
                    continue;
                }
                each(
                    OsStr::from_bytes(name),
                    kind(entry.file_type()),
                    entry.ino(),
                );
            }
            Ok(())
        }

        #[cfg(not(target_os = "linux"))]
        pub fn read(
            &self,
            _buffer: &mut Buffer,
            mut each: impl FnMut(&OsStr, EntryKind, u64),
        ) -> io::Result<()> {
            for entry in rustix::fs::Dir::read_from(&self.fd)? {
                let entry = entry?;
                let name = entry.file_name().to_bytes();
                if name == b"." || name == b".." {
                    continue;
                }
                each(
                    OsStr::from_bytes(name),
                    kind(entry.file_type()),
                    entry.ino(),
                );
            }
            Ok(())
        }
    }
}

impl Dir {
    /// @name Dir::kind_at
    /// @description Resolves the type of an entry whose directory listing did not say, as some
    /// filesystems report `DT_UNKNOWN`.
    ///
    /// @example
    /// let kind = dir.kind_at(name).unwrap_or(EntryKind::Other);
    pub fn kind_at(&self, name: &OsStr) -> io::Result<EntryKind> {
        self.stat_at(name).map(|stat| stat.kind)
    }
}

/// @name stat_path
/// @description Stats `path` without following a final symlink, in the same shape as
/// [`Dir::stat_at`].
///
/// @example
/// let stat = platform::dir::stat_path(&candidate)?;
pub fn stat_path(path: &Path) -> io::Result<Stat> {
    std::fs::symlink_metadata(path).map(|metadata| Stat::from_metadata(&metadata))
}
