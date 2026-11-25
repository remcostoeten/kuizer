use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

use super::super::jobs::GitState;
use super::{
    App, Grouping, Item, Load, Overlay, Span, Status, Tab, Tally, project_of, repeats, review_items,
};
use crate::detect::Safety;
use crate::keymap::{Action as Key, Scope};
use crate::scan::Candidate;

impl App {
    pub fn switch(&mut self, tab: Tab) {
        if tab == self.tab {
            return;
        }
        self.end_range();
        self.disarm_widen();
        self.tab = tab;
        if tab == Tab::History && self.history.stale && self.history.load == Load::Done {
            self.history.load = Load::Idle;
        }
        if let Some(source) = tab.source()
            && source != self.listed
        {
            if let Some(id) = self.current_id() {
                self.remembered.insert(self.listed, id);
            }
            self.keep = self.remembered.get(&source).copied();
            self.listed = source;
            self.dirty = true;
        }
    }

    fn cursor_and_len(&mut self) -> (&mut usize, usize) {
        match self.tab {
            Tab::Artifacts | Tab::Caches | Tab::Toolchains | Tab::Docker => {
                (&mut self.cursor, self.view.len())
            }
            Tab::Duplicates => (&mut self.duplicates.cursor, self.duplicates.rows.len()),
            Tab::Issues => (&mut self.issues.cursor, self.issues.rows.len()),
            Tab::Repos => (&mut self.repos.cursor, self.repos.rows.len()),
            Tab::History => (&mut self.history.cursor, self.history.rows.len()),
        }
    }

    pub fn move_by(&mut self, delta: isize) {
        let (cursor, len) = self.cursor_and_len();
        if len == 0 {
            return;
        }
        *cursor = cursor.saturating_add_signed(delta).min(len - 1);
    }

    /// Moves the cursor and grows or shrinks the Shift selection to span the anchor and the
    /// cursor. Only the rows between the old and the new cursor change.
    pub fn extend(&mut self, delta: isize) {
        if self.view.is_empty() {
            return;
        }
        let current = self.cursor;
        let mut range = self
            .range
            .take()
            .filter(|range| self.position(range.anchor).is_some())
            .unwrap_or_else(|| Span {
                anchor: self.items[self.view[current]].candidate.id,
                added: HashSet::new(),
            });
        let anchor = self.position(range.anchor).unwrap_or(current);
        self.move_by(delta);
        let next = self.cursor;
        let spans = |end: usize, row: usize| anchor.min(end) <= row && row <= anchor.max(end);
        for row in current.min(next)..=current.max(next) {
            let index = self.view[row];
            let item = &self.items[index];
            if item.status != Status::Ready {
                continue;
            }
            let id = item.candidate.id;
            if spans(next, row) {
                if !item.selected {
                    self.mark(index, true);
                    range.added.insert(id);
                }
            } else if spans(current, row) && range.added.remove(&id) {
                self.mark(index, false);
            }
        }
        self.range = Some(range);
    }

    /// Ends the Shift selection, so the next Shift motion starts a new one at the cursor.
    pub fn end_range(&mut self) {
        self.range = None;
    }

    pub(super) fn position(&self, id: u64) -> Option<usize> {
        let index = *self.index.get(&id)?;
        self.view_pos
            .get(index)
            .copied()
            .filter(|&position| position != u32::MAX)
            .map(|position| position as usize)
    }

    pub fn move_to(&mut self, index: usize) {
        let (cursor, len) = self.cursor_and_len();
        *cursor = index.min(len.saturating_sub(1));
    }

    pub(super) fn mark(&mut self, index: usize, on: bool) {
        let item = &mut self.items[index];
        if item.selected == on {
            return;
        }
        item.selected = on;
        let bytes = item.candidate.usage.allocated;
        let review = usize::from(item.candidate.rule.safety == Safety::Review);
        let tally = self.tallies.entry(item.source).or_default();
        if on {
            tally.selected += 1;
            tally.selected_bytes += bytes;
            tally.review += review;
        } else {
            tally.selected -= 1;
            tally.selected_bytes -= bytes;
            tally.review -= review;
        }
    }

    pub(super) fn tally(&self) -> Tally {
        self.tallies.get(&self.listed).copied().unwrap_or_default()
    }

    pub fn toggle(&mut self) {
        if let Some(&index) = self.view.get(self.cursor) {
            let item = &self.items[index];
            if item.status == Status::Ready {
                let on = !item.selected;
                self.mark(index, on);
            }
        }
        self.move_by(1);
    }

    pub fn select_safe(&mut self) {
        for position in 0..self.view.len() {
            let index = self.view[position];
            let item = &self.items[index];
            if item.status == Status::Ready && item.candidate.rule.safety == Safety::Safe {
                self.mark(index, true);
            }
        }
    }

