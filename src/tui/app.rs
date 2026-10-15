use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::cli::SortKey;
use crate::delete::{Mode, Target};
use crate::detect::{Category, Safety};
use crate::scan::{Candidate, Progress, Summary};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Ready,
    Deleting,
    Deleted,
    Failed(String),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Grouping {
    #[default]
    Rule,
    Project,
}

pub type GroupKey = (Option<Category>, String);

pub fn project_of(candidate: &Candidate) -> &Path {
    candidate
        .project
        .as_deref()
        .or_else(|| candidate.path.parent())
        .unwrap_or(Path::new(""))
}

pub struct Item {
    pub candidate: Candidate,
    pub selected: bool,
    pub status: Status,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Overlay {
    None,
    Confirm { ids: Vec<u64>, mode: Mode },
    Help,
}

pub struct App {
    pub items: Vec<Item>,
    pub view: Vec<usize>,
    pub cursor: usize,
    pub sort: SortKey,
    pub grouping: Grouping,
    pub changes: HashMap<PathBuf, usize>,
    pub filter: String,
    pub filtering: bool,
    pub progress: Progress,
    pub finished: Option<Summary>,
    pub issues: usize,
    pub overlay: Overlay,
    pub mode: Mode,
    pub confirm_before_delete: bool,
    pub freed: u64,
    pub trashed: u64,
    pub free: Option<u64>,
    pub message: Option<String>,
    pub quit: bool,
    dirty: bool,
}

impl App {
    pub fn new(mode: Mode, confirm_before_delete: bool) -> Self {
        Self {
            items: Vec::new(),
            view: Vec::new(),
            cursor: 0,
            sort: SortKey::Size,
            grouping: Grouping::Rule,
            changes: HashMap::new(),
            filter: String::new(),
            filtering: false,
            progress: Progress::default(),
            finished: None,
            issues: 0,
            overlay: Overlay::None,
            mode,
            confirm_before_delete,
            freed: 0,
            trashed: 0,
            free: None,
            message: None,
            quit: false,
            dirty: false,
        }
    }

    pub fn push(&mut self, candidate: Candidate) {
        self.items.push(Item {
            candidate,
            selected: false,
            status: Status::Ready,
        });
        self.dirty = true;
    }

    fn current_id(&self) -> Option<u64> {
        self.view
            .get(self.cursor)
            .map(|&index| self.items[index].candidate.id)
    }

    pub fn current(&self) -> Option<&Item> {
        self.view.get(self.cursor).map(|&index| &self.items[index])
    }

    fn matches(&self, item: &Item) -> bool {
        if self.filter.is_empty() {
            return true;
        }
        let needle = self.filter.to_lowercase();
        item.candidate
            .path
            .to_string_lossy()
            .to_lowercase()
            .contains(&needle)
            || item.candidate.rule.name.contains(&needle)
    }

    pub fn refresh(&mut self) {
        if !self.dirty {
            return;
        }
        let keep = self.current_id();
        let items = &self.items;
        let mut rows: Vec<(usize, GroupKey)> = (0..items.len())
            .filter(|&index| self.matches(&items[index]))
            .map(|index| (index, self.group_key(&items[index].candidate)))
            .collect();
        let mut group_totals: HashMap<GroupKey, u64> = HashMap::new();
        for (index, key) in &rows {
            *group_totals.entry(key.clone()).or_default() +=
                items[*index].candidate.usage.allocated;
        }
        rows.sort_by(|(a, left_group), (b, right_group)| {
            let left = &items[*a];
            let right = &items[*b];
            let group_order = match self.sort {
                SortKey::Size => group_totals
                    .get(right_group)
                    .cmp(&group_totals.get(left_group))
                    .then_with(|| left_group.cmp(right_group)),
                SortKey::Path | SortKey::Category => left_group.cmp(right_group),
            };
            group_order.then_with(|| match self.sort {
                SortKey::Size | SortKey::Category => right
                    .candidate
                    .usage
                    .allocated
                    .cmp(&left.candidate.usage.allocated)
                    .then_with(|| left.candidate.path.cmp(&right.candidate.path)),
                SortKey::Path => left.candidate.path.cmp(&right.candidate.path),
            })
        });
        let view: Vec<usize> = rows.into_iter().map(|(index, _)| index).collect();
        self.view = view;
        self.cursor = keep
            .and_then(|id| {
                self.view
                    .iter()
                    .position(|&index| self.items[index].candidate.id == id)
            })
            .unwrap_or(0)
            .min(self.view.len().saturating_sub(1));
        self.dirty = false;
    }

