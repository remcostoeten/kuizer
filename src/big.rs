use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::config::{Config, ConfigError};
use crate::detect::{Category, Kind, RuleInfo, Safety};
use crate::platform::{self, MountTable, dir::EntryKind};
use crate::protect::Protected;
use crate::scan::walk::{FileEntry, Flow, Visitor, Walker};
use crate::scan::{self, Cancel, Candidate, Issue, Usage};
use crate::units::{ByteSize, Idle};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BigConfig {
    pub roots: Vec<PathBuf>,
    pub min_size: ByteSize,
    pub limit: usize,
    pub downloads: bool,
    pub downloads_age: Idle,
    pub downloads_min_size: ByteSize,
}

impl Default for BigConfig {
    fn default() -> Self {
        Self {
            roots: Vec::new(),
            min_size: ByteSize(500_000_000),
            limit: 50,
            downloads: true,
            downloads_age: Idle(Duration::from_secs(90 * 86_400)),
            downloads_min_size: ByteSize(10_000_000),
        }
    }
}

impl BigConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.min_size.0 == 0 {
            return Err(ConfigError::Invalid {
                key: "big.min_size".to_owned(),
                message: "must be greater than 0".to_owned(),
            });
        }
        if self.limit == 0 {
            return Err(ConfigError::Invalid {
                key: "big.limit".to_owned(),
                message: "must be at least 1".to_owned(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LargeFile {
    pub path: PathBuf,
    pub size: u64,
    pub allocated: u64,
    pub identity: Option<(u64, u64)>,
    pub modified: Option<SystemTime>,
}

#[derive(Debug, Default)]
pub struct Found {
    pub files: Vec<LargeFile>,
    pub issues: Vec<Issue>,
}

struct Large<'a> {
    min_size: u64,
    found: Vec<LargeFile>,
    issues: Vec<Issue>,
    files: u64,
    on_progress: &'a mut dyn FnMut(u64),
}

impl Visitor for Large<'_> {
    fn wants_files(&self) -> bool {
        true
    }

    fn wants_stat(&self) -> bool {
        true
    }

    fn issue(&mut self, issue: Issue) -> Flow {
        self.issues.push(issue);
        Flow::Continue
    }

    fn counted(&mut self, kind: EntryKind) {
        if kind == EntryKind::File {
            self.files += 1;
            if self.files.is_multiple_of(4096) {
                (self.on_progress)(self.files);
            }
        }
    }

    fn entry(&mut self, entry: &FileEntry<'_>) -> Flow {
        if entry.kind != EntryKind::File {
            return Flow::Continue;
        }
        let Some(stat) = entry.stat else {
            return Flow::Continue;
        };
        if stat.len < self.min_size {
            return Flow::Continue;
        }
        self.found.push(LargeFile {
            path: entry.path().into_owned(),
            size: stat.len,
            allocated: stat.allocated,
            identity: Some((stat.device, stat.inode)),
            modified: stat.modified,
        });
        Flow::Continue
    }
}

/// @name find
/// @description Walks `roots` outside artifacts, excludes, and protected app data, returning
/// every regular file of at least `min_size` apparent bytes, largest first.
///
/// @example
/// let found = big::find(&config, &roots, 500_000_000, |_| {})?;
pub fn find(
    config: &Config,
    roots: &[PathBuf],
    min_size: u64,
    mut on_progress: impl FnMut(u64),
) -> Result<Found, ConfigError> {
    let detector = config.detector()?;
    let excludes = config.excludes()?;
    let mounts = MountTable::load();
    let protected = Protected::for_user();
    let mut walker = Walker {
        scan: &config.scan,
        detector: &detector,
        excludes: &excludes,
        mounts: &mounts,
        protected: &protected,
        visitor: Large {
            min_size,
            found: Vec::new(),
            issues: Vec::new(),
            files: 0,
            on_progress: &mut on_progress,
        },
    };
    for root in roots {
        walker.walk_root(root);
    }
    let mut files = walker.visitor.found;
    files.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.path.cmp(&b.path)));
    Ok(Found {
        files,
        issues: walker.visitor.issues,
    })
}

/// @name downloads_dir
/// @description Returns the user's Downloads folder if this platform defines one.
///
/// @example
/// if let Some(dir) = big::downloads_dir() { println!("{}", dir.display()); }
pub fn downloads_dir() -> Option<PathBuf> {
    dirs::download_dir()
}

