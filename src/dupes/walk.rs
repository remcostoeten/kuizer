use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::Instant;

use super::{Options, Seen, Stage};
use crate::platform::dir::{EntryKind, Stat};
use crate::protect::Protected;
use crate::scan::walk::{Companion, Flow, Visitor};
use crate::scan::{Cancel, Issue};
use crate::{caches, toolchains};

#[derive(Debug, Clone)]
pub(super) struct Skip {
    protected: Protected,
    tool_roots: Vec<PathBuf>,
    lifted: Vec<PathBuf>,
    last: Option<(PathBuf, bool)>,
}

impl Skip {
    pub(super) fn for_user(roots: &[PathBuf]) -> Self {
        let mut tool_roots = Vec::new();
        if let Some(locations) = caches::Locations::for_user() {
            tool_roots.extend(caches::roots(&locations));
        }
        if let Some(locations) = toolchains::Locations::for_user() {
            tool_roots.extend(locations.roots());
        }
        Self::new(Protected::for_user(), tool_roots, roots)
    }

    pub(super) fn new(protected: Protected, tool_roots: Vec<PathBuf>, roots: &[PathBuf]) -> Self {
        let mut skip = Self {
            protected,
            tool_roots,
            lifted: Vec::new(),
            last: None,
        };
        skip.lifted = roots
            .iter()
            .filter(|root| skip.hidden(root))
            .cloned()
            .collect();
        skip
    }

    fn hidden(&self, dir: &Path) -> bool {
        self.protected.in_hidden_home_dir(dir)
            || self.tool_roots.iter().any(|root| dir.starts_with(root))
    }

    pub(super) fn skips(&mut self, path: &Path) -> bool {
        path.parent().is_some_and(|dir| self.skips_dir(dir))
    }

    fn skips_dir(&mut self, dir: &Path) -> bool {
        if let Some((last, skipped)) = &self.last
            && last == dir
        {
            return *skipped;
        }
        let skipped = self.hidden(dir) && !self.lifted.iter().any(|root| dir.starts_with(root));
        self.last = Some((dir.to_path_buf(), skipped));
        skipped
    }
}

#[derive(Clone, Copy)]
struct Entry {
    start: usize,
    dir: u32,
    len: u32,
}

#[derive(Default)]
pub(super) struct Paths {
    dirs: Vec<PathBuf>,
    names: Vec<u8>,
    entries: Vec<Entry>,
    stats: Vec<Stat>,
}

impl Paths {
    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(super) fn push(&mut self, path: &Path, stat: Stat) {
        let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
            return;
        };
        let Ok(len) = u32::try_from(name.len()) else {
            return;
        };
        if self.dirs.last().is_none_or(|last| last != dir) {
            self.dirs.push(dir.to_path_buf());
        }
        let Ok(dir) = u32::try_from(self.dirs.len() - 1) else {
            return;
        };
        self.entries.push(Entry {
            start: self.names.len(),
            dir,
            len,
        });
        self.names.extend_from_slice(name.as_bytes());
        self.stats.push(stat);
    }

    pub(super) fn fill(&self, index: usize, buffer: &mut PathBuf) {
        let entry = self.entries[index];
        let name = &self.names[entry.start..entry.start + entry.len as usize];
        buffer.clear();
        buffer.push(&self.dirs[entry.dir as usize]);
        buffer.push(OsStr::from_bytes(name));
    }

    pub(super) fn append(&mut self, other: Self) {
        let Ok(dirs) = u32::try_from(self.dirs.len()) else {
            return;
        };
        let names = self.names.len();
        self.entries
            .extend(other.entries.into_iter().map(|entry| Entry {
                start: entry.start + names,
                dir: entry.dir + dirs,
                len: entry.len,
            }));
        self.dirs.extend(other.dirs);
        self.names.extend(other.names);
        self.stats.extend(other.stats);
    }
}

pub struct Files {
    min_size: u64,
    pub(super) roots: Vec<PathBuf>,
    pub(super) paths: Paths,
    skip: Option<Skip>,
    skipping: bool,
    pub(super) issues: Vec<Issue>,
    pub(super) counted: u64,
    pub(super) started: Instant,
}

impl Files {
    pub fn new(options: Options) -> Self {
        Self::for_roots(options, &[])
    }

    /// @name for_roots
    /// @description Like `new`, and keeps looking inside a scan root that is itself in a hidden or
    /// tool-managed folder, even when `include_hidden` is off.
    ///
    /// @example
    /// let files = Files::for_roots(options, &[PathBuf::from("/home/me/.config/app")]);
    pub fn for_roots(options: Options, roots: &[PathBuf]) -> Self {
        let mut files = Self::with_skip(
            options,
            (!options.include_hidden).then(|| Skip::for_user(roots)),
        );
        files.roots = roots.to_vec();
        files
    }