    pub fn move_by(&mut self, delta: isize) {
        if self.view.is_empty() {
            return;
        }
        let last = self.view.len() - 1;
        self.cursor = self.cursor.saturating_add_signed(delta).min(last);
    }

    pub fn move_to(&mut self, index: usize) {
        self.cursor = index.min(self.view.len().saturating_sub(1));
    }

    pub fn toggle(&mut self) {
        if let Some(&index) = self.view.get(self.cursor) {
            let item = &mut self.items[index];
            if item.status == Status::Ready {
                item.selected = !item.selected;
            }
        }
        self.move_by(1);
    }

    pub fn select_safe(&mut self) {
        for &index in &self.view {
            let item = &mut self.items[index];
            if item.status == Status::Ready && item.candidate.rule.safety == Safety::Safe {
                item.selected = true;
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
        for &index in &self.view {
            let item = &mut self.items[index];
            if item.status == Status::Ready && project_of(&item.candidate) == project {
                item.selected = true;
            }
        }
    }

    pub fn group_key(&self, candidate: &Candidate) -> GroupKey {
        match self.grouping {
            Grouping::Rule => (Some(candidate.rule.category), candidate.rule.name.clone()),
            Grouping::Project => (None, project_of(candidate).to_string_lossy().into_owned()),
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

    pub fn clear_selection(&mut self) {
        for item in &mut self.items {
            item.selected = false;
        }
    }

    pub fn cycle_sort(&mut self) {
        self.sort = match self.sort {
            SortKey::Size => SortKey::Path,
            SortKey::Path => SortKey::Category,
            SortKey::Category => SortKey::Size,
        };
        self.dirty = true;
    }

    pub fn set_filter(&mut self, filter: String) {
        self.filter = filter;
        self.dirty = true;
    }

    pub fn selected(&self) -> impl Iterator<Item = &Item> {
        self.items
            .iter()
            .filter(|item| item.selected && item.status == Status::Ready)
    }

    pub fn selected_bytes(&self) -> u64 {
        self.selected()
            .map(|item| item.candidate.usage.allocated)
            .sum()
    }

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
            mode: self.mode,
        };
    }

    pub fn delete(&mut self) -> Vec<(u64, Target, Mode)> {
        self.open_confirm();
        if self.confirm_before_delete || self.mode == Mode::Permanent {
            return Vec::new();
        }
        self.confirm()
    }

    pub fn delete_selected(&mut self) -> Vec<(u64, Target, Mode)> {
        if self.selected().next().is_none() {
            return Vec::new();
        }
        self.delete()
    }

    pub fn toggle_confirm_mode(&mut self) {
        if let Overlay::Confirm { mode, .. } = &mut self.overlay {
            *mode = match mode {
                Mode::Trash => Mode::Permanent,
                Mode::Permanent | Mode::DryRun => Mode::Trash,
            };
        }
    }

    pub fn item(&self, id: u64) -> Option<&Item> {
        self.items.iter().find(|item| item.candidate.id == id)
    }

    fn item_mut(&mut self, id: u64) -> Option<&mut Item> {
        self.items.iter_mut().find(|item| item.candidate.id == id)
    }

    pub fn confirm(&mut self) -> Vec<(u64, Target, Mode)> {
        let Overlay::Confirm { ids, mode } = std::mem::replace(&mut self.overlay, Overlay::None)
        else {
            return Vec::new();
        };
        let mut jobs = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(item) = self.item_mut(id)
                && item.status == Status::Ready
            {
                item.status = Status::Deleting;
                item.selected = false;
                jobs.push((
                    id,
                    Target {
                        path: item.candidate.path.clone(),
                        identity: item.candidate.identity,
                    },
                    mode,
                ));
            }
        }
        jobs
    }

    pub fn finish(&mut self, id: u64, mode: Mode, result: Result<(), String>) {
        let mut removed = 0;
        if let Some(item) = self.item_mut(id) {
            match result {
                Ok(()) => {
                    item.status = Status::Deleted;
                    removed = item.candidate.usage.allocated;
                }
                Err(error) => item.status = Status::Failed(error),
            }
        }
        match mode {
            Mode::Permanent => self.freed += removed,
            Mode::Trash => self.trashed += removed,
            Mode::DryRun => {}
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
        self.items
            .iter()
            .filter(|item| item.status == Status::Deleting)
            .count()
    }
}

pub fn age(newest: Option<SystemTime>) -> String {
    let Some(days) = newest
        .and_then(|time| SystemTime::now().duration_since(time).ok())
        .map(|elapsed| elapsed.as_secs() / 86_400)
    else {
        return "-".to_owned();
    };
    match days {
        0 => "today".to_owned(),
        1..=13 => format!("{days}d"),
        14..=59 => format!("{}w", days / 7),
        60..=729 => format!("{}mo", days / 30),
        _ => format!("{}y", days / 365),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use super::*;
    use crate::detect::{Category, Kind, RuleInfo};
    use crate::scan::Usage;

    fn candidate(id: u64, path: &str, bytes: u64, safety: Safety) -> Candidate {
        Candidate {
            id,
            path: PathBuf::from(path),
            kind: Kind::Dir,
            rule: Arc::new(RuleInfo {
                name: "node-modules".to_owned(),
                category: Category::Dependencies,
                safety,
                ecosystem: "node".to_owned(),
            }),
            project: None,
            usage: Usage {
                allocated: bytes,
                ..Usage::default()
            },
            newest: None,
            identity: None,
        }
    }

    fn app() -> App {
        app_with(Mode::Trash, true)
    }

    fn app_with(mode: Mode, confirm: bool) -> App {
        let mut app = App::new(mode, confirm);
        app.push(candidate(1, "/p/a/node_modules", 10, Safety::Safe));
        app.push(candidate(2, "/p/b/node_modules", 30, Safety::Safe));
        app.push(candidate(3, "/p/c/dist", 20, Safety::Review));
        app.refresh();
        app
    }

    #[test]
    fn sorts_by_size_and_keeps_cursor_on_item() {
        let mut app = app();
        let order: Vec<u64> = app
            .view
            .iter()
            .map(|&i| app.items[i].candidate.id)
            .collect();
        assert_eq!(order, [2, 3, 1]);
        app.move_by(2);
        app.push(candidate(4, "/p/d/node_modules", 100, Safety::Safe));
        app.refresh();
        assert_eq!(app.current().unwrap().candidate.id, 1);
    }

    #[test]
    fn space_selects_multiple_and_moves_down() {
        let mut app = app();
        app.toggle();
        app.toggle();
        let ids: Vec<u64> = app.selected().map(|item| item.candidate.id).collect();
        assert_eq!(ids, [2, 3]);
        assert_eq!(app.selected_bytes(), 50);
    }

    #[test]
    fn enter_opens_confirm_only_with_a_selection() {
        let mut app = app();
        assert!(app.delete_selected().is_empty());
        assert!(matches!(app.overlay, Overlay::None));
        app.toggle();
        assert!(app.delete_selected().is_empty());
        assert!(matches!(&app.overlay, Overlay::Confirm { ids, .. } if ids == &[2]));
    }

    #[test]
    fn trash_skips_the_dialog_unless_confirmation_is_on() {
        let mut app = app_with(Mode::Trash, false);
        app.toggle();
        let jobs = app.delete_selected();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].2, Mode::Trash);
        assert!(matches!(app.overlay, Overlay::None));
        assert_eq!(app.item(2).unwrap().status, Status::Deleting);
    }

    #[test]
    fn permanent_mode_always_asks() {
        let mut app = app_with(Mode::Permanent, false);
        assert!(app.delete().is_empty());
        assert!(matches!(&app.overlay, Overlay::Confirm { ids, .. } if ids == &[2]));
    }

    #[test]
    fn select_safe_skips_review_items() {
        let mut app = app();
        app.select_safe();
        let mut ids: Vec<u64> = app.selected().map(|item| item.candidate.id).collect();
        ids.sort_unstable();
        assert_eq!(ids, [1, 2]);
    }

    #[test]
    fn delete_without_selection_targets_cursor() {
        let mut app = app();
        app.open_confirm();
        assert_eq!(
            app.overlay,
            Overlay::Confirm {
                ids: vec![2],
                mode: Mode::Trash
            }
        );
        app.toggle_confirm_mode();
        let jobs = app.confirm();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].2, Mode::Permanent);
        assert_eq!(app.item(2).unwrap().status, Status::Deleting);
        app.finish(2, Mode::Permanent, Ok(()));
        assert_eq!(app.freed, 30);
        assert_eq!(app.trashed, 0);
        assert_eq!(app.item(2).unwrap().status, Status::Deleted);
    }

