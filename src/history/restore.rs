use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{Action, Entry, History, HistoryError, Run, TrashRecord};
use crate::delete::Mode;
use crate::output;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    Restored,
    WouldRestore,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub target: (usize, usize),
    pub id: String,
    pub path: PathBuf,
    pub bytes: u64,
    pub status: Status,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Restore {
    pub outcomes: Vec<Outcome>,
    pub put_back: Vec<Entry>,
}

#[derive(Debug, thiserror::Error)]
pub enum RestoreError {
    #[error("restore needs a trash kuizer can read; on macOS use Put Back in Finder")]
    Unsupported,
    #[error("cannot read the trash: {0}")]
    Trash(String),
}

impl Restore {
    pub fn count(&self, status: Status) -> usize {
        self.outcomes
            .iter()
            .filter(|outcome| outcome.status == status)
            .count()
    }

    pub fn bytes(&self, status: Status) -> u64 {
        self.outcomes
            .iter()
            .filter(|outcome| outcome.status == status)
            .map(|outcome| outcome.bytes)
            .sum()
    }

    /// @name Restore::record
    /// @description Appends one restore run for everything that was put back to the default
    /// history file, so `kuizer history` counts it as restored. Does nothing when nothing was put
    /// back.
    ///
    /// @example
    /// let restore = restore::restore(&history, &targets, false)?;
    /// restore.record()?;
    pub fn record(&self) -> Result<(), HistoryError> {
        if self.put_back.is_empty() {
            return Ok(());
        }
        super::record(Run::restore(super::now(), self.put_back.clone()))
    }
}

#[cfg(any(
    target_os = "windows",
    all(
        unix,
        not(target_os = "macos"),
        not(target_os = "ios"),
        not(target_os = "android")
    )
))]
mod backend {
    use super::RestoreError;

    pub fn list() -> Result<Vec<trash::TrashItem>, RestoreError> {
        trash::os_limited::list().map_err(|error| RestoreError::Trash(error.to_string()))
    }

    pub fn restore(item: trash::TrashItem) -> Result<(), String> {
        trash::os_limited::restore_all([item]).map_err(|error| error.to_string())
    }
}

#[cfg(not(any(
    target_os = "windows",
    all(
        unix,
        not(target_os = "macos"),
        not(target_os = "ios"),
        not(target_os = "android")
    )
)))]
mod backend {
    use super::RestoreError;

    pub fn list() -> Result<Vec<trash::TrashItem>, RestoreError> {
        Err(RestoreError::Unsupported)
    }

    pub fn restore(_item: trash::TrashItem) -> Result<(), String> {
        Err(RestoreError::Unsupported.to_string())
    }
}

/// @name latest_run
/// @description Finds the newest delete run that still has trashed items not restored yet and
/// returns those items, which is what `kuizer restore --last` brings back.
///
/// @example
/// let targets = restore::latest_run(&history, &restored).unwrap_or_default();
pub fn latest_run(history: &History, restored: &HashSet<String>) -> Option<Vec<(usize, usize)>> {
    history
        .runs
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, run)| run.action == Action::Delete)
        .map(|(run_index, run)| {
            (0..run.deleted.len())
                .filter(|&entry| run.deleted[entry].mode == Mode::Trash)
                .filter(|&entry| !restored.contains(&history.item_id(run_index, entry)))
                .map(|entry| (run_index, entry))
                .collect::<Vec<_>>()
        })
        .find(|targets| !targets.is_empty())
}

fn precheck(
    history: &History,
    restored: &HashSet<String>,
    target: (usize, usize),
) -> Option<String> {
    let (run, entry) = target;
    let item = &history.runs[run].deleted[entry];
    if history.runs[run].action == Action::Restore {
        return Some("is a restore, not a deletion".to_owned());
    }
    if restored.contains(&history.item_id(run, entry)) {
        return Some("was already restored".to_owned());
    }
    match item.mode {
        Mode::Permanent => Some("was deleted permanently".to_owned()),
        Mode::DryRun => Some("was a dry run, nothing was removed".to_owned()),
        Mode::Trash => None,
    }
}