    pub(super) fn with_skip(options: Options, skip: Option<Skip>) -> Self {
        Self {
            min_size: options.min_size,
            roots: Vec::new(),
            paths: Paths::default(),
            skip,
            skipping: false,
            issues: Vec::new(),
            counted: 0,
            started: Instant::now(),
        }
    }

    /// @name Files::at_least
    /// @description Returns the collected files of at least `min_size` bytes, largest first, so a
    /// caller that walked once for duplicates also gets the large files of that walk.
    ///
    /// @example
    /// let large = files.at_least(500_000_000);
    pub fn at_least(&self, min_size: u64) -> Vec<crate::big::LargeFile> {
        let mut found = Vec::new();
        let mut buffer = PathBuf::new();
        for (index, stat) in self.paths.stats.iter().enumerate() {
            if stat.len < min_size {
                continue;
            }
            self.paths.fill(index, &mut buffer);
            found.push(crate::big::LargeFile {
                path: buffer.clone(),
                size: stat.len,
                allocated: stat.allocated,
                identity: Some((stat.device, stat.inode)),
                modified: stat.modified,
            });
        }
        found.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.path.cmp(&b.path)));
        found
    }
}

impl Visitor for Files {
    fn wants_files(&self) -> bool {
        !self.skipping
    }

    fn wants_stat(&self) -> bool {
        true
    }

    fn issue(&mut self, issue: Issue) -> Flow {
        self.issues.push(issue);
        Flow::Continue
    }

    fn entered(&mut self, path: &Path) -> Flow {
        self.skipping = self.skip.as_mut().is_some_and(|skip| skip.skips_dir(path));
        if self.skipping {
            Flow::Skip
        } else {
            Flow::Continue
        }
    }

    fn counted(&mut self, kind: EntryKind) {
        if kind == EntryKind::File {
            self.counted += 1;
        }
    }

    fn file(&mut self, path: &Path, kind: EntryKind) -> Flow {
        if kind != EntryKind::File {
            return Flow::Continue;
        }
        if let Some(skip) = &mut self.skip
            && skip.skips(path)
        {
            return Flow::Continue;
        }
        Flow::Continue
    }

    fn entry(&mut self, entry: &crate::scan::walk::FileEntry<'_>) -> Flow {
        if entry.kind != EntryKind::File {
            return Flow::Continue;
        }
        let Some(stat) = entry.stat else {
            return Flow::Continue;
        };
        if stat.len == 0 || stat.len < self.min_size {
            return Flow::Continue;
        }
        self.paths.push(&entry.path(), stat);
        Flow::Continue
    }
}

impl Companion for Files {
    fn fork(&self) -> Self {
        Self {
            min_size: self.min_size,
            roots: Vec::new(),
            paths: Paths::default(),
            skip: self.skip.clone(),
            skipping: false,
            issues: Vec::new(),
            counted: 0,
            started: self.started,
        }
    }

    fn merge(&mut self, other: Self) {
        self.paths.append(other.paths);
        self.issues.extend(other.issues);
        self.counted += other.counted;
    }
}

pub(super) struct Reporting<'a> {
    pub(super) files: Files,
    pub(super) cancel: &'a Cancel,
    pub(super) on_stage: &'a mut dyn FnMut(Stage),
}

impl Visitor for Reporting<'_> {
    fn wants_files(&self) -> bool {
        self.files.wants_files()
    }

    fn wants_stat(&self) -> bool {
        self.files.wants_stat()
    }

    fn cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    fn issue(&mut self, issue: Issue) -> Flow {
        self.files.issue(issue)
    }

    fn entered(&mut self, path: &Path) -> Flow {
        self.files.entered(path)
    }

    fn counted(&mut self, kind: EntryKind) {
        self.files.counted(kind);
        if kind == EntryKind::File && self.files.counted.is_multiple_of(4096) {
            (self.on_stage)(Stage::Walking {
                files: self.files.counted,
            });
        }
    }

    fn file(&mut self, path: &Path, kind: EntryKind) -> Flow {
        self.files.file(path, kind)
    }

    fn entry(&mut self, entry: &crate::scan::walk::FileEntry<'_>) -> Flow {
        self.files.entry(entry)
    }
}

pub(super) fn seen(paths: &Paths) -> Vec<Seen> {
    let mut seen = Vec::new();
    let mut buffer = PathBuf::new();
    for (index, stat) in paths.stats.iter().enumerate() {
        paths.fill(index, &mut buffer);
        seen.push(Seen {
            path: buffer.clone(),
            size: stat.len,
            allocated: stat.allocated,
            identity: stat.shared_inode(),
            modified: stat.modified,
        });
    }
    seen
}
