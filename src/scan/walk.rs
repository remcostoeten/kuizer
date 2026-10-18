use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::types::{Issue, IssueKind};
use crate::config::{Excludes, ScanConfig};
use crate::detect::{Detector, Kind, Listing, Match, RuleInfo, Safety};
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

    fn issue(&mut self, issue: Issue) -> Flow;

    fn entered(&mut self) {}

    fn counted(&mut self, _file_type: fs::FileType) {}

    fn artifact(&mut self, _found: Found) -> Flow {
        Flow::Continue
    }

    fn file(&mut self, _path: &Path, _file_type: fs::FileType) -> Flow {
        Flow::Continue
    }

    fn repository(&mut self, _root: &Path) {}
}

impl Visitor for () {
    fn issue(&mut self, _issue: Issue) -> Flow {
        Flow::Continue
    }
}

impl<V: Visitor> Visitor for Option<V> {
    fn issue(&mut self, issue: Issue) -> Flow {
        match self {
            Some(visitor) => visitor.issue(issue),
            None => Flow::Continue,
        }
    }

    fn counted(&mut self, file_type: fs::FileType) {
        if let Some(visitor) = self {
            visitor.counted(file_type);
        }
    }

    fn file(&mut self, path: &Path, file_type: fs::FileType) -> Flow {
        match self {
            Some(visitor) => visitor.file(path, file_type),
            None => Flow::Continue,
        }
    }

    fn repository(&mut self, root: &Path) {
        if let Some(visitor) = self {
            visitor.repository(root);
        }
    }
}

impl<A: Visitor, B: Visitor> Visitor for (A, B) {
    fn issue(&mut self, issue: Issue) -> Flow {
        match (self.0.issue(issue.clone()), self.1.issue(issue)) {
            (Flow::Continue, Flow::Continue) => Flow::Continue,
            _ => Flow::Stop,
        }
    }

    fn counted(&mut self, file_type: fs::FileType) {
        self.0.counted(file_type);
        self.1.counted(file_type);
    }

    fn file(&mut self, path: &Path, file_type: fs::FileType) -> Flow {
        match (self.0.file(path, file_type), self.1.file(path, file_type)) {
            (Flow::Continue, Flow::Continue) => Flow::Continue,
            _ => Flow::Stop,
        }
    }

    fn repository(&mut self, root: &Path) {
        self.0.repository(root);
        self.1.repository(root);
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

struct Pending {
    path: PathBuf,
    parent: Arc<Listing>,
    depth: usize,
    device: u64,
}

impl<V: Visitor> Walker<'_, V> {
    fn emit(&mut self, mut found: Found) -> Flow {
        if found.rule.safety == Safety::Safe && self.protected.in_hidden_home_dir(&found.path) {
            found.rule = Arc::new(RuleInfo {
                safety: Safety::Review,
                ..RuleInfo::clone(&found.rule)
            });
        }
        self.visitor.artifact(found)
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

    pub fn walk_root(&mut self, root: &Path) {
        let metadata = match fs::metadata(root) {
            Ok(metadata) if metadata.is_dir() => metadata,
            Ok(_) => {
                self.visitor.issue(Issue {
                    path: root.to_path_buf(),
                    kind: IssueKind::InvalidRoot,
                    message: "not a directory".to_owned(),
                });
                return;
            }
            Err(error) => {
                self.visitor
                    .issue(Issue::from_io(root.to_path_buf(), &error));
                return;
            }
        };
        if self.excluded_mount(root) {
            self.visitor.issue(Issue {
                path: root.to_path_buf(),
                kind: IssueKind::SkippedMount,
                message: "root is on an excluded mount or filesystem type".to_owned(),
            });
            return;
        }
        if self.protected.covers(root) {
            self.visitor.issue(Issue {
                path: root.to_path_buf(),
                kind: IssueKind::Protected,
                message: "root holds browser or application data".to_owned(),
            });
            return;
        }
        let mut stack = vec![Pending {
            path: root.to_path_buf(),
            parent: Arc::new(Listing::new(Vec::new())),
            depth: 0,
            device: platform::device(&metadata),
        }];
        while let Some(pending) = stack.pop() {
            if self.visitor.cancelled() {
                return;
            }
            if let Flow::Stop = self.visit(pending, &mut stack) {
                return;
            }
        }
    }

    fn visit(&mut self, pending: Pending, stack: &mut Vec<Pending>) -> Flow {
        self.visitor.entered();
        let entries = match fs::read_dir(&pending.path) {
            Ok(entries) => entries,
            Err(error) => return self.visitor.issue(Issue::from_io(pending.path, &error)),
        };
        let mut children: Vec<(OsString, fs::FileType)> = Vec::new();
        for entry in entries {
            match entry.and_then(|entry| Ok((entry.file_name(), entry.file_type()?))) {
                Ok(child) => children.push(child),
                Err(error) => {
                    if let Flow::Stop = self
                        .visitor
                        .issue(Issue::from_io(pending.path.clone(), &error))
                    {
                        return Flow::Stop;
                    }
                }
            }
        }
        let own = Listing::new(children.iter().map(|(name, _)| name.clone()).collect());

        if self.scan.ignores(&own) || protect::holds_app_data(&own) {
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
            return self.emit(Found {
                path: pending.path,
                kind: Kind::Dir,
                rule,
            });
        }
        if self.scan.max_depth.is_some_and(|max| pending.depth >= max) {
            return Flow::Continue;
        }

        let own = Arc::new(own);
        for (name, file_type) in children {
            if file_type.is_symlink() {
                continue;
            }
            let path = pending.path.join(&name);
            if file_type.is_dir() {
                if name == GIT_DIR {
                    self.visitor.repository(&pending.path);
                }
                if self.scan.skip_dirs.iter().any(|skip| name == skip.as_str())
                    || self.excludes.path(&path)
                    || self.protected.is_known(&path)
                {
                    continue;
                }
                let metadata = match fs::symlink_metadata(&path) {
                    Ok(metadata) => metadata,
                    Err(error) => {
                        if let Flow::Stop = self.visitor.issue(Issue::from_io(path, &error)) {
                            return Flow::Stop;
                        }
                        continue;
                    }
                };
                let device = platform::device(&metadata);
                if device != pending.device
                    && (self.scan.one_file_system || self.excluded_mount(&path))
                {
                    continue;
                }
                if self
                    .scan
                    .excluded_mounts
                    .iter()
                    .any(|m| path.starts_with(m))
                {
                    continue;
                }
                stack.push(Pending {
                    path,
                    parent: Arc::clone(&own),
                    depth: pending.depth + 1,
                    device,
                });
            } else {
                self.visitor.counted(file_type);
                if self.excludes.extension(&name) || self.excludes.path(&path) {
                    continue;
                }
                if let Flow::Stop = self.visitor.file(&path, file_type) {
                    return Flow::Stop;
                }
                if let Some(rule) = self.detector.classify_file(&name, &own)
                    && self.detector.reports(&rule)
                    && let Flow::Stop = self.emit(Found {
                        path,
                        kind: Kind::File,
                        rule,
                    })
                {
                    return Flow::Stop;
                }
            }
        }
        Flow::Continue
    }
}