fn blocked(path: &Path) -> Option<String> {
    if path.symlink_metadata().is_ok() {
        return Some(format!(
            "{} exists, not overwriting",
            output::display_path(path)
        ));
    }
    path.parent()
        .filter(|parent| !parent.is_dir())
        .map(|parent| format!("{} no longer exists", output::display_path(parent)))
}

/// @name restore_with
/// @description Restores the requested history entries from a trash described by `trash`,
/// calling `put` with the index of each matched trash item. Entries that are not trashed
/// deletions, were already restored, are no longer in the trash or whose path is taken again are
/// skipped with a reason, never overwritten. Outcomes keep the order of `targets`; `put_back`
/// holds the history entries for a restore run.
///
/// @example
/// let restore = restore::restore_with(&history, &targets, false, &records, |index| put(index));
pub fn restore_with(
    history: &History,
    targets: &[(usize, usize)],
    dry_run: bool,
    trash: &[TrashRecord],
    mut put: impl FnMut(usize) -> Result<(), String>,
) -> Restore {
    let restored: HashSet<String> = history
        .restored_ids()
        .into_iter()
        .map(str::to_owned)
        .collect();
    let reasons: Vec<Option<String>> = targets
        .iter()
        .map(|&target| precheck(history, &restored, target))
        .collect();
    let eligible: Vec<(usize, usize)> = targets
        .iter()
        .zip(&reasons)
        .filter(|(_, reason)| reason.is_none())
        .map(|(&target, _)| target)
        .collect();
    let mut matched = super::match_trash(history, &eligible, trash).into_iter();

    let mut restore = Restore::default();
    for (&target, reason) in targets.iter().zip(reasons) {
        let entry = &history.runs[target.0].deleted[target.1];
        let id = history.item_id(target.0, target.1);
        let (status, reason) = match reason {
            Some(reason) => (Status::Skipped, Some(reason)),
            None => match matched.next().flatten() {
                None => (
                    Status::Skipped,
                    Some("is no longer in the trash".to_owned()),
                ),
                Some(index) => match blocked(&entry.path) {
                    Some(reason) => (Status::Skipped, Some(reason)),
                    None if dry_run => (Status::WouldRestore, None),
                    None => match put(index) {
                        Ok(()) => {
                            restore.put_back.push(Entry::restored(entry, id.clone()));
                            (Status::Restored, None)
                        }
                        Err(error) => (Status::Failed, Some(error)),
                    },
                },
            },
        };
        restore.outcomes.push(Outcome {
            target,
            id,
            path: entry.path.clone(),
            bytes: entry.bytes,
            status,
            reason,
        });
    }
    restore
}

