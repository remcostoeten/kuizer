use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crossbeam_channel::Sender;

use super::cancel::Cancel;
use super::types::{Counters, Event, Issue, IssueKind};
use crate::config::{Excludes, ScanConfig};
use crate::detect::{Detector, Kind, Listing, Match, RuleInfo, Safety};
use crate::platform::{self, MountTable};
use crate::protect::{self, Protected};

pub struct Found {
    pub path: PathBuf,
    pub kind: Kind,
    pub rule: Match,
}

pub struct Walker<'a> {
    pub scan: &'a ScanConfig,
    pub detector: &'a Detector,
    pub excludes: &'a Excludes,
    pub mounts: &'a MountTable,
    pub protected: &'a Protected,
    pub cancel: &'a Cancel,
    pub counters: &'a Counters,
    pub found: &'a Sender<Found>,
    pub events: &'a Sender<Event>,
}

struct Pending {
    path: PathBuf,
    parent: Arc<Listing>,
    depth: usize,
    device: u64,
}

enum Flow {
    Continue,
    Stop,
}

impl Walker<'_> {
    fn issue(&self, issue: Issue) -> Flow {
        Counters::add(&self.counters.issues, 1);
        match self.events.send(Event::Issue(issue)) {
            Ok(()) => Flow::Continue,
            Err(_) => Flow::Stop,
        }
    }

    fn emit(&self, mut found: Found) -> Flow {
        if found.rule.safety == Safety::Safe && self.protected.in_hidden_home_dir(&found.path) {
            found.rule = Arc::new(RuleInfo {
                safety: Safety::Review,
                ..RuleInfo::clone(&found.rule)
            });
        }
        match self.found.send(found) {
            Ok(()) => Flow::Continue,
            Err(_) => Flow::Stop,
        }
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

    pub fn walk_root(&self, root: &Path) {
        let metadata = match fs::metadata(root) {
            Ok(metadata) if metadata.is_dir() => metadata,
            Ok(_) => {
                self.issue(Issue {
                    path: root.to_path_buf(),
                    kind: IssueKind::InvalidRoot,
                    message: "not a directory".to_owned(),
                });
                return;
            }
            Err(error) => {
                self.issue(Issue::from_io(root.to_path_buf(), &error));
                return;
            }
        };
        if self.excluded_mount(root) {
            self.issue(Issue {
                path: root.to_path_buf(),
                kind: IssueKind::SkippedMount,
                message: "root is on an excluded mount or filesystem type".to_owned(),
            });
            return;
        }
        if self.protected.covers(root) {
            self.issue(Issue {
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
            if self.cancel.is_cancelled() {
                return;
            }
            if let Flow::Stop = self.visit(pending, &mut stack) {
                return;
            }
        }
    }

    fn visit(&self, pending: Pending, stack: &mut Vec<Pending>) -> Flow {
        Counters::add(&self.counters.dirs, 1);
        let entries = match fs::read_dir(&pending.path) {
            Ok(entries) => entries,
            Err(error) => return self.issue(Issue::from_io(pending.path, &error)),
        };
        let mut children: Vec<(OsString, fs::FileType)> = Vec::new();
        for entry in entries {
            match entry.and_then(|entry| Ok((entry.file_name(), entry.file_type()?))) {
                Ok(child) => children.push(child),
                Err(error) => {
                    if let Flow::Stop = self.issue(Issue::from_io(pending.path.clone(), &error)) {
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
                if self.scan.skip_dirs.iter().any(|skip| name == skip.as_str())
                    || self.excludes.path(&path)
                    || self.protected.is_known(&path)
                {
                    continue;
                }
                let metadata = match fs::symlink_metadata(&path) {
                    Ok(metadata) => metadata,
                    Err(error) => {
                        if let Flow::Stop = self.issue(Issue::from_io(path, &error)) {
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
                Counters::add(&self.counters.files, 1);
                if self.excludes.extension(&name) || self.excludes.path(&path) {
                    continue;
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
