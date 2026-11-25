use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::delete::{self, DeleteError, Guard, Mode, Target};
use crate::detect::{Category, Kind, RuleInfo, Safety};
use crate::platform;
use crate::scan::{self, Cancel, Candidate, Issue, IssueKind};

const FULL_DISK_ACCESS_MESSAGE: &str =
    "cannot read the trash; give this terminal or kuizer Full Disk Access in System Settings";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Folder {
    pub path: PathBuf,
    pub items: usize,
    pub layout: Layout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Layout {
    Freedesktop,
    Finder,
}

/// @name home_trash
/// @description Returns the current user's home trash path and layout for this platform.
///
/// @example
/// if let Some((path, layout)) = trashcan::home_trash() {
///     let folder = trashcan::inspect(&path, layout)?;
/// }
pub fn home_trash() -> Option<(PathBuf, Layout)> {
    #[cfg(target_os = "macos")]
    {
        Some((dirs::home_dir()?.join(".Trash"), Layout::Finder))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Some((dirs::data_dir()?.join("Trash"), Layout::Freedesktop))
    }
}

fn issue_for(path: &Path, layout: Layout, error: &io::Error) -> Issue {
    let mut issue = Issue::from_io(path.to_path_buf(), error);
    if layout == Layout::Finder && issue.kind == IssueKind::PermissionDenied {
        issue.message = FULL_DISK_ACCESS_MESSAGE.to_owned();
    }
    issue
}

/// @name inspect
/// @description Inspects a trash directory without following symlinks and returns `Ok(Some(folder))`
/// when it contains at least one item.
///
/// @example
/// let folder = trashcan::inspect(&path, Layout::Freedesktop)?;
pub fn inspect(path: &Path, layout: Layout) -> Result<Option<Folder>, Issue> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(issue_for(path, layout, &error)),
    };
    if !metadata.file_type().is_dir() {
        return Ok(None);
    }
    let content_dir = match layout {
        Layout::Freedesktop => path.join("files"),
        Layout::Finder => path.to_path_buf(),
    };
    let entries = match fs::read_dir(&content_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(issue_for(&content_dir, layout, &error)),
    };
    let mut items = 0usize;
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        if layout == Layout::Finder && entry.file_name() == ".DS_Store" {
            continue;
        }
        items += 1;
    }
    if items == 0 {
        return Ok(None);
    }
    Ok(Some(Folder {
        path: path.to_path_buf(),
        items,
        layout,
    }))
}

/// @name entries
/// @description Measures each trash folder and builds the matching candidate rows.
///
/// @example
/// let rows = trashcan::entries(folders, &Cancel::new());
pub fn entries(folders: Vec<Folder>, cancel: &Cancel) -> Vec<(Folder, Candidate)> {
    let rule = Arc::new(RuleInfo {
        name: "trash".to_owned(),
        category: Category::Trash,
        safety: Safety::Safe,
        ecosystem: "trash".to_owned(),
    });
    folders
        .into_iter()
        .enumerate()
        .map(|(index, folder)| {
            let measured = scan::measure(&folder.path, cancel);
            let candidate = Candidate {
                id: (index + 1) as u64,
                path: folder.path.clone(),
                kind: Kind::Dir,
                rule: Arc::clone(&rule),
                project: None,
                usage: measured.usage,
                newest: measured.newest,
                identity: measured.identity,
                project_last_active: None,
            };
            (folder, candidate)
        })
        .collect()
}

/// @name roots
/// @description Returns the guard root directories for a trash folder so only children inside
/// them can ever be removed.
///
/// @example
/// let guard = Guard::new(&trashcan::roots(&folder));
pub fn roots(folder: &Folder) -> Vec<PathBuf> {
    match folder.layout {
        Layout::Freedesktop => vec![folder.path.join("files"), folder.path.join("info")],
        Layout::Finder => vec![folder.path.clone()],
    }
}

