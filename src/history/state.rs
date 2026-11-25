use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::restore::{RestoreError, backend};
use super::{Action, History, Total, TrashRecord, parse_id};
use crate::delete::Mode;

/// @name Restored
/// @description The delete entries that a later restore run put back, as `(run, entry)` indexes
/// into the history it was built from. Built once so callers stop re-reading the restore runs for
/// every entry they ask about.
///
/// @example
/// let restored = history::Restored::of(&history);
/// let back = restored.contains((run, entry));
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Restored(HashSet<(usize, usize)>);

impl Restored {
    pub fn of(history: &History) -> Self {
        let runs: HashMap<u64, usize> = history
            .runs
            .iter()
            .enumerate()
            .filter(|(_, run)| run.action == Action::Delete)
            .map(|(index, run)| (run.id, index))
            .collect();
        Self(
            history
                .restored_ids()
                .into_iter()
                .filter_map(|id| {
                    let (session, item) = parse_id(id)?;
                    let run = *runs.get(&session)?;
                    let entry = item?.checked_sub(1)?;
                    (entry < history.runs[run].deleted.len()).then_some((run, entry))
                })
                .collect(),
        )
    }

    pub fn contains(&self, target: (usize, usize)) -> bool {
        self.0.contains(&target)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemState {
    RestoredHere,
    Restored,
    Deleted,
    DryRun,
    Trashed,
    InTrash,
    LeftTrash,
}

impl ItemState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RestoredHere => "restored here",
            Self::Restored => "restored",
            Self::Deleted => "deleted",
            Self::DryRun => "dry run",
            Self::Trashed => "trashed",
            Self::InTrash => "in trash",
            Self::LeftTrash => "left trash",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrashState {
    Restorable { items: usize, bytes: u64 },
    Restored,
    Emptied,
    Unknown,
    NotTrashed,
}

/// @name TrashSnapshot
/// @description One history held against the system trash at one moment: which entries a
/// restore put back, which trashed entries are still in the trash, and whether the trash could be
/// read at all. Indexes refer to the history it was built from.
///
/// @example
/// let trash = TrashSnapshot::read(&history);
/// let state = trash.state(&history, run);
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrashSnapshot {
    pub restored: Restored,
    pub kept: HashSet<(usize, usize)>,
    pub known: bool,
}

impl TrashSnapshot {
    /// @name TrashSnapshot::read
    /// @description Lists the system trash and matches it against `history`. Where the trash
    /// cannot be read the snapshot says so and nothing counts as still in the trash.
    ///
    /// @example
    /// let trash = TrashSnapshot::read(&history);
    pub fn read(history: &History) -> Self {
        match TrashView::list() {
            Ok(view) => view.snapshot(history),
            Err(_) => Self::new(history, None),
        }
    }

    /// @name TrashSnapshot::new
    /// @description Builds a snapshot from `kept`, the entries `still_in_trash` found, or `None`
    /// when the trash cannot be read.
    ///
    /// @example
    /// let trash = TrashSnapshot::new(&history, saved.trash_known.then_some(saved.kept));
    pub fn new(history: &History, kept: Option<HashSet<(usize, usize)>>) -> Self {
        Self {
            restored: Restored::of(history),
            known: kept.is_some(),
            kept: kept.unwrap_or_default(),
        }
    }

    pub fn kept(&self) -> Option<&HashSet<(usize, usize)>> {
        self.known.then_some(&self.kept)
    }

    /// @name TrashSnapshot::state
    /// @description Says whether one run can still be restored: what is left in the trash, all of
    /// it restored, gone from the trash, unknown because the trash cannot be read, or nothing
    /// trashed at all. Restore runs and unknown runs are `NotTrashed`.
    ///
    /// @example
    /// if let TrashState::Restorable { items, bytes } = trash.state(&history, run) { .. }
    pub fn state(&self, history: &History, run: usize) -> TrashState {
        let Some(found) = history.runs.get(run) else {
            return TrashState::NotTrashed;
        };
        if found.action == Action::Restore {
            return TrashState::NotTrashed;
        }
        let mut trashed = false;
        let mut all_restored = true;
        let (mut items, mut bytes) = (0, 0);
        for (entry, item) in found.deleted.iter().enumerate() {
            match item_state(history, self, run, entry) {
                ItemState::Deleted | ItemState::DryRun => continue,
                ItemState::InTrash => {
                    items += 1;
                    bytes += item.bytes;
                }
                ItemState::Restored => {}
                _ => all_restored = false,
            }
            trashed = true;
        }
        if !trashed {
            TrashState::NotTrashed
        } else if !self.known {
            TrashState::Unknown
        } else if items > 0 {
            TrashState::Restorable { items, bytes }
        } else if all_restored {
            TrashState::Restored
        } else {
            TrashState::Emptied
        }
    }

    pub fn states(&self, history: &History) -> Vec<TrashState> {
        (0..history.runs.len())
            .map(|run| self.state(history, run))
            .collect()
    }

    /// @name TrashSnapshot::in_trash
    /// @description Adds up the entries that are still in the trash and were not restored.
    ///
    /// @example
    /// let waiting = trash.in_trash(&history).bytes;
    pub fn in_trash(&self, history: &History) -> Total {
        Total::sum(
            self.kept
                .iter()
                .filter(|&&target| !self.restored.contains(target))
                .filter_map(|&(run, entry)| history.runs.get(run)?.deleted.get(entry)),
        )
    }
}

/// @name TrashView
/// @description One listing of the system trash, kept so a frontend can work out what is still
/// in the trash and restore from it without listing the trash again. `restore::restore_listed`
/// drops what it puts back, so the view stays current after a restore.
///
/// @example
/// let mut view = TrashView::list()?;
/// let trash = view.snapshot(&history);
/// let restore = restore::restore_listed(&history, &targets, false, &mut view);
pub struct TrashView {
    pub(super) items: Vec<trash::TrashItem>,
    pub(super) records: Vec<TrashRecord>,
}

impl TrashView {
    pub fn list() -> Result<Self, RestoreError> {
        let items = backend::list()?;
        let records = items
            .iter()
            .map(|item| TrashRecord::new(item.original_path(), item.time_deleted))
            .collect();
        Ok(Self { items, records })
    }

    pub fn records(&self) -> &[TrashRecord] {
        &self.records
    }

    pub fn snapshot(&self, history: &History) -> TrashSnapshot {
        let paths = self.records.iter().map(|record| record.path.as_path());
        TrashSnapshot::new(history, Some(kept_in(history, paths)))
    }

    pub(super) fn forget(&mut self, taken: &HashSet<usize>) {
        if taken.is_empty() {
            return;
        }
        let (items, records) = std::mem::take(&mut self.items)
            .into_iter()
            .zip(std::mem::take(&mut self.records))
            .enumerate()
            .filter(|(index, _)| !taken.contains(index))
            .map(|(_, pair)| pair)
            .unzip();
        self.items = items;
        self.records = records;
    }
}

/// @name still_in_trash
/// @description Returns the `(run, entry)` indexes of trashed entries whose path is still in the
/// trash. Each trashed path is matched once, newest entries first, so a folder trashed twice and
/// emptied once counts once.
///
/// @example
/// let kept = history::still_in_trash(&history, &history::trashed_paths().unwrap_or_default());
pub fn still_in_trash(history: &History, trashed: &[PathBuf]) -> HashSet<(usize, usize)> {
    kept_in(history, trashed.iter().map(PathBuf::as_path))
}

fn kept_in<'a>(
    history: &History,
    trashed: impl Iterator<Item = &'a Path>,
) -> HashSet<(usize, usize)> {
    let mut remaining: HashMap<&Path, usize> = HashMap::new();
    for path in trashed {
        *remaining.entry(path).or_default() += 1;
    }
    let mut found = HashSet::new();
    for (run_index, run) in history
        .runs
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, run)| run.action == Action::Delete)
    {
        for (entry_index, entry) in run.deleted.iter().enumerate().rev() {
            if entry.mode != Mode::Trash {
                continue;
            }
            if let Some(count) = remaining.get_mut(entry.path.as_path())
                && *count > 0
            {
                *count -= 1;
                found.insert((run_index, entry_index));
            }
        }
    }
    found
}

