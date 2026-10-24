use std::borrow::Cow;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::types::{Issue, IssueKind};
use crate::config::{Excludes, ScanConfig};
use crate::detect::{Detector, Kind, Listing, Match, RuleInfo, Safety};
use crate::platform::dir::{Buffer, Dir, EntryKind};
use crate::platform::{self, MountTable};
use crate::protect::{self, Protected};

const GIT_DIR: &str = ".git";

pub struct Found {
    pub path: PathBuf,
    pub kind: Kind,
    pub rule: Match,
}

pub enum Flow {
    Continue,
    Stop,
}

pub struct FileEntry<'a> {
    parent: &'a Path,
    built: Option<&'a Path>,
    pub name: &'a OsStr,
    pub kind: EntryKind,
}

impl FileEntry<'_> {
    /// @name FileEntry::path
    /// @description Returns the full path of the file, built on demand.
    ///
    /// @example
    /// if is_pin_file(entry.name) { pins.push(entry.path().into_owned()); }
    pub fn path(&self) -> Cow<'_, Path> {
        match self.built {
            Some(path) => Cow::Borrowed(path),
            None => Cow::Owned(self.parent.join(self.name)),
        }
    }
}

pub trait Visitor {
    fn cancelled(&self) -> bool {
        false
    }

    /// Whether [`Visitor::file`] or [`Visitor::entry`] wants to be called. Walkers skip
    /// building a path per file when nothing asks for it.
    fn wants_files(&self) -> bool {
        false
    }

    fn issue(&mut self, issue: Issue) -> Flow;

    fn entered(&mut self, _path: &Path) {}

    fn counted(&mut self, _kind: EntryKind) {}

    fn artifact(&mut self, _found: Found) -> Flow {
        Flow::Continue
    }

    fn file(&mut self, _path: &Path, _kind: EntryKind) -> Flow {
        Flow::Continue
    }

    fn entry(&mut self, entry: &FileEntry<'_>) -> Flow {
        if self.wants_files() {
            self.file(&entry.path(), entry.kind)
        } else {
            Flow::Continue
        }
    }

    fn repository(&mut self, _root: &Path) {}
}

/// A visitor that rides along a parallel scan. Every scan worker gets its own `fork`, and the
/// forks are merged back into one when the walk ends.
pub trait Companion: Visitor + Send + Sized + 'static {
    fn fork(&self) -> Self;

    fn merge(&mut self, other: Self);
}

impl Visitor for () {
    fn issue(&mut self, _issue: Issue) -> Flow {
        Flow::Continue
    }
}

impl Companion for () {
    fn fork(&self) -> Self {}

    fn merge(&mut self, _other: Self) {}
}

impl<V: Visitor> Visitor for Option<V> {
    fn wants_files(&self) -> bool {
        self.as_ref().is_some_and(V::wants_files)
    }

    fn issue(&mut self, issue: Issue) -> Flow {
        match self {
            Some(visitor) => visitor.issue(issue),
            None => Flow::Continue,
        }
    }

    fn counted(&mut self, kind: EntryKind) {
        if let Some(visitor) = self {
            visitor.counted(kind);
        }
    }

    fn file(&mut self, path: &Path, kind: EntryKind) -> Flow {
        match self {
            Some(visitor) => visitor.file(path, kind),
            None => Flow::Continue,
        }
    }

    fn entry(&mut self, entry: &FileEntry<'_>) -> Flow {
        match self {
            Some(visitor) => visitor.entry(entry),
            None => Flow::Continue,
        }
    }

    fn repository(&mut self, root: &Path) {
        if let Some(visitor) = self {
            visitor.repository(root);
        }
    }
}

impl<V: Companion> Companion for Option<V> {
    fn fork(&self) -> Self {
        self.as_ref().map(V::fork)
    }

    fn merge(&mut self, other: Self) {
        if let (Some(visitor), Some(other)) = (self, other) {
            visitor.merge(other);
        }
    }
}

impl<A: Visitor, B: Visitor> Visitor for (A, B) {
    fn wants_files(&self) -> bool {
        self.0.wants_files() || self.1.wants_files()
    }

    fn issue(&mut self, issue: Issue) -> Flow {
        match (self.0.issue(issue.clone()), self.1.issue(issue)) {
            (Flow::Continue, Flow::Continue) => Flow::Continue,
            _ => Flow::Stop,
        }
    }