/// @name remove
/// @description Permanently empties the trash folder represented by `candidate` through `guard`,
/// leaving the folder structure, `directorysizes`, and any symlinks untouched.
///
/// @example
/// trashcan::remove(&guard, &candidate, Mode::Permanent)?;
pub fn remove(guard: &Guard, candidate: &Candidate, mode: Mode) -> Result<(), DeleteError> {
    if mode == Mode::DryRun {
        return Ok(());
    }
    let metadata = fs::symlink_metadata(&candidate.path).map_err(|_| {
        DeleteError::Tool("the trash folder changed since it was listed".to_owned())
    })?;
    if !metadata.file_type().is_dir() || platform::identity(&metadata) != candidate.identity {
        return Err(DeleteError::Tool(
            "the trash folder changed since it was listed".to_owned(),
        ));
    }
    let layout = if candidate.path.join("files").is_dir() {
        Layout::Freedesktop
    } else {
        Layout::Finder
    };
    let target_roots = match layout {
        Layout::Freedesktop => vec![candidate.path.join("files"), candidate.path.join("info")],
        Layout::Finder => vec![candidate.path.clone()],
    };
    let mut failed = 0usize;
    let mut skipped = 0usize;
    let mut first: Option<String> = None;

    for root in target_roots {
        let entries = match fs::read_dir(&root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                failed += 1;
                if first.is_none() {
                    first = Some(error.to_string());
                }
                continue;
            }
        };
        let mut children: Vec<PathBuf> = Vec::new();
        for entry in entries {
            let Ok(entry) = entry else {
                continue;
            };
            if layout == Layout::Finder && entry.file_name() == ".DS_Store" {
                continue;
            }
            children.push(entry.path());
        }
        children.sort();
        for child in children {
            let identity = match fs::symlink_metadata(&child) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    skipped += 1;
                    continue;
                }
                Ok(meta) => platform::identity(&meta),
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => {
                    failed += 1;
                    if first.is_none() {
                        first = Some(error.to_string());
                    }
                    continue;
                }
            };
            if let Err(error) =
                delete::delete(guard, &Target::new(child, identity), Mode::Permanent)
            {
                failed += 1;
                if first.is_none() {
                    first = Some(error.to_string());
                }
            }
        }
    }

    if failed > 0 || skipped > 0 {
        let mut parts = Vec::new();
        if failed > 0 {
            let noun = if failed == 1 { "item" } else { "items" };
            parts.push(format!("{failed} {noun} could not be removed"));
        }
        if skipped > 0 {
            let phrase = if skipped == 1 {
                "1 link was left in the trash".to_owned()
            } else {
                format!("{skipped} links were left in the trash")
            };
            parts.push(phrase);
        }
        let mut message = parts.join(", ");
        if let Some(reason) = first {
            message.push_str(": ");
            message.push_str(&reason);
        }
        return Err(DeleteError::Tool(message));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_freedesktop_trash(root: &Path) -> PathBuf {
        let trash = root.join("Trash");
        fs::create_dir_all(trash.join("files/dir")).expect("files/dir");
        fs::create_dir_all(trash.join("info")).expect("info");
        fs::write(trash.join("files/a.txt"), b"hello").expect("a.txt");
        fs::write(trash.join("files/dir/b"), b"world").expect("dir/b");
        fs::write(trash.join("info/a.txt.trashinfo"), b"[Trash Info]\n").expect("a.trashinfo");
        fs::write(trash.join("info/dir.trashinfo"), b"[Trash Info]\n").expect("dir.trashinfo");
        fs::write(trash.join("directorysizes"), b"0 0 dir\n").expect("directorysizes");
        trash
    }

    #[test]
    fn inspects_a_freedesktop_trash() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let trash = make_freedesktop_trash(tmp.path());
        let folder = inspect(&trash, Layout::Freedesktop)
            .expect("inspect")
            .expect("non-empty");
        assert_eq!(folder.items, 2);
        assert_eq!(folder.layout, Layout::Freedesktop);

        let empty = tmp.path().join("EmptyTrash");
        fs::create_dir_all(empty.join("files")).expect("empty files");
        assert_eq!(inspect(&empty, Layout::Freedesktop).expect("empty"), None);

        #[cfg(unix)]
        {
            let link = tmp.path().join("LinkTrash");
            std::os::unix::fs::symlink(&trash, &link).expect("symlink");
            assert!(
                inspect(&link, Layout::Freedesktop)
                    .expect("symlink")
                    .is_none()
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn remove_empties_files_and_info_and_keeps_links() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let base = fs::canonicalize(tmp.path()).expect("canonicalize");
        let outside = base.join("outside.txt");
        fs::write(&outside, b"keep me").expect("outside");

        let trash = make_freedesktop_trash(&base);
        let link = trash.join("files/outside-link");
        std::os::unix::fs::symlink(&outside, &link).expect("symlink");

        let folder = inspect(&trash, Layout::Freedesktop)
            .expect("inspect")
            .expect("folder");
        let rows = entries(vec![folder.clone()], &Cancel::new());
        let candidate = &rows[0].1;
        let guard = Guard::new(&roots(&folder)).with_cwd(None);

        let err = remove(&guard, candidate, Mode::Trash).expect_err("link should be skipped");
        assert!(
            err.to_string().contains("1 link"),
            "unexpected error: {err}"
        );

        let remaining_files: Vec<_> = fs::read_dir(trash.join("files"))
            .expect("files")
            .map(|e| e.expect("entry").file_name())
            .collect();
        assert_eq!(
            remaining_files,
            vec![std::ffi::OsString::from("outside-link")]
        );
        assert_eq!(fs::read_dir(trash.join("info")).expect("info").count(), 0);
        assert!(outside.exists());
        assert!(trash.join("directorysizes").exists());
    }

    #[test]
    fn remove_refuses_a_swapped_folder() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let base = fs::canonicalize(tmp.path()).expect("canonicalize");
        let trash = make_freedesktop_trash(&base);
        let folder = inspect(&trash, Layout::Freedesktop)
            .expect("inspect")
            .expect("folder");
        let mut rows = entries(vec![folder.clone()], &Cancel::new());
        rows[0].1.identity = Some((0, 0));
        let guard = Guard::new(&roots(&folder)).with_cwd(None);

        let err = remove(&guard, &rows[0].1, Mode::Permanent).expect_err("swapped folder");
        assert!(err.to_string().contains("changed"), "unexpected: {err}");
        assert!(trash.join("files/a.txt").exists());
    }
}
