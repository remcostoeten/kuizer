use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::cancel::Cancel;
use super::types::Usage;
use crate::platform;

pub struct Measured {
    pub usage: Usage,
    pub newest: Option<SystemTime>,
    pub identity: Option<(u64, u64)>,
}

fn newer(current: Option<SystemTime>, metadata: &fs::Metadata) -> Option<SystemTime> {
    match (current, metadata.modified().ok()) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

pub fn measure(path: &Path, cancel: &Cancel) -> Measured {
    let mut usage = Usage::default();
    let Ok(root) = fs::symlink_metadata(path) else {
        usage.partial = true;
        return Measured {
            usage,
            newest: None,
            identity: None,
        };
    };
    let device = platform::device(&root);
    let identity = platform::identity(&root);
    let mut newest = newer(None, &root);
    let mut seen: HashSet<(u64, u64)> = HashSet::new();
    usage.allocated = platform::allocated(&root);
    usage.apparent = root.len();
    if !root.is_dir() {
        usage.entries = 1;
        return Measured {
            usage,
            newest,
            identity,
        };
    }

    let mut stack: Vec<PathBuf> = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if cancel.is_cancelled() {
            usage.partial = true;
            break;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            usage.partial = true;
            continue;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                usage.partial = true;
                continue;
            };
            let Ok(metadata) = entry.metadata() else {
                usage.partial = true;
                continue;
            };
            if metadata.is_dir() && platform::device(&metadata) != device {
                usage.partial = true;
                continue;
            }
            usage.entries += 1;
            newest = newer(newest, &metadata);
            if platform::shared_inode(&metadata).is_some_and(|key| !seen.insert(key)) {
                continue;
            }
            usage.allocated += platform::allocated(&metadata);
            usage.apparent += metadata.len();
            if metadata.is_dir() {
                stack.push(entry.path());
            }
        }
    }
    Measured {
        usage,
        newest,
        identity,
    }
}
