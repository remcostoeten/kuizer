use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::Candidate;
use crate::detect::Safety;
use crate::platform::{self, MountTable};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MountSpace {
    pub mount: PathBuf,
    pub available: u64,
    pub total: u64,
    pub reclaimable: u64,
    pub safe_reclaimable: u64,
}

/// @name mount_spaces
/// @description Measures free and total space for every distinct filesystem holding `roots` and
/// attributes each candidate's allocated bytes to its mount so callers can show free space per
/// mount alongside how much the scan would bring back.
///
/// @example
/// let mounts = scan::mount_spaces(&roots, &candidates);
pub fn mount_spaces(roots: &[PathBuf], candidates: &[Candidate]) -> Vec<MountSpace> {
    let table = MountTable::load();
    let mut by_dev: HashMap<u64, MountSpace> = HashMap::new();

    for root in roots {
        let Ok(meta) = fs::metadata(root) else {
            continue;
        };
        let dev = platform::device(&meta);
        if by_dev.contains_key(&dev) {
            continue;
        }
        let Some((available, total)) = platform::filesystem_space(root) else {
            continue;
        };
        let mount = table
            .mount_point(root)
            .map(Path::to_path_buf)
            .unwrap_or_else(|| root.clone());
        by_dev.insert(
            dev,
            MountSpace {
                mount,
                available,
                total,
                reclaimable: 0,
                safe_reclaimable: 0,
            },
        );
    }

    for candidate in candidates {
        let dev = candidate.identity.map(|(dev, _)| dev).or_else(|| {
            fs::symlink_metadata(&candidate.path)
                .ok()
                .map(|m| platform::device(&m))
        });
        let entry = match dev {
            Some(dev) => by_dev.get_mut(&dev),
            None => by_dev
                .values_mut()
                .filter(|m| candidate.path.starts_with(&m.mount))
                .max_by_key(|m| m.mount.components().count()),
        };
        if let Some(space) = entry {
            space.reclaimable = space.reclaimable.saturating_add(candidate.usage.allocated);
            if candidate.rule.safety == Safety::Safe {
                space.safe_reclaimable = space
                    .safe_reclaimable
                    .saturating_add(candidate.usage.allocated);
            }
        }
    }

    let mut mounts: Vec<MountSpace> = by_dev.into_values().collect();
    mounts.sort_by(|a, b| a.mount.cmp(&b.mount));
    mounts
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::detect::{Category, Kind, RuleInfo};
    use crate::scan::Usage;

    fn candidate(path: PathBuf, bytes: u64, safety: Safety) -> Candidate {
        Candidate {
            id: 1,
            path,
            kind: Kind::Dir,
            rule: Arc::new(RuleInfo {
                name: "target".to_owned(),
                category: Category::Build,
                safety,
                ecosystem: "rust".to_owned(),
            }),
            project: None,
            usage: Usage {
                allocated: bytes,
                apparent: bytes,
                entries: 1,
                partial: false,
            },
            newest: None,
            identity: None,
            project_last_active: None,
        }
    }

    #[test]
    fn groups_roots_by_mount_and_accumulates_reclaimable_bytes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();

        let candidates = vec![
            candidate(a.clone(), 1_000, Safety::Safe),
            candidate(b.clone(), 2_500, Safety::Review),
        ];
        let mounts = mount_spaces(&[a, b], &candidates);
        assert_eq!(mounts.len(), 1);
        assert!(mounts[0].total > 0);
        assert_eq!(mounts[0].reclaimable, 3_500);
        assert_eq!(mounts[0].safe_reclaimable, 1_000);
    }
}