    fn counted(&mut self, kind: EntryKind) {
        self.0.counted(kind);
        self.1.counted(kind);
    }

    fn file(&mut self, path: &Path, kind: EntryKind) -> Flow {
        match (self.0.file(path, kind), self.1.file(path, kind)) {
            (Flow::Continue, Flow::Continue) => Flow::Continue,
            _ => Flow::Stop,
        }
    }

    fn entry(&mut self, entry: &FileEntry<'_>) -> Flow {
        match (self.0.entry(entry), self.1.entry(entry)) {
            (Flow::Continue, Flow::Continue) => Flow::Continue,
            _ => Flow::Stop,
        }
    }

    fn repository(&mut self, root: &Path) {
        self.0.repository(root);
        self.1.repository(root);
    }
}

impl<A: Companion, B: Companion> Companion for (A, B) {
    fn fork(&self) -> Self {
        (self.0.fork(), self.1.fork())
    }

    fn merge(&mut self, other: Self) {
        self.0.merge(other.0);
        self.1.merge(other.1);
    }
}

pub struct Walker<'a, V> {
    pub scan: &'a ScanConfig,
    pub detector: &'a Detector,
    pub excludes: &'a Excludes,
    pub mounts: &'a MountTable,
    pub protected: &'a Protected,
    pub visitor: V,
}

impl<V: Visitor> Walker<'_, V> {
    pub fn walk_root(&mut self, root: &Path) {
        let context = Context::new(
            self.scan,
            self.detector,
            self.excludes,
            self.mounts,
            self.protected,
        );
        let Some(first) = context.root(&mut self.visitor, root) else {
            return;
        };
        let mut scratch = Scratch::default();
        let mut stack = vec![first];
        while let Some(pending) = stack.pop() {
            if self.visitor.cancelled() {
                return;
            }
            if let Flow::Stop = context.visit(&mut self.visitor, pending, &mut scratch, &mut stack)
            {
                return;
            }
        }
    }
}

pub(crate) struct Child {
    pub path: PathBuf,
    pub probe: bool,
    pub splits: bool,
}

pub(crate) struct Pending {
    pub path: PathBuf,
    parent: Arc<Listing>,
    depth: usize,
    device: u64,
    probe: bool,
    splits: bool,
    inode: u64,
}

struct Entry {
    start: usize,
    end: usize,
    kind: EntryKind,
    inode: u64,
}

/// Per-thread buffers a walk reuses from one directory to the next.
#[derive(Default)]
pub(crate) struct Scratch {
    buffer: Buffer,
    names: Vec<u8>,
    entries: Vec<Entry>,
}

pub(crate) struct Context<'a> {
    scan: &'a ScanConfig,
    detector: &'a Detector,
    excludes: &'a Excludes,
    mounts: &'a MountTable,
    protected: &'a Protected,
    excluded_mounts: HashSet<&'a Path>,
}

fn name_of<'n>(names: &'n [u8], entry: &Entry) -> &'n OsStr {
    OsStr::from_bytes(&names[entry.start..entry.end])
}

impl<'a> Context<'a> {
    pub fn new(
        scan: &'a ScanConfig,
        detector: &'a Detector,
        excludes: &'a Excludes,
        mounts: &'a MountTable,
        protected: &'a Protected,
    ) -> Self {
        Self {
            scan,
            detector,
            excludes,
            mounts,
            protected,
            excluded_mounts: scan.excluded_mounts.iter().map(PathBuf::as_path).collect(),
        }
    }

    fn emit<V: Visitor>(&self, visitor: &mut V, mut found: Found) -> Flow {
        if found.rule.safety == Safety::Safe && self.protected.in_hidden_home_dir(&found.path) {
            found.rule = Arc::new(RuleInfo {
                safety: Safety::Review,
                ..RuleInfo::clone(&found.rule)
            });
        }
        visitor.artifact(found)
    }

    fn excluded_mount(&self, path: &Path) -> bool {
        self.scan
            .excluded_mounts
            .iter()
            .any(|mount| path.starts_with(mount))
            || self.mounts.fs_type(path).is_some_and(|fs_type| {
                self.scan
                    .excluded_fs_types
                    .iter()
                    .any(|excluded| excluded == fs_type)
            })
    }

