use std::collections::HashSet;
use std::time::Instant;

use ratatui::crossterm::event::KeyEvent;

use super::{
    App, ConfirmStats, Item, Job, KEY_HOLD, Overlay, SHOWN, Status, TYPED_DIGITS, project_of,
    repeats,
};
use crate::delete::{Confirmation, Mode, Risk};
use crate::detect::Safety;
use crate::source::Source;

impl App {
    pub fn open_confirm(&mut self) {
        let mut ids: Vec<u64> = self.selected().map(|item| item.candidate.id).collect();
        if ids.is_empty()
            && let Some(item) = self.current()
            && item.status == Status::Ready
        {
            ids.push(item.candidate.id);
        }
        if ids.is_empty() {
            self.message = Some("nothing to delete".to_owned());
            return;
        }
        self.overlay = Overlay::Confirm {
            ids,
            mode: self.listed.mode(self.mode),
        };
        self.typed.clear();
        self.typed_wrong = false;
        self.confirm_stats = self.compute_confirm_stats();
    }

    pub fn confirm_count(&self) -> usize {
        match &self.overlay {
            Overlay::Confirm { ids, .. } => ids.len(),
            _ => 0,
        }
    }

    pub fn type_digit(&mut self, digit: char) {
        if digit.is_ascii_digit() && self.typed.len() < TYPED_DIGITS {
            self.typed.push(digit);
            self.typed_wrong = false;
        }
    }

    pub fn erase_digit(&mut self) {
        self.typed.pop();
        self.typed_wrong = false;
    }

    pub fn submit_count(&mut self) -> Vec<Job> {
        if self.typed.parse::<usize>().ok() == Some(self.confirm_count()) {
            return self.confirm();
        }
        self.typed.clear();
        self.typed_wrong = true;
        Vec::new()
    }

    pub fn close_overlay(&mut self) {
        self.overlay = Overlay::None;
        self.confirm_stats = None;
        self.typed.clear();
        self.typed_wrong = false;
    }

    pub(super) fn compute_confirm_stats(&self) -> Option<ConfirmStats> {
        let Overlay::Confirm { ids, .. } = &self.overlay else {
            return None;
        };
        let mut stats = ConfirmStats::default();
        let mut dirty_seen = HashSet::new();
        let mut checking = HashSet::new();
        for index in ids.iter().filter_map(|id| self.index.get(id).copied()) {
            let item = &self.items[index];
            let candidate = &item.candidate;
            stats.count += 1;
            stats.bytes += candidate.usage.allocated;
            if candidate.rule.safety == Safety::Review {
                stats.review += 1;
            }
            if stats.shown.len() < SHOWN {
                stats.shown.push(index);
            }
            if item.source != Source::Artifact {
                continue;
            }
            let project = project_of(candidate);
            stats.unknown |= self.changes_unknown || self.unknown_projects.contains(project);
            if let Some(changes) = self.changes_in(candidate)
                && dirty_seen.insert(project)
            {
                stats.dirty.push((project.to_path_buf(), changes));
            }
            if self.unchecked.contains(project) {
                checking.insert(project);
            }
        }
        stats.checking = checking.len();
        Some(stats)
    }

    pub fn delete(&mut self) -> Vec<Job> {
        self.open_confirm();
        if self.pending_confirmation() != Confirmation::Skip {
            return Vec::new();
        }
        self.confirm()
    }

    pub fn pending_confirmation(&self) -> Confirmation {
        let Overlay::Confirm { ids, .. } = &self.overlay else {
            return Confirmation::Skip;
        };
        let mut risk = Risk::default();
        for item in ids.iter().filter_map(|&id| self.item(id)) {
            risk.review += usize::from(item.candidate.rule.safety == Safety::Review);
            if item.source == Source::Artifact {
                let project = project_of(&item.candidate);
                risk.dirty += usize::from(self.changes_in(&item.candidate).is_some());
                risk.unknown |= self.changes_unknown
                    || self.unchecked.contains(project)
                    || self.unknown_projects.contains(project);
            }
        }
        risk.confirmation(self.confirm_before_delete)
    }