/// @name freed_bytes
/// @description Sums the space that past runs actually freed: everything deleted permanently, plus
/// trashed entries that are no longer in the trash. Restored entries never count. When the trash
/// cannot be read, trashed entries are left out.
///
/// @example
/// let freed = history::freed_bytes(&history, &TrashSnapshot::read(&history));
pub fn freed_bytes(history: &History, trash: &TrashSnapshot) -> u64 {
    history
        .runs
        .iter()
        .enumerate()
        .filter(|(_, run)| run.action == Action::Delete)
        .flat_map(|(run, found)| {
            found
                .deleted
                .iter()
                .enumerate()
                .map(move |(entry, item)| ((run, entry), item))
        })
        .filter(|&(target, _)| !trash.restored.contains(target))
        .filter(|&(target, item)| match item.mode {
            Mode::Permanent => true,
            Mode::Trash => trash.kept().is_some_and(|kept| !kept.contains(&target)),
            Mode::DryRun => false,
        })
        .map(|(_, item)| item.bytes)
        .sum()
}

/// @name item_state
/// @description Says what became of one history entry: restored, deleted permanently, a dry run,
/// still in the trash or gone from it. When the trash cannot be read, trashed entries are just
/// `Trashed`.
///
/// @example
/// let state = history::item_state(&history, &trash, 0, 2);
pub fn item_state(history: &History, trash: &TrashSnapshot, run: usize, entry: usize) -> ItemState {
    let item = &history.runs[run].deleted[entry];
    if history.runs[run].action == Action::Restore {
        return ItemState::RestoredHere;
    }
    if trash.restored.contains((run, entry)) {
        return ItemState::Restored;
    }
    match (item.mode, trash.kept()) {
        (Mode::Permanent, _) => ItemState::Deleted,
        (Mode::DryRun, _) => ItemState::DryRun,
        (Mode::Trash, None) => ItemState::Trashed,
        (Mode::Trash, Some(kept)) if kept.contains(&(run, entry)) => ItemState::InTrash,
        (Mode::Trash, Some(_)) => ItemState::LeftTrash,
    }
}

