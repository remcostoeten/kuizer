use std::collections::{HashMap, HashSet};
use std::io;
use std::path::Path;

use crate::detect::Safety;
use crate::git;
use crate::scan::Candidate;

/// How a frontend has to ask before a selection is removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confirmation {
    Skip,
    YesNo,
    TypeCount,
}

/// What in a selection makes removing it risky.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Risk {
    pub review: usize,
    pub dirty: usize,
    pub unknown: bool,
}

impl Risk {
    /// @name Risk::confirmation
    /// @description Decides how to ask before removing a selection. `review` items always ask
    /// and need the item count typed. Projects with uncommitted changes, or whose state is not
    /// known, always ask. Everything else asks only when `delete.confirm` is on.
    ///
    /// @example
    /// let ask = Risk { review: 1, ..Risk::default() }.confirmation(false);
    /// assert_eq!(ask, Confirmation::TypeCount);
    pub fn confirmation(self, confirm: bool) -> Confirmation {
        if self.review > 0 {
            Confirmation::TypeCount
        } else if confirm || self.dirty > 0 || self.unknown {
            Confirmation::YesNo
        } else {
            Confirmation::Skip
        }
    }
}

/// @name review_items
/// @description Counts the candidates whose rule is marked `review`.
///
/// @example
/// let review = confirm::review_items(&selected);
pub fn review_items(candidates: &[Candidate]) -> usize {
    candidates
        .iter()
        .filter(|candidate| candidate.rule.safety == Safety::Review)
        .count()
}

fn repository_of(project: &Path) -> io::Result<Option<&Path>> {
    for dir in project.ancestors() {
        match dir.join(".git").symlink_metadata() {
            Ok(_) => return Ok(Some(dir)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(None)
}

fn git_is_redirected() -> bool {
    ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE"]
        .iter()
        .any(|name| std::env::var_os(name).is_some())
}

fn has_changes(repository: &Path) -> bool {
    git_is_redirected() || git::uncommitted_changes(repository) != Some(0)
}

/// @name dirty_projects
/// @description Counts the projects of the candidates that sit in a git repository with
/// uncommitted changes, or whose state cannot be read. Each repository is asked once.
///
/// @example
/// let risk = Risk { dirty: confirm::dirty_projects(&selected), ..Risk::default() };
pub fn dirty_projects(candidates: &[Candidate]) -> usize {
    let projects: HashSet<&Path> = candidates
        .iter()
        .filter_map(|candidate| candidate.project.as_deref())
        .collect();
    let mut repositories: HashMap<&Path, bool> = HashMap::new();
    let mut dirty = 0;
    for project in projects {
        let changed = match repository_of(project) {
            Ok(Some(repository)) => *repositories
                .entry(repository)
                .or_insert_with(|| has_changes(repository)),
            Ok(None) => false,
            Err(_) => true,
        };
        dirty += usize::from(changed);
    }
    dirty
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_items_need_the_count_and_risky_projects_ask_even_with_confirm_off() {
        let risk = |review, dirty, unknown| Risk {
            review,
            dirty,
            unknown,
        };
        assert_eq!(risk(0, 0, false).confirmation(false), Confirmation::Skip);
        assert_eq!(risk(0, 0, false).confirmation(true), Confirmation::YesNo);
        assert_eq!(risk(0, 2, false).confirmation(false), Confirmation::YesNo);
        assert_eq!(risk(0, 0, true).confirmation(false), Confirmation::YesNo);
        assert_eq!(
            risk(1, 0, false).confirmation(false),
            Confirmation::TypeCount
        );
        assert_eq!(risk(1, 3, true).confirmation(true), Confirmation::TypeCount);
    }
}