    #[test]
    fn only_permanent_deletion_adds_free_space() {
        let mut app = app();
        assert_eq!(app.free_after(10, Mode::Permanent), None);
        app.free = Some(100);
        assert_eq!(app.free_after(10, Mode::Permanent), Some(110));
        assert_eq!(app.free_after(10, Mode::Trash), Some(100));
        app.open_confirm();
        app.confirm();
        app.finish(2, Mode::Trash, Ok(()));
        assert_eq!(app.trashed, 30);
        assert_eq!(app.freed, 0);
    }

    #[test]
    fn project_grouping_orders_by_project_and_selects_whole_project() {
        let mut app = App::new(Mode::Trash, true);
        let mut cache = candidate(4, "/p/a/.cache", 50, Safety::Review);
        cache.project = Some(PathBuf::from("/p/a"));
        let mut modules = candidate(1, "/p/a/node_modules", 10, Safety::Safe);
        modules.project = Some(PathBuf::from("/p/a"));
        app.push(modules);
        app.push(candidate(2, "/p/b/node_modules", 30, Safety::Safe));
        app.push(cache);
        app.toggle_grouping();
        app.refresh();
        let order: Vec<u64> = app
            .view
            .iter()
            .map(|&index| app.items[index].candidate.id)
            .collect();
        assert_eq!(order, [4, 1, 2]);

        app.move_to(1);
        app.select_project();
        let selected: Vec<u64> = app.selected().map(|item| item.candidate.id).collect();
        assert_eq!(selected, [1, 4]);
        assert_eq!(app.selected_bytes(), 60);
    }

    #[test]
    fn reports_only_nonzero_uncommitted_changes() {
        let mut app = app();
        app.changes.insert(PathBuf::from("/p/a"), 3);
        app.changes.insert(PathBuf::from("/p/b"), 0);
        assert_eq!(app.changes_in(&app.items[0].candidate), Some(3));
        assert_eq!(app.changes_in(&app.items[1].candidate), None);
    }

    #[test]
    fn deleted_items_cannot_be_selected_again() {
        let mut app = app();
        app.open_confirm();
        app.confirm();
        app.finish(2, Mode::Trash, Err("refused".to_owned()));
        assert_eq!(app.trashed, 0);
        app.move_to(0);
        app.toggle();
        assert_eq!(app.selected().count(), 0);
        app.move_to(0);
        app.open_confirm();
        assert_eq!(app.overlay, Overlay::None);
    }

    #[test]
    fn filter_narrows_view() {
        let mut app = app();
        app.set_filter("dist".to_owned());
        app.refresh();
        assert_eq!(app.view.len(), 1);
        app.select_safe();
        assert_eq!(app.selected().count(), 0);
    }
}