/// @name restore
/// @description Restores the requested history entries from the system trash, the same way
/// `kuizer restore` does. Fails only when the trash cannot be read; per item problems are
/// reported in the outcomes. Call `record` on the result to log the restore in the history.
///
/// @example
/// let targets = history.find("s4").unwrap_or_default();
/// let restore = restore::restore(&history, &targets, false)?;
/// restore.record()?;
pub fn restore(
    history: &History,
    targets: &[(usize, usize)],
    dry_run: bool,
) -> Result<Restore, RestoreError> {
    let items = backend::list()?;
    let records: Vec<TrashRecord> = items
        .iter()
        .map(|item| TrashRecord::new(item.original_path(), item.time_deleted))
        .collect();
    Ok(restore_with(history, targets, dry_run, &records, |index| {
        backend::restore(items[index].clone())
    }))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::history::{Cumulative, now};

    fn entry(path: &Path, mode: Mode) -> Entry {
        Entry {
            deleted_at: now(),
            path: path.to_path_buf(),
            rule: "node-modules".to_owned(),
            category: "dependencies".to_owned(),
            mode,
            bytes: 10,
            size: "10 B".to_owned(),
            restores: None,
        }
    }

    fn history(runs: Vec<Run>) -> History {
        let mut runs = runs;
        for (index, run) in runs.iter_mut().enumerate() {
            run.id = index as u64 + 1;
        }
        History {
            next_session: runs.len() as u64 + 1,
            runs,
            cumulative: Cumulative::default(),
        }
    }

    fn in_trash(paths: &[&Path]) -> Vec<TrashRecord> {
        let now = jiff::Zoned::now().datetime();
        paths
            .iter()
            .map(|path| TrashRecord {
                path: path.to_path_buf(),
                deleted_at: Some(now),
            })
            .collect()
    }

    #[test]
    fn restores_every_trashed_entry_and_reports_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let free = dir.path().join("a").join("node_modules");
        let taken = dir.path().join("b").join("node_modules");
        let emptied = dir.path().join("c").join("node_modules");
        let permanent = dir.path().join("d").join("node_modules");
        for path in [&free, &taken, &emptied] {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
        }
        fs::create_dir_all(&taken).unwrap();
        let history = history(vec![Run::new(
            now(),
            &[],
            vec![
                entry(&free, Mode::Trash),
                entry(&taken, Mode::Trash),
                entry(&emptied, Mode::Trash),
                entry(&permanent, Mode::Permanent),
            ],
        )]);
        let targets = history.find("s1").unwrap();
        let mut put = Vec::new();
        let restore = restore_with(
            &history,
            &targets,
            false,
            &in_trash(&[&taken, &free]),
            |index| {
                put.push(index);
                Ok(())
            },
        );

        assert_eq!(put, vec![1]);
        let statuses: Vec<Status> = restore.outcomes.iter().map(|o| o.status).collect();
        assert_eq!(
            statuses,
            vec![
                Status::Restored,
                Status::Skipped,
                Status::Skipped,
                Status::Skipped
            ]
        );
        let reasons: Vec<&str> = restore.outcomes[1..]
            .iter()
            .map(|outcome| outcome.reason.as_deref().unwrap())
            .collect();
        assert!(reasons[0].ends_with("exists, not overwriting"));
        assert_eq!(reasons[1], "is no longer in the trash");
        assert_eq!(reasons[2], "was deleted permanently");
        assert_eq!(restore.count(Status::Restored), 1);
        assert_eq!(restore.bytes(Status::Restored), 10);
        assert_eq!(restore.put_back.len(), 1);
        assert_eq!(restore.put_back[0].restores.as_deref(), Some("s1.1"));
    }

    #[test]
    fn a_failed_put_back_is_reported_and_not_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node_modules");
        let history = history(vec![Run::new(now(), &[], vec![entry(&path, Mode::Trash)])]);
        let restore = restore_with(&history, &[(0, 0)], false, &in_trash(&[&path]), |_| {
            Err("permission denied".to_owned())
        });
        assert_eq!(restore.outcomes[0].status, Status::Failed);
        assert_eq!(
            restore.outcomes[0].reason.as_deref(),
            Some("permission denied")
        );
        assert!(restore.put_back.is_empty());
    }

    #[test]
    fn a_dry_run_and_restored_entries_put_nothing_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node_modules");
        let first = entry(&path, Mode::Trash);
        let back = Entry::restored(&first, "s1.1".to_owned());
        let history = history(vec![
            Run::new(now(), &[], vec![first]),
            Run::restore(now(), vec![back]),
        ]);
        let trash = in_trash(&[&path]);
        let dry = restore_with(&history, &[(0, 0)], true, &trash, |_| unreachable!());
        assert_eq!(dry.outcomes[0].status, Status::Skipped);
        assert_eq!(
            dry.outcomes[0].reason.as_deref(),
            Some("was already restored")
        );
        assert!(dry.put_back.is_empty());

        let fresh = history_with_one(&path);
        let dry = restore_with(&fresh, &[(0, 0)], true, &trash, |_| unreachable!());
        assert_eq!(dry.outcomes[0].status, Status::WouldRestore);
        assert!(dry.put_back.is_empty());
    }

    fn history_with_one(path: &Path) -> History {
        history(vec![Run::new(now(), &[], vec![entry(path, Mode::Trash)])])
    }

    #[test]
    fn the_latest_run_skips_runs_that_were_restored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node_modules");
        let history = history(vec![
            Run::new(now(), &[], vec![entry(&path, Mode::Trash)]),
            Run::new(now(), &[], vec![entry(&path, Mode::Permanent)]),
        ]);
        assert_eq!(latest_run(&history, &HashSet::new()), Some(vec![(0, 0)]));
        let restored = HashSet::from(["s1.1".to_owned()]);
        assert_eq!(latest_run(&history, &restored), None);
    }
}
