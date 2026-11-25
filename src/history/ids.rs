use std::collections::HashSet;

use super::{Action, History};

impl History {
    pub(super) fn assign_ids(&mut self) {
        let mut next = self
            .runs
            .iter()
            .map(|run| run.id)
            .max()
            .unwrap_or(0)
            .max(self.next_session.saturating_sub(1))
            + 1;
        for run in self.runs.iter_mut().filter(|run| run.id == 0) {
            run.id = next;
            next += 1;
        }
        self.next_session = next;
    }

    /// @name find
    /// @description Resolves a session id such as `s42` or an item id such as `s42.3` to
    /// `(run, entry)` indexes. A session id yields every entry of that run.
    ///
    /// @example
    /// let targets = history.find("s42.3")?;
    pub fn find(&self, id: &str) -> Option<Vec<(usize, usize)>> {
        let (session, item) = parse_id(id)?;
        let run_index = self.runs.iter().position(|run| run.id == session)?;
        let run = &self.runs[run_index];
        match item {
            Some(item) => {
                let entry = item
                    .checked_sub(1)
                    .filter(|&index| index < run.deleted.len())?;
                Some(vec![(run_index, entry)])
            }
            None => Some(
                (0..run.deleted.len())
                    .map(|entry| (run_index, entry))
                    .collect(),
            ),
        }
    }

    pub fn item_id(&self, run: usize, entry: usize) -> String {
        format!("s{}.{}", self.runs[run].id, entry + 1)
    }

    /// @name restored_ids
    /// @description Returns the item ids that a later restore run put back.
    ///
    /// @example
    /// let restored = history.restored_ids();
    /// assert!(restored.contains("s4.2"));
    pub fn restored_ids(&self) -> HashSet<&str> {
        self.runs
            .iter()
            .filter(|run| run.action == Action::Restore)
            .flat_map(|run| &run.deleted)
            .filter_map(|entry| entry.restores.as_deref())
            .collect()
    }
}

pub(super) fn parse_id(id: &str) -> Option<(u64, Option<usize>)> {
    let rest = id.strip_prefix('s')?;
    match rest.split_once('.') {
        Some((session, item)) => Some((session.parse().ok()?, Some(item.parse().ok()?))),
        None => Some((rest.parse().ok()?, None)),
    }
}