    pub fn delete_selected(&mut self) -> Vec<Job> {
        if self.selected_count() == 0 {
            return Vec::new();
        }
        self.delete()
    }

    pub fn item(&self, id: u64) -> Option<&Item> {
        self.index.get(&id).map(|&index| &self.items[index])
    }

    pub fn action_key_repeats(&mut self, key: KeyEvent, now: Instant) -> bool {
        let previous = self.last_action_key.replace((key, now));
        previous.is_some_and(|(previous, at)| {
            previous.code == key.code
                && previous.modifiers == key.modifiers
                && now.duration_since(at) < KEY_HOLD
        })
    }

    pub fn enter_repeats(&mut self, now: Instant) -> bool {
        repeats(&mut self.last_enter, now)
    }

    pub fn confirm(&mut self) -> Vec<Job> {
        let Overlay::Confirm { ids, mode } = std::mem::replace(&mut self.overlay, Overlay::None)
        else {
            return Vec::new();
        };
        self.confirm_stats = None;
        let mut jobs = Vec::with_capacity(ids.len());
        for id in ids {
            let Some(&index) = self.index.get(&id) else {
                continue;
            };
            if self.items[index].status != Status::Ready {
                continue;
            }
            self.mark(index, false);
            let item = &mut self.items[index];
            item.status = Status::Deleting;
            self.tallies.entry(item.source).or_default().deleting += 1;
            jobs.push((id, mode));
        }
        jobs
    }

    pub fn finish(&mut self, id: u64, mode: Mode, result: Result<(), String>) {
        let mut removed = 0;
        if let Some(&index) = self.index.get(&id) {
            self.mark(index, false);
            let item = &mut self.items[index];
            let bytes = item.candidate.usage.allocated;
            let now_deleted = result.is_ok();
            let before = std::mem::replace(
                &mut item.status,
                match result {
                    Ok(()) => Status::Deleted,
                    Err(error) => Status::Failed(error),
                },
            );
            let tally = self.tallies.entry(item.source).or_default();
            if before == Status::Deleting {
                tally.deleting -= 1;
            }
            match (before == Status::Deleted, now_deleted) {
                (false, true) => tally.bytes -= bytes,
                (true, false) => tally.bytes += bytes,
                _ => {}
            }
            if before == Status::Deleting && self.unfinished() == 0 && self.overlay == Overlay::Quit
            {
                self.overlay = Overlay::None;
                self.message = Some(format!("deletions finished, {} quits", self.quit_key()));
            }
            if now_deleted {
                removed = bytes;
                if mode != Mode::DryRun {
                    self.history.stale = true;
                }
            }
        }
        match mode {
            Mode::Permanent => self.freed += removed,
            Mode::Trash => self.trashed += removed,
            Mode::DryRun => {}
        }
    }

    pub fn restore_deleting(&mut self, source: Source) {
        let mut restored = 0;
        for index in 0..self.items.len() {
            let item = &mut self.items[index];
            if item.source == source && item.status == Status::Deleting {
                item.status = Status::Ready;
                self.mark(index, true);
                restored += 1;
            }
        }
        self.tallies.entry(source).or_default().deleting = 0;
        if self.overlay == Overlay::Quit && self.unfinished() == 0 {
            self.overlay = Overlay::None;
        }
        if restored > 0 {
            self.message = Some(format!(
                "delete worker unavailable, {restored} item{} kept selected",
                if restored == 1 { "" } else { "s" }
            ));
        }
    }

    pub fn free_after(&self, bytes: u64, mode: Mode) -> Option<u64> {
        let free = self.free?;
        Some(match mode {
            Mode::Permanent => free.saturating_add(bytes),
            Mode::Trash | Mode::DryRun => free,
        })
    }

    pub fn deleting(&self) -> usize {
        self.tallies.values().map(|tally| tally.deleting).sum()
    }

    pub fn unfinished(&self) -> usize {
        self.deleting() + self.repos.running.len() + usize::from(self.history.restoring)
    }

    pub fn deleting_in(&self, source: Source) -> usize {
        self.tallies.get(&source).map_or(0, |tally| tally.deleting)
    }
}