    /// @name Context::child
    /// @description Decides whether the subdirectory `name` of `parent` is entered at all and
    /// how: skipped names, excludes, known protected directories and excluded mounts are
    /// refused, mount points and btrfs-style splits are marked for a device check. `splits` is
    /// whether `parent` may hold subvolumes on other devices.
    ///
    /// @example
    /// let Some(child) = context.child(&dir, name, pending.splits) else { continue };
    pub fn child(&self, parent: &Path, name: &OsStr, splits: bool) -> Option<Child> {
        if self.scan.skip_dirs.iter().any(|skip| name == skip.as_str()) || self.excludes.name(name)
        {
            return None;
        }
        let path = parent.join(name);
        if self.excludes.full_path(&path)
            || self.protected.is_known(&path)
            || self.excluded_mounts.contains(path.as_path())
        {
            return None;
        }
        let mount = self.mounts.is_mount_point(&path);
        let below = if mount {
            self.mounts.device_may_change_below(&path)
        } else {
            splits
        };
        Some(Child {
            path,
            probe: mount || splits,
            splits: below,
        })
    }

    /// @name Context::same_device
    /// @description Compares the device of the open `dir` with `expected`. Returns the device
    /// to carry on with, or `None` when the directory sits on another device that the scan
    /// must not enter: `one_file_system` is set or the mount is excluded.
    ///
    /// @example
    /// match context.same_device(&dir, pending.device, &pending.path) { Ok(Some(device)) => walk(device), _ => return }
    pub fn same_device(&self, dir: &Dir, expected: u64, path: &Path) -> io::Result<Option<u64>> {
        let device = dir.stat()?.device;
        if device == expected {
            return Ok(Some(device));
        }
        if self.scan.one_file_system || self.excluded_mount(path) {
            return Ok(None);
        }
        Ok(Some(device))
    }

    /// @name Context::splits
    /// @description Returns whether directories below `root` can sit on other devices without
    /// being mount points, so every one needs a device check.
    ///
    /// @example
    /// let splits = context.splits(&root);
    pub fn splits(&self, root: &Path) -> bool {
        self.mounts.device_may_change_below(root)
    }

    /// @name Context::prunes
    /// @description Returns whether a directory with this listing is left alone entirely: it
    /// holds the ignore file or browser and application data.
    ///
    /// @example
    /// if context.prunes(&own) { return Flow::Continue; }
    pub fn prunes(&self, own: &Listing) -> bool {
        self.scan.ignores(self.detector, own) || protect::holds_app_data(self.detector, own)
    }

    /// @name Context::root
    /// @description Checks a scan root and returns it as the first directory to visit, or
    /// reports why it is skipped.
    ///
    /// @example
    /// let first = context.root(&mut visitor, &root);
    pub fn root<V: Visitor>(&self, visitor: &mut V, root: &Path) -> Option<Pending> {
        let metadata = match fs::metadata(root) {
            Ok(metadata) if metadata.is_dir() => metadata,
            Ok(_) => {
                visitor.issue(Issue {
                    path: root.to_path_buf(),
                    kind: IssueKind::InvalidRoot,
                    message: "not a directory".to_owned(),
                });
                return None;
            }
            Err(error) => {
                visitor.issue(Issue::from_io(root.to_path_buf(), &error));
                return None;
            }
        };
        if self.excluded_mount(root) {
            visitor.issue(Issue {
                path: root.to_path_buf(),
                kind: IssueKind::SkippedMount,
                message: "root is on an excluded mount or filesystem type".to_owned(),
            });
            return None;
        }
        if self.protected.covers(root) {
            visitor.issue(Issue {
                path: root.to_path_buf(),
                kind: IssueKind::Protected,
                message: "root holds browser or application data".to_owned(),
            });
            return None;
        }
        Some(Pending {
            path: root.to_path_buf(),
            parent: Arc::new(Listing::default()),
            depth: 0,
            device: platform::device(&metadata),
            probe: false,
            splits: self.splits(root),
            inode: 0,
        })
    }