    pub fn select_project(&mut self) {
        let Some(project) = self
            .current()
            .map(|item| project_of(&item.candidate).to_path_buf())
        else {
            return;
        };
        let armed = self.widen_armed.take().is_some_and(|path| path == project);
        let members = self.view.iter().copied().filter(|&index| {
            let item = &self.items[index];
            item.status == Status::Ready && project_of(&item.candidate) == project
        });
        let (review, safe): (Vec<usize>, Vec<usize>) =
            members.partition(|&index| self.items[index].candidate.rule.safety == Safety::Review);
        let safe_left = self.unselected(&safe);
        let review_left = self.unselected(&review);
        if safe.is_empty() && review.is_empty() {
            self.message = Some("nothing to select in this project".to_owned());
        } else if safe_left > 0 {
            self.mark_all(&safe, true);
            self.message = Some(if review_left > 0 {
                format!(
                    "selected {} safe, P again adds {}",
                    safe.len(),
                    review_items(review_left)
                )
            } else {
                format!("selected {} safe", safe.len())
            });
        } else if review_left > 0 && safe.is_empty() && !armed {
            self.widen_armed = Some(project);
            self.message = Some(format!(
                "only review items here, P again selects {}",
                review_items(review_left)
            ));
        } else if review_left > 0 {
            self.mark_all(&review, true);
            self.message = Some(format!(
                "selection widened to include {}",
                review_items(review_left)
            ));
        } else {
            self.mark_all(&safe, false);
            self.mark_all(&review, false);
            self.message = Some("project selection cleared".to_owned());
        }
    }

    fn unselected(&self, indices: &[usize]) -> usize {
        indices
            .iter()
            .filter(|&&index| !self.items[index].selected)
            .count()
    }

    fn mark_all(&mut self, indices: &[usize], on: bool) {
        for &index in indices {
            self.mark(index, on);
        }
    }

    pub fn disarm_widen(&mut self) {
        self.widen_armed = None;
    }

    pub fn project_key_repeats(&mut self, now: Instant) -> bool {
        repeats(&mut self.last_project_press, now)
    }

    pub fn selected_review(&self) -> usize {
        self.tally().review
    }

    pub fn back_out(&mut self) {
        if self.tab.source().is_none() {
            if self.filter.is_empty() {
                self.message = Some(format!("nothing to back out of, {} quits", self.quit_key()));
            } else {
                self.set_filter(String::new());
            }
        } else if self.filtered() {
            self.reset_filters();
        } else if self.selected_count() > 0 {
            self.clear_selection();
        } else {
            self.message = Some(format!("nothing to back out of, {} quits", self.quit_key()));
        }
    }

    pub(super) fn quit_key(&self) -> &str {
        self.keymap
            .keys(Scope::Tui, Key::Quit)
            .next()
            .unwrap_or("q")
    }

    pub fn request_quit(&mut self) {
        if self.unfinished() == 0 {
            self.quit = true;
        } else {
            self.overlay = Overlay::Quit;
            self.confirm_stats = None;
        }
    }

    pub fn toggle_grouping(&mut self) {
        self.grouping = match self.grouping {
            Grouping::Rule => Grouping::Project,
            Grouping::Project => Grouping::Rule,
        };
        self.dirty = true;
    }

    pub fn changes_in(&self, candidate: &Candidate) -> Option<usize> {
        self.changes
            .get(project_of(candidate))
            .copied()
            .filter(|&count| count > 0)
    }

    pub fn expect_status(&mut self, project: PathBuf) {
        if !self.changes_unknown
            && !self.changes.contains_key(&project)
            && !self.unknown_projects.contains(&project)
        {
            self.unchecked.insert(project);
        }
    }

    pub fn set_changes_unknown(&mut self) {
        self.changes_unknown = true;
        self.unchecked.clear();
    }

    pub fn set_statuses(&mut self, updates: impl IntoIterator<Item = (PathBuf, GitState)>) {
        for (project, state) in updates {
            self.unchecked.remove(&project);
            match state {
                GitState::NotRepo => {
                    self.changes.remove(&project);
                    self.unknown_projects.remove(&project);
                }
                GitState::Changes(count) => {
                    self.unknown_projects.remove(&project);
                    self.changes.insert(project, count);
                }
                GitState::Unknown => {
                    self.changes.remove(&project);
                    self.unknown_projects.insert(project);
                }
            }
        }
        if matches!(self.overlay, Overlay::Confirm { .. }) {
            self.confirm_stats = self.compute_confirm_stats();
        }
    }

    pub fn awaiting_status(&self) -> usize {
        self.unchecked.len()
    }

    pub fn clear_selection(&mut self) {
        for index in 0..self.items.len() {
            if self.items[index].source == self.listed {
                self.mark(index, false);
            }
        }
    }

    pub fn selected(&self) -> impl Iterator<Item = &Item> {
        self.items.iter().filter(|item| {
            item.selected && item.status == Status::Ready && item.source == self.listed
        })
    }

    pub fn selected_bytes(&self) -> u64 {
        self.tally().selected_bytes
    }

    pub fn selected_count(&self) -> usize {
        self.tally().selected
    }

    pub fn total_bytes(&self) -> u64 {
        self.tally().bytes
    }

    pub fn total_items(&self) -> usize {
        self.tally().items
    }
}
