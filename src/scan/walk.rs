use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs;
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

pub trait Visitor {
    fn cancelled(&self) -> bool {
        false
    }

    /// Whether [`Visitor::file`] wants to be called. Walkers skip building a path per file
    /// when nothing asks for it.
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
            splits: self.mounts.device_may_change_below(root),
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
            match dir.stat() {
                Ok(stat) if stat.device != pending.device => {
                    if self.scan.one_file_system || self.excluded_mount(&pending.path) {
                        return Flow::Continue;
                    }
                    device = stat.device;
                }
                Ok(_) => {}
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

        if self.scan.ignores(self.detector, &own) || protect::holds_app_data(self.detector, &own) {
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
        let file_paths = wants_files || self.excludes.matches_paths();
        let first_child = out.len();
        for entry in entries.iter() {
            let name = name_of(names, entry);
            match entry.kind {
                EntryKind::Symlink | EntryKind::Unknown => {}
                EntryKind::Dir => {
                    if name == GIT_DIR {
                        visitor.repository(&pending.path);
                    }
                    if self.scan.skip_dirs.iter().any(|skip| name == skip.as_str())
                        || self.excludes.name(name)
                    {
                        continue;
                    }
                    let path = pending.path.join(name);
                    if self.excludes.full_path(&path)
                        || self.protected.is_known(&path)
                        || self.excluded_mounts.contains(path.as_path())
                    {
                        continue;
                    }
                    let mount = self.mounts.is_mount_point(&path);
                    let splits = if mount {
                        self.mounts.device_may_change_below(&path)
                    } else {
                        pending.splits
                    };
                    out.push(Pending {
                        path,
                        parent: Arc::clone(&own),
                        depth: pending.depth + 1,
                        device,
                        probe: mount || pending.splits,
                        splits,
                        inode: entry.inode,
                    });
                }
                EntryKind::File | EntryKind::Other => {
                    visitor.counted(entry.kind);
                    if self.excludes.extension(name) || self.excludes.name(name) {
                        continue;
                    }
                    let path = file_paths.then(|| pending.path.join(name));
                    if let Some(path) = &path {
                        if self.excludes.full_path(path) {
                            continue;
                        }
                        if wants_files && let Flow::Stop = visitor.file(path, entry.kind) {
                            return Flow::Stop;
                        }
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
