use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;

use super::Group;
use super::hash::same_bytes;
use crate::platform;
use crate::scan::Cancel;

static TEMP_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkMode {
    Reflink,
    Hardlink,
}

impl LinkMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reflink => "reflink",
            Self::Hardlink => "hardlink",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkStatus {
    Linked,
    DryRun,
    Skipped,
    Unsupported,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct LinkOutcome {
    pub source: PathBuf,
    pub target: PathBuf,
    pub bytes: u64,
    pub status: LinkStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct LinkReport {
    pub outcomes: Vec<LinkOutcome>,
    pub linked: usize,
    pub dry_run: usize,
    pub skipped: usize,
    pub unsupported: usize,
    pub failed: usize,
    pub saved_bytes: u64,
}

impl LinkReport {
    fn push(&mut self, outcome: LinkOutcome) {
        match outcome.status {
            LinkStatus::Linked => {
                self.linked += 1;
                self.saved_bytes = self.saved_bytes.saturating_add(outcome.bytes);
            }
            LinkStatus::DryRun => {
                self.dry_run += 1;
                self.saved_bytes = self.saved_bytes.saturating_add(outcome.bytes);
            }
            LinkStatus::Skipped => self.skipped += 1,
            LinkStatus::Unsupported => self.unsupported += 1,
            LinkStatus::Failed => self.failed += 1,
        }
        self.outcomes.push(outcome);
    }
}

/// @name link_groups
/// @description Replaces duplicate copies in `groups` with copy-on-write reflinks or hard links
/// pointing at each group's first member, without deleting any content. Every target is verified
/// byte-for-byte against its source immediately before an atomic sibling rename.
///
/// @example
/// let report = dupes::link_groups(&found.groups, LinkMode::Reflink, false, &Cancel::new());
pub fn link_groups(groups: &[Group], mode: LinkMode, dry_run: bool, cancel: &Cancel) -> LinkReport {
    let mut report = LinkReport::default();
    for group in groups {
        let Some((first, rest)) = group.members.split_first() else {
            continue;
        };
        for member in rest {
            if cancel.is_cancelled() {
                return report;
            }
            let outcome = link_member(
                &first.path,
                &member.path,
                &member.links,
                group.allocated,
                mode,
                dry_run,
                cancel,
            );
            report.push(outcome);
        }
    }
    report
}

fn link_member(
    source: &Path,
    target: &Path,
    extra_links: &[PathBuf],
    allocated: u64,
    mode: LinkMode,
    dry_run: bool,
    cancel: &Cancel,
) -> LinkOutcome {
    let outcome = |status: LinkStatus, bytes: u64, reason: Option<String>| LinkOutcome {
        source: source.to_path_buf(),
        target: target.to_path_buf(),
        bytes,
        status,
        reason,
    };

    let src_meta = match fs::symlink_metadata(source) {
        Ok(meta) if meta.is_file() => meta,
        Ok(_) => {
            return outcome(
                LinkStatus::Skipped,
                0,
                Some("source is not a regular file".into()),
            );
        }
        Err(err) => return outcome(LinkStatus::Failed, 0, Some(err.to_string())),
    };
    let dst_meta = match fs::symlink_metadata(target) {
        Ok(meta) if meta.is_file() => meta,
        Ok(_) => {
            return outcome(
                LinkStatus::Skipped,
                0,
                Some("target is not a regular file".into()),
            );
        }
        Err(err) => return outcome(LinkStatus::Failed, 0, Some(err.to_string())),
    };

    if platform::identity(&src_meta) == platform::identity(&dst_meta) {
        return outcome(LinkStatus::Skipped, 0, Some("already linked".into()));
    }

    if src_meta.len() != dst_meta.len() {
        return outcome(
            LinkStatus::Skipped,
            0,
            Some("file changed since scan".into()),
        );
    }

    if platform::device(&src_meta) != platform::device(&dst_meta) {
        return outcome(LinkStatus::Skipped, 0, Some("different filesystem".into()));
    }

    match same_bytes(source, target, cancel) {
        Ok(true) => {}
        Ok(false) => {
            return outcome(
                LinkStatus::Skipped,
                0,
                Some("file changed since scan".into()),
            );
        }
        Err(err) if err.kind() == io::ErrorKind::Interrupted => {
            return outcome(LinkStatus::Skipped, 0, Some("cancelled".into()));
        }
        Err(err) => return outcome(LinkStatus::Failed, 0, Some(err.to_string())),
    }

    let bytes = allocated.min(platform::allocated(&dst_meta));
    if dry_run {
        return outcome(LinkStatus::DryRun, bytes, None);
    }

    match replace_file(source, target, &dst_meta, mode) {
        Ok(()) => {}
        Err(ReplaceError::Unsupported(msg)) => {
            return outcome(LinkStatus::Unsupported, 0, Some(msg));
        }
        Err(ReplaceError::CrossDevice) => {
            return outcome(LinkStatus::Skipped, 0, Some("different filesystem".into()));
        }
        Err(ReplaceError::Io(err)) => {
            return outcome(LinkStatus::Failed, 0, Some(err.to_string()));
        }
    }

    // If the target had sibling hard links to its old inode, relink them so the old inode's
    // extents are actually freed and their hardlink relationship is preserved.
    let link_base = match mode {
        LinkMode::Reflink => target,
        LinkMode::Hardlink => source,
    };
    for extra in extra_links {
        if let Ok(extra_meta) = fs::symlink_metadata(extra)
            && extra_meta.is_file()
            && platform::identity(&extra_meta) == platform::identity(&dst_meta)
        {
            let _ = replace_file(link_base, extra, &extra_meta, LinkMode::Hardlink);
        }
    }

    outcome(LinkStatus::Linked, bytes, None)
}

enum ReplaceError {
    Unsupported(String),
    CrossDevice,
    Io(io::Error),
}

fn temp_sibling(target: &Path) -> io::Result<PathBuf> {
    let parent = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
    Ok(parent.join(format!(".kuizer-link.{}.{id}.tmp", std::process::id())))
}

fn replace_file(
    source: &Path,
    target: &Path,
    target_meta: &fs::Metadata,
    mode: LinkMode,
) -> Result<(), ReplaceError> {
    let temp = temp_sibling(target).map_err(ReplaceError::Io)?;
    let result = match mode {
        LinkMode::Reflink => create_reflink(source, &temp, target_meta),
        LinkMode::Hardlink => create_hardlink(source, &temp),
    };
    if let Err(err) = result {
        let _ = fs::remove_file(&temp);
        return Err(err);
    }
    if let Err(err) = fs::rename(&temp, target) {
        let _ = fs::remove_file(&temp);
        return Err(ReplaceError::Io(err));
    }
    Ok(())
}

fn create_hardlink(source: &Path, temp: &Path) -> Result<(), ReplaceError> {
    match fs::hard_link(source, temp) {
        Ok(()) => Ok(()),
        Err(err) if err.raw_os_error() == Some(libc::EXDEV) => Err(ReplaceError::CrossDevice),
        Err(err) => Err(ReplaceError::Io(err)),
    }
}

#[cfg(target_os = "linux")]
fn create_reflink(
    source: &Path,
    temp: &Path,
    target_meta: &fs::Metadata,
) -> Result<(), ReplaceError> {
    use rustix::io::Errno;

    let src = File::open(source).map_err(ReplaceError::Io)?;
    let dst = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp)
        .map_err(ReplaceError::Io)?;
    dst.set_permissions(target_meta.permissions())
        .map_err(ReplaceError::Io)?;

    if let Err(errno) = rustix::fs::ioctl_ficlone(&dst, &src) {
        return Err(match errno {
            Errno::XDEV => ReplaceError::CrossDevice,
            Errno::OPNOTSUPP | Errno::INVAL | Errno::NOSYS | Errno::NOTTY => {
                ReplaceError::Unsupported("filesystem does not support reflinks".into())
            }
            other => ReplaceError::Io(io::Error::from(other)),
        });
    }

    if let Ok(modified) = target_meta.modified() {
        let _ = dst.set_modified(modified);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn create_reflink(
    source: &Path,
    temp: &Path,
    target_meta: &fs::Metadata,
) -> Result<(), ReplaceError> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    unsafe extern "C" {
        fn clonefile(src: *const libc::c_char, dst: *const libc::c_char, flags: u32)
        -> libc::c_int;
    }

    let src_c = CString::new(source.as_os_str().as_bytes()).map_err(|_| {
        ReplaceError::Io(io::Error::new(io::ErrorKind::InvalidInput, "interior NUL"))
    })?;
    let dst_c = CString::new(temp.as_os_str().as_bytes()).map_err(|_| {
        ReplaceError::Io(io::Error::new(io::ErrorKind::InvalidInput, "interior NUL"))
    })?;

    let rc = unsafe { clonefile(src_c.as_ptr(), dst_c.as_ptr(), 0) };
    if rc != 0 {
        let err = io::Error::last_os_error();
        return Err(match err.raw_os_error() {
            Some(libc::EXDEV) => ReplaceError::CrossDevice,
            Some(libc::ENOTSUP | libc::EINVAL | libc::ENOSYS) => {
                ReplaceError::Unsupported("filesystem does not support reflinks".into())
            }
            _ => ReplaceError::Io(err),
        });
    }
    let _ = fs::set_permissions(temp, target_meta.permissions());
    if let Ok(modified) = target_meta.modified()
        && let Ok(file) = File::open(temp)
    {
        let _ = file.set_modified(modified);
    }
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn create_reflink(
    _source: &Path,
    _temp: &Path,
    _target_meta: &fs::Metadata,
) -> Result<(), ReplaceError> {
    Err(ReplaceError::Unsupported(
        "reflinks are not supported on this platform".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dupes::Member;
    use tempfile::TempDir;

    fn make_group(paths: &[PathBuf], size: u64) -> Group {
        Group {
            size,
            allocated: size,
            hash: "0".repeat(64),
            verified: true,
            members: paths
                .iter()
                .map(|path| Member {
                    path: path.clone(),
                    links: Vec::new(),
                    modified: None,
                })
                .collect(),
        }
    }

    #[test]
    fn hardlink_deduplicates_copies_into_one_inode_and_preserves_contents() {
        let dir = TempDir::new().unwrap();
        let data = vec![b'k'; 8192];
        let a = dir.path().join("a.bin");
        let b = dir.path().join("b.bin");
        let c = dir.path().join("c.bin");
        fs::write(&a, &data).unwrap();
        fs::write(&b, &data).unwrap();
        fs::write(&c, &data).unwrap();

        let id_a = platform::identity(&fs::metadata(&a).unwrap());
        let id_b_before = platform::identity(&fs::metadata(&b).unwrap());
        assert_ne!(id_a, id_b_before);

        let group = make_group(&[a.clone(), b.clone(), c.clone()], data.len() as u64);
        let report = link_groups(&[group], LinkMode::Hardlink, false, &Cancel::new());

        assert_eq!(report.linked, 2);
        assert_eq!(report.failed, 0);
        assert_eq!(report.skipped, 0);
        assert_eq!(platform::identity(&fs::metadata(&b).unwrap()), id_a);
        assert_eq!(platform::identity(&fs::metadata(&c).unwrap()), id_a);
        assert_eq!(fs::read(&a).unwrap(), data);
        assert_eq!(fs::read(&b).unwrap(), data);
        assert_eq!(fs::read(&c).unwrap(), data);
    }

    #[test]
    fn refuses_to_link_when_target_changed_since_scan() {
        let dir = TempDir::new().unwrap();
        let a = dir.path().join("a.bin");
        let b = dir.path().join("b.bin");
        fs::write(&a, b"aaaa-identical-payload").unwrap();
        fs::write(&b, b"bbbb-different-payload").unwrap();

        let id_b_before = platform::identity(&fs::metadata(&b).unwrap());
        let group = make_group(&[a.clone(), b.clone()], 22);
        let report = link_groups(&[group], LinkMode::Hardlink, false, &Cancel::new());

        assert_eq!(report.linked, 0);
        assert_eq!(report.skipped, 1);
        assert_eq!(
            report.outcomes[0].reason.as_deref(),
            Some("file changed since scan")
        );
        assert_eq!(platform::identity(&fs::metadata(&b).unwrap()), id_b_before);
        assert_eq!(fs::read(&b).unwrap(), b"bbbb-different-payload");
    }

    #[test]
    fn dry_run_reports_planned_links_without_modifying_files() {
        let dir = TempDir::new().unwrap();
        let data = vec![b'z'; 4096];
        let a = dir.path().join("a.bin");
        let b = dir.path().join("b.bin");
        fs::write(&a, &data).unwrap();
        fs::write(&b, &data).unwrap();

        let id_a = platform::identity(&fs::metadata(&a).unwrap());
        let id_b = platform::identity(&fs::metadata(&b).unwrap());
        assert_ne!(id_a, id_b);

        let group = make_group(&[a.clone(), b.clone()], data.len() as u64);
        let report = link_groups(&[group], LinkMode::Hardlink, true, &Cancel::new());

        assert_eq!(report.dry_run, 1);
        assert_eq!(report.linked, 0);
        assert_eq!(platform::identity(&fs::metadata(&b).unwrap()), id_b);
    }

    #[test]
    fn reflink_either_links_or_reports_unsupported_without_losing_data() {
        let dir = TempDir::new().unwrap();
        let data = vec![b'r'; 8192];
        let a = dir.path().join("a.bin");
        let b = dir.path().join("b.bin");
        fs::write(&a, &data).unwrap();
        fs::write(&b, &data).unwrap();

        let group = make_group(&[a.clone(), b.clone()], data.len() as u64);
        let report = link_groups(&[group], LinkMode::Reflink, false, &Cancel::new());

        assert_eq!(report.failed, 0);
        assert_eq!(report.linked + report.unsupported, 1);
        assert_eq!(fs::read(&a).unwrap(), data);
        assert_eq!(fs::read(&b).unwrap(), data);
    }
}
