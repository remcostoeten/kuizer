use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::Candidate;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectGrowth {
    pub project: PathBuf,
    pub previous_bytes: u64,
    pub current_bytes: u64,
    pub delta_bytes: i64,
    pub added_items: usize,
    pub removed_items: usize,
    pub grown_paths: Vec<(PathBuf, i64)>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ScanDiff {
    pub previous_total: u64,
    pub current_total: u64,
    pub delta_bytes: i64,
    pub projects: Vec<ProjectGrowth>,
}

impl ScanDiff {
    pub fn grown_projects(&self) -> impl Iterator<Item = &ProjectGrowth> {
        self.projects
            .iter()
            .filter(|project| project.delta_bytes > 0)
    }

    pub fn growth_for(&self, project: &Path) -> Option<&ProjectGrowth> {
        self.projects.iter().find(|entry| entry.project == project)
    }
}

/// @name diff_candidates
/// @description Compares a previous scan's candidates against the current scan's candidates and
/// summarizes per-project byte changes, sorted by largest growth first.
///
/// @example
/// let diff = scan::diff_candidates(&previous, &current);
/// for grown in diff.grown_projects() {
///     println!("{}: +{}", grown.project.display(), grown.delta_bytes);
/// }
pub fn diff_candidates(previous: &[Candidate], current: &[Candidate]) -> ScanDiff {
    let prev = previous.iter().map(|c| {
        (
            c.path.as_path(),
            c.project.as_deref().unwrap_or(c.path.as_path()),
            c.usage.allocated,
        )
    });
    let curr = current.iter().map(|c| {
        (
            c.path.as_path(),
            c.project.as_deref().unwrap_or(c.path.as_path()),
            c.usage.allocated,
        )
    });
    diff_entries(prev, curr)
}

/// @name diff_entries
/// @description Compares two iterators of `(item_path, project_path, allocated_bytes)` entries and
/// produces a [`ScanDiff`] with per-project totals, net deltas, and which individual items grew.
///
/// @example
/// let diff = scan::diff_entries(prev_rows, curr_rows);
pub fn diff_entries<'a>(
    previous: impl IntoIterator<Item = (&'a Path, &'a Path, u64)>,
    current: impl IntoIterator<Item = (&'a Path, &'a Path, u64)>,
) -> ScanDiff {
    #[derive(Default)]
    struct ProjectItems {
        prev: HashMap<PathBuf, u64>,
        curr: HashMap<PathBuf, u64>,
    }

    let mut by_project: BTreeMap<PathBuf, ProjectItems> = BTreeMap::new();
    let mut previous_total = 0u64;
    for (path, project, bytes) in previous {
        previous_total = previous_total.saturating_add(bytes);
        *by_project
            .entry(project.to_path_buf())
            .or_default()
            .prev
            .entry(path.to_path_buf())
            .or_default() += bytes;
    }

    let mut current_total = 0u64;
    for (path, project, bytes) in current {
        current_total = current_total.saturating_add(bytes);
        *by_project
            .entry(project.to_path_buf())
            .or_default()
            .curr
            .entry(path.to_path_buf())
            .or_default() += bytes;
    }

    let mut projects = Vec::new();
    for (project, items) in by_project {
        let prev_sum: u64 = items.prev.values().copied().sum();
        let curr_sum: u64 = items.curr.values().copied().sum();
        let delta_bytes = saturating_delta(prev_sum, curr_sum);

        let mut added_items = 0usize;
        let mut removed_items = 0usize;
        let mut grown_paths = Vec::new();

        for (path, &curr_bytes) in &items.curr {
            match items.prev.get(path) {
                None => {
                    added_items += 1;
                    if curr_bytes > 0 {
                        grown_paths.push((path.clone(), saturating_delta(0, curr_bytes)));
                    }
                }
                Some(&prev_bytes) => {
                    let item_delta = saturating_delta(prev_bytes, curr_bytes);
                    if item_delta > 0 {
                        grown_paths.push((path.clone(), item_delta));
                    }
                }
            }
        }
        for path in items.prev.keys() {
            if !items.curr.contains_key(path) {
                removed_items += 1;
            }
        }

        grown_paths.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

        if delta_bytes != 0 || added_items > 0 || removed_items > 0 {
            projects.push(ProjectGrowth {
                project,
                previous_bytes: prev_sum,
                current_bytes: curr_sum,
                delta_bytes,
                added_items,
                removed_items,
                grown_paths,
            });
        }
    }

    projects.sort_by(|a, b| {
        b.delta_bytes
            .cmp(&a.delta_bytes)
            .then_with(|| a.project.cmp(&b.project))
    });

    ScanDiff {
        previous_total,
        current_total,
        delta_bytes: saturating_delta(previous_total, current_total),
        projects,
    }
}

fn saturating_delta(previous: u64, current: u64) -> i64 {
    if current >= previous {
        i64::try_from(current - previous).unwrap_or(i64::MAX)
    } else {
        let diff = previous - current;
        i64::try_from(diff).map(|value| -value).unwrap_or(i64::MIN)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_project_growth_new_items_and_shrinkage() {
        let prev = vec![
            (
                Path::new("/work/alpha/target"),
                Path::new("/work/alpha"),
                1_000,
            ),
            (
                Path::new("/work/beta/node_modules"),
                Path::new("/work/beta"),
                5_000,
            ),
            (
                Path::new("/work/gamma/dist"),
                Path::new("/work/gamma"),
                2_000,
            ),
        ];
        let curr = vec![
            (
                Path::new("/work/alpha/target"),
                Path::new("/work/alpha"),
                4_500,
            ),
            (
                Path::new("/work/alpha/.next"),
                Path::new("/work/alpha"),
                500,
            ),
            (
                Path::new("/work/beta/node_modules"),
                Path::new("/work/beta"),
                5_000,
            ),
            (
                Path::new("/work/gamma/dist"),
                Path::new("/work/gamma"),
                1_000,
            ),
        ];

        let diff = diff_entries(prev, curr);
        assert_eq!(diff.previous_total, 8_000);
        assert_eq!(diff.current_total, 11_000);
        assert_eq!(diff.delta_bytes, 3_000);

        let grown: Vec<_> = diff.grown_projects().collect();
        assert_eq!(grown.len(), 1);
        assert_eq!(grown[0].project, PathBuf::from("/work/alpha"));
        assert_eq!(grown[0].delta_bytes, 4_000);
        assert_eq!(grown[0].added_items, 1);
        assert_eq!(grown[0].grown_paths.len(), 2);
        assert_eq!(
            grown[0].grown_paths[0],
            (PathBuf::from("/work/alpha/target"), 3_500)
        );
        assert_eq!(
            grown[0].grown_paths[1],
            (PathBuf::from("/work/alpha/.next"), 500)
        );

        let gamma = diff.growth_for(Path::new("/work/gamma")).unwrap();
        assert_eq!(gamma.delta_bytes, -1_000);
        assert!(diff.growth_for(Path::new("/work/beta")).is_none());
    }
}