/// @name old_downloads
/// @description Inspects direct children of `dir` and returns files or folders whose newest
/// modification time is older than `now - age` and whose size is at least `min_size`.
///
/// @example
/// let old = big::old_downloads(&dir, Duration::from_secs(90 * 86_400), 10_000_000, SystemTime::now(), &Protected::for_user());
pub fn old_downloads(
    dir: &Path,
    age: Duration,
    min_size: u64,
    now: SystemTime,
    protected: &Protected,
) -> Vec<LargeFile> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let cancel = Cancel::new();
    let mut out = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();
        if protected.refuses(&path) {
            continue;
        }
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        let file_type = meta.file_type();
        if file_type.is_symlink() {
            continue;
        }
        let item = if file_type.is_file() {
            let size = meta.len();
            if size < min_size {
                continue;
            }
            let Some(modified) = meta.modified().ok() else {
                continue;
            };
            LargeFile {
                path,
                size,
                allocated: platform::allocated(&meta),
                identity: platform::identity(&meta),
                modified: Some(modified),
            }
        } else if file_type.is_dir() {
            let measured = scan::measure(&path, &cancel);
            if measured.usage.allocated < min_size {
                continue;
            }
            let Some(modified) = measured.newest else {
                continue;
            };
            LargeFile {
                path,
                size: measured.usage.apparent,
                allocated: measured.usage.allocated,
                identity: measured.identity,
                modified: Some(modified),
            }
        } else {
            continue;
        };
        let Some(modified) = item.modified else {
            continue;
        };
        let older = now
            .duration_since(modified)
            .is_ok_and(|elapsed| elapsed > age);
        if older {
            out.push(item);
        }
    }
    out.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.path.cmp(&b.path)));
    out
}

/// @name candidates
/// @description Turns large files and old downloads into review-only candidates. A path present
/// in both lists appears once, classified as `old-download`.
///
/// @example
/// let rows = big::candidates(found.files, old);
pub fn candidates(large: Vec<LargeFile>, old: Vec<LargeFile>) -> Vec<Candidate> {
    let large_rule = Arc::new(RuleInfo {
        name: "large-file".to_owned(),
        category: Category::Files,
        safety: Safety::Review,
        ecosystem: "files".to_owned(),
    });
    let download_rule = Arc::new(RuleInfo {
        name: "old-download".to_owned(),
        category: Category::Files,
        safety: Safety::Review,
        ecosystem: "files".to_owned(),
    });
    let old_paths: HashSet<PathBuf> = old.iter().map(|item| item.path.clone()).collect();
    let mut combined: Vec<(LargeFile, Arc<RuleInfo>)> = Vec::with_capacity(old.len() + large.len());
    for item in old {
        combined.push((item, Arc::clone(&download_rule)));
    }
    for item in large {
        if !old_paths.contains(&item.path) {
            combined.push((item, Arc::clone(&large_rule)));
        }
    }
    combined.sort_by(|(a, _), (b, _)| b.size.cmp(&a.size).then_with(|| a.path.cmp(&b.path)));
    combined
        .into_iter()
        .enumerate()
        .map(|(index, (item, rule))| {
            let kind = if item.path.is_dir() {
                Kind::Dir
            } else {
                Kind::File
            };
            Candidate {
                id: (index + 1) as u64,
                path: item.path,
                kind,
                rule,
                project: None,
                usage: Usage {
                    allocated: item.allocated,
                    apparent: item.size,
                    entries: 1,
                    partial: false,
                },
                newest: item.modified,
                identity: item.identity,
                project_last_active: None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use super::*;

    fn sparse_file(path: &Path, size: u64) -> File {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent");
        }
        let file = File::create(path).expect("create");
        file.set_len(size).expect("set_len");
        file
    }

    #[test]
    fn finds_large_files_outside_artifacts() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = fs::canonicalize(tmp.path()).expect("canonicalize");
        let video = home.join("Videos/movie.mkv");
        sparse_file(&video, 600_000_000);
        fs::create_dir_all(home.join("web/node_modules/pkg")).expect("node_modules");
        fs::write(home.join("web/package.json"), b"{}").expect("package.json");
        sparse_file(&home.join("web/node_modules/pkg/blob.bin"), 600_000_000);
        sparse_file(&home.join("Videos/small.txt"), 1024);

        let found = find(&Config::default(), &[home], 500_000_000, |_| {}).expect("find");
        assert_eq!(found.files.len(), 1);
        assert_eq!(found.files[0].path, video);
        assert_eq!(found.files[0].size, 600_000_000);
    }

    #[test]
    fn old_downloads_respect_age_and_size() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = fs::canonicalize(tmp.path()).expect("canonicalize");
        let now = SystemTime::now();
        let old_time = now - Duration::from_secs(120 * 86_400);

        let old_big = dir.join("installer.iso");
        sparse_file(&old_big, 20_000_000)
            .set_modified(old_time)
            .expect("set_modified");

        let new_big = dir.join("recent.iso");
        sparse_file(&new_big, 20_000_000);

        let old_small = dir.join("note.txt");
        sparse_file(&old_small, 1024)
            .set_modified(old_time)
            .expect("set_modified");

        #[cfg(unix)]
        std::os::unix::fs::symlink(&old_big, dir.join("link.iso")).expect("symlink");

        let found = old_downloads(
            &dir,
            Duration::from_secs(90 * 86_400),
            10_000_000,
            now,
            &Protected::for_user(),
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, old_big);
    }

    #[test]
    fn a_path_in_both_lists_is_one_old_download() {
        let path = PathBuf::from("/tmp/Downloads/archive.tar");
        let item = LargeFile {
            path: path.clone(),
            size: 700_000_000,
            allocated: 700_000_000,
            identity: Some((1, 2)),
            modified: Some(SystemTime::UNIX_EPOCH),
        };
        let rows = candidates(vec![item.clone()], vec![item]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].path, path);
        assert_eq!(rows[0].rule.name, "old-download");
        assert_eq!(rows[0].rule.category, Category::Files);
        assert_eq!(rows[0].rule.safety, Safety::Review);
    }
}