/// @name restorable
/// @description Lists the entries of one run that can still be restored: trashed, not restored
/// yet and still in the trash.
///
/// @example
/// let targets = history::restorable(&history, &trash, run_index);
/// let restore = history::restore::restore(&history, &targets, false)?;
pub fn restorable(history: &History, trash: &TrashSnapshot, run: usize) -> Vec<(usize, usize)> {
    let entries = history.runs.get(run).map_or(0, |found| found.deleted.len());
    (0..entries)
        .filter(|&entry| item_state(history, trash, run, entry) == ItemState::InTrash)
        .map(|entry| (run, entry))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::{Cumulative, Entry, Run, now};

    fn entry(path: &str, mode: Mode, bytes: u64) -> Entry {
        Entry {
            deleted_at: now(),
            path: PathBuf::from(path),
            rule: "node-modules".to_owned(),
            category: "dependencies".to_owned(),
            mode,
            bytes,
            size: String::new(),
            restores: None,
        }
    }

    fn history(mut runs: Vec<Run>) -> History {
        for (index, run) in runs.iter_mut().enumerate() {
            run.id = index as u64 + 1;
        }
        History {
            next_session: runs.len() as u64 + 1,
            runs,
            cumulative: Cumulative::default(),
        }
    }

    fn sample() -> History {
        let run = |entries| Run::new(now(), &[], entries);
        let mut runs = vec![
            run(vec![
                entry("/p/a", Mode::Trash, 10),
                entry("/p/b", Mode::Trash, 20),
                entry("/p/c", Mode::Trash, 40),
                entry("/p/d", Mode::Permanent, 1000),
            ]),
            run(vec![entry("/p/e", Mode::Trash, 10)]),
            run(vec![entry("/p/f", Mode::Trash, 10)]),
            run(vec![entry("/p/g", Mode::DryRun, 10)]),
        ];
        let back = vec![
            Entry::restored(&runs[0].deleted[1], "s1.2".to_owned()),
            Entry::restored(&runs[1].deleted[0], "s2.1".to_owned()),
        ];
        runs.push(Run::restore(now(), back));
        history(runs)
    }

    #[test]
    fn each_entry_and_run_says_what_is_left_in_the_trash() {
        let history = sample();
        let trash = TrashSnapshot::new(&history, Some(HashSet::from([(0, 0)])));

        let states: Vec<ItemState> = (0..4)
            .map(|entry| item_state(&history, &trash, 0, entry))
            .collect();
        assert_eq!(
            states,
            [
                ItemState::InTrash,
                ItemState::Restored,
                ItemState::LeftTrash,
                ItemState::Deleted
            ]
        );
        assert_eq!(item_state(&history, &trash, 4, 0), ItemState::RestoredHere);
        assert_eq!(restorable(&history, &trash, 0), [(0, 0)]);
        assert!(restorable(&history, &trash, 1).is_empty());
        assert!(restorable(&history, &trash, 9).is_empty());
        assert_eq!(
            trash.states(&history),
            [
                TrashState::Restorable {
                    items: 1,
                    bytes: 10
                },
                TrashState::Restored,
                TrashState::Emptied,
                TrashState::NotTrashed,
                TrashState::NotTrashed,
            ]
        );
        assert_eq!(trash.in_trash(&history).bytes, 10);
        assert_eq!(freed_bytes(&history, &trash), 1000 + 40 + 10);

        let unknown = TrashSnapshot::new(&history, None);
        assert_eq!(item_state(&history, &unknown, 0, 2), ItemState::Trashed);
        assert_eq!(unknown.state(&history, 0), TrashState::Unknown);
        assert_eq!(unknown.state(&history, 3), TrashState::NotTrashed);
        assert_eq!(freed_bytes(&history, &unknown), 1000);
        assert!(restorable(&history, &unknown, 0).is_empty());
    }

    #[test]
    fn matches_each_trashed_path_once_newest_first() {
        let history = history(vec![
            Run::new(now(), &[], vec![entry("/p/a", Mode::Trash, 1)]),
            Run::new(
                now(),
                &[],
                vec![
                    entry("/p/a", Mode::Trash, 1),
                    entry("/p/a", Mode::Permanent, 1),
                ],
            ),
        ]);
        let kept = still_in_trash(&history, &[PathBuf::from("/p/a")]);
        assert_eq!(kept, HashSet::from([(1, 0)]));
        assert!(still_in_trash(&history, &[]).is_empty());
    }
}