    /// @name Context::visit
    /// @description Lists one directory, reports what it finds to `visitor` and appends the
    /// subdirectories still to walk to `out`, lowest inode last so a stack pops it first.
    ///
    /// @example
    /// while let Some(pending) = stack.pop() { context.visit(&mut visitor, pending, &mut scratch, &mut stack); }
    pub fn visit<V: Visitor>(
        &self,
        visitor: &mut V,
        pending: Pending,
        scratch: &mut Scratch,
        out: &mut Vec<Pending>,
    ) -> Flow {
        let dir = match Dir::open(&pending.path, pending.depth == 0) {
            Ok(dir) => dir,
            Err(error) => return visitor.issue(Issue::from_io(pending.path, &error)),
        };
        let mut device = pending.device;
        if pending.probe {
            match self.same_device(&dir, pending.device, &pending.path) {
                Ok(Some(found)) => device = found,
                Ok(None) => return Flow::Continue,
                Err(error) => return visitor.issue(Issue::from_io(pending.path, &error)),
            }
        }
        visitor.entered(&pending.path);

        let Scratch {
            buffer,
            names,
            entries,
        } = scratch;
        names.clear();
        entries.clear();
        let read = dir.read(buffer, |name, kind, inode| {
            let start = names.len();
            names.extend_from_slice(name.as_encoded_bytes());
            entries.push(Entry {
                start,
                end: names.len(),
                kind,
                inode,
            });
        });
        if let Err(error) = read
            && let Flow::Stop = visitor.issue(Issue::from_io(pending.path.clone(), &error))
        {
            return Flow::Stop;
        }

        let mut own = Listing::default();
        for entry in entries.iter_mut() {
            let name = name_of(names, entry);
            if entry.kind == EntryKind::Unknown {
                match dir.kind_at(name) {
                    Ok(kind) => entry.kind = kind,
                    Err(error) => {
                        if let Flow::Stop =
                            visitor.issue(Issue::from_io(pending.path.join(name), &error))
                        {
                            return Flow::Stop;
                        }
                        continue;
                    }
                }
            }
            self.detector.note(&mut own, name);
        }
        drop(dir);

        if self.prunes(&own) {
            return Flow::Continue;
        }
        if pending.depth > 0
            && let Some(rule) = self
                .detector
                .classify_dir(&pending.path, &pending.parent, &own)
        {
            if !self.detector.reports(&rule) {
                return Flow::Continue;
            }
            return self.emit(
                visitor,
                Found {
                    path: pending.path,
                    kind: Kind::Dir,
                    rule,
                },
            );
        }
        if self.scan.max_depth.is_some_and(|max| pending.depth >= max) {
            return Flow::Continue;
        }

        let own = Arc::new(own);
        let wants_files = visitor.wants_files();
        let first_child = out.len();
        for entry in entries.iter() {
            let name = name_of(names, entry);
            match entry.kind {
                EntryKind::Symlink | EntryKind::Unknown => {}
                EntryKind::Dir => {
                    if name == GIT_DIR {
                        visitor.repository(&pending.path);
                    }
                    let Some(child) = self.child(&pending.path, name, pending.splits) else {
                        continue;
                    };
                    out.push(Pending {
                        path: child.path,
                        parent: Arc::clone(&own),
                        depth: pending.depth + 1,
                        device,
                        probe: child.probe,
                        splits: child.splits,
                        inode: entry.inode,
                    });
                }
                EntryKind::File | EntryKind::Other => {
                    visitor.counted(entry.kind);
                    if self.excludes.extension(name) || self.excludes.name(name) {
                        continue;
                    }
                    let path = self
                        .excludes
                        .matches_paths()
                        .then(|| pending.path.join(name));
                    if let Some(path) = &path
                        && self.excludes.full_path(path)
                    {
                        continue;
                    }
                    if wants_files
                        && let Flow::Stop = visitor.entry(&FileEntry {
                            parent: &pending.path,
                            built: path.as_deref(),
                            name,
                            kind: entry.kind,
                        })
                    {
                        return Flow::Stop;
                    }
                    if let Some(rule) = self.detector.classify_file(name, &own)
                        && self.detector.reports(&rule)
                        && let Flow::Stop = self.emit(
                            visitor,
                            Found {
                                path: path.unwrap_or_else(|| pending.path.join(name)),
                                kind: Kind::File,
                                rule,
                            },
                        )
                    {
                        return Flow::Stop;
                    }
                }
            }
        }
        out[first_child..].sort_unstable_by_key(|child| std::cmp::Reverse(child.inode));
        Flow::Continue
    }
}
