use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::cli::SortKey;
use crate::delete::{Mode, Target};
use crate::detect::{Category, Safety};
use crate::scan::{Candidate, Progress, Summary};

const SHOWN: usize = 8;

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
    lower_path: String,
    rule_group: u32,
    project_group: u32,
}

/// A Shift+motion selection: the item it started on and the items it selected that were not
/// selected before, so moving back toward the anchor deselects only those.
struct Span {
    anchor: u64,
    added: HashSet<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Overlay {
    None,
    Confirm { ids: Vec<u64>, mode: Mode },
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Group(usize),
    Item(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub category: Option<Category>,
    pub name: String,
    pub count: usize,
    pub bytes: u64,
    pub first: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfirmStats {
    pub count: usize,
    pub bytes: u64,
    pub review: usize,
    pub shown: Vec<usize>,
    pub dirty: Vec<(PathBuf, usize)>,
    pub checking: usize,
}

type Job = (u64, Target, Mode);

pub struct App {
    pub items: Vec<Item>,
    pub view: Vec<usize>,
    pub rows: Vec<Row>,
    pub groups: Vec<Group>,
    pub view_row: Vec<usize>,
    pub largest: u64,
    pub cursor: usize,
    pub sort: SortKey,
    pub grouping: Grouping,
    pub changes: HashMap<PathBuf, usize>,
    pub filter: String,
    pub filtering: bool,
    pub category: Option<Category>,
    pub safe_only: bool,
    pub progress: Progress,
    pub current: Option<PathBuf>,
    pub finished: Option<Summary>,
    pub issues: usize,
    pub overlay: Overlay,
    pub confirm_stats: Option<ConfirmStats>,
    pub mode: Mode,
    pub confirm_before_delete: bool,
    pub freed: u64,
    pub trashed: u64,
    pub free: Option<u64>,
    pub message: Option<String>,
    pub quit: bool,
    range: Option<Span>,
    index: HashMap<u64, usize>,
    sorted: Vec<u32>,
    sorted_upto: usize,
    sorted_by_path: bool,
    view_pos: Vec<u32>,
    rule_ids: HashMap<Category, HashMap<String, u32>>,
    rule_groups: Vec<(Category, String)>,
    project_ids: HashMap<PathBuf, u32>,
    project_groups: Vec<PathBuf>,
    unchecked: HashSet<PathBuf>,
    total_bytes: u64,
    selected_count: usize,
    selected_bytes: u64,
    deleting_count: usize,
    dirty: bool,
    grown: bool,
}

impl App {
    pub fn new(mode: Mode, confirm_before_delete: bool) -> Self {
        Self {
            items: Vec::new(),
            view: Vec::new(),
            rows: Vec::new(),
            groups: Vec::new(),
            view_row: Vec::new(),
            largest: 0,
            cursor: 0,
            sort: SortKey::Size,
            grouping: Grouping::Rule,
            changes: HashMap::new(),
            filter: String::new(),
            filtering: false,
            category: None,
            safe_only: false,
            progress: Progress::default(),
            current: None,
            finished: None,
            issues: 0,
            overlay: Overlay::None,
            confirm_stats: None,
            mode,
            confirm_before_delete,
            freed: 0,
            trashed: 0,
            free: None,
            message: None,
            quit: false,
            range: None,
            index: HashMap::new(),
            sorted: Vec::new(),
            sorted_upto: 0,
            sorted_by_path: false,
            view_pos: Vec::new(),
            rule_ids: HashMap::new(),
            rule_groups: Vec::new(),
            project_ids: HashMap::new(),
            project_groups: Vec::new(),
            unchecked: HashSet::new(),
            total_bytes: 0,
            selected_count: 0,
            selected_bytes: 0,
            deleting_count: 0,
            dirty: false,
            grown: false,
        }
    }

    pub fn push(&mut self, candidate: Candidate) {
        self.index.insert(candidate.id, self.items.len());
        self.total_bytes += candidate.usage.allocated;
        let lower_path = candidate.path.to_string_lossy().to_lowercase();
        let rule_group = self.rule_group_of(&candidate);
        let project_group = self.project_group_of(&candidate);
        self.items.push(Item {
            candidate,
            selected: false,
            status: Status::Ready,
            lower_path,
            rule_group,
            project_group,
        });
        self.grown = true;
    }

    fn rule_group_of(&mut self, candidate: &Candidate) -> u32 {
        let rule = &candidate.rule;
        let by_name = self.rule_ids.entry(rule.category).or_default();
        if let Some(&id) = by_name.get(rule.name.as_str()) {
            return id;
        }
        let id = self.rule_groups.len() as u32;
        by_name.insert(rule.name.clone(), id);
        self.rule_groups.push((rule.category, rule.name.clone()));
        id
    }

    fn project_group_of(&mut self, candidate: &Candidate) -> u32 {
        let project = project_of(candidate);
        if let Some(&id) = self.project_ids.get(project) {
            return id;
        }
        let id = self.project_groups.len() as u32;
        self.project_ids.insert(project.to_path_buf(), id);
        self.project_groups.push(project.to_path_buf());
        id
    }

    pub fn knows_project(&self, project: &Path) -> bool {
        self.project_ids.contains_key(project)
    }

    fn current_id(&self) -> Option<u64> {
        self.view
            .get(self.cursor)
            .map(|&index| self.items[index].candidate.id)
    }

    pub fn current(&self) -> Option<&Item> {
        self.view.get(self.cursor).map(|&index| &self.items[index])
    }

    fn matches(&self, item: &Item, needle: &str) -> bool {
        if self
            .category
            .is_some_and(|only| item.candidate.rule.category != only)
        {
            return false;
        }
        if self.safe_only && item.candidate.rule.safety != Safety::Safe {
            return false;
        }
        needle.is_empty()
            || item.lower_path.contains(needle)
            || item.candidate.rule.name.contains(needle)
    }

    pub fn changed(&self) -> bool {
        self.dirty
    }

    pub fn arrivals(&self) -> bool {
        self.grown
    }

    fn compare_items(items: &[Item], by_path: bool, a: u32, b: u32) -> Ordering {
        let left = &items[a as usize].candidate;
        let right = &items[b as usize].candidate;
        let order = if by_path {
            left.path.cmp(&right.path)
        } else {
            right
                .usage
                .allocated
                .cmp(&left.usage.allocated)
                .then_with(|| left.path.cmp(&right.path))
        };
        order.then(a.cmp(&b))
    }

    fn order_items(&mut self) {
        let by_path = self.sort == SortKey::Path;
        let total = self.items.len();
        let items = &self.items;
        let compare = |a: &u32, b: &u32| Self::compare_items(items, by_path, *a, *b);
        if self.sorted_by_path != by_path {
            self.sorted = (0..total as u32).collect();
            self.sorted.sort_unstable_by(compare);
            self.sorted_by_path = by_path;
        } else if self.sorted_upto < total {
            let mut fresh: Vec<u32> = (self.sorted_upto as u32..total as u32).collect();
            fresh.sort_unstable_by(compare);
            let mut merged = Vec::with_capacity(total);
            let (mut old, mut new) = (self.sorted.iter().peekable(), fresh.iter().peekable());
            while let (Some(&&a), Some(&&b)) = (old.peek(), new.peek()) {
                if compare(&a, &b) == Ordering::Less {
                    merged.push(a);
                    old.next();
                } else {
                    merged.push(b);
                    new.next();
                }
            }
            merged.extend(old);
            merged.extend(new);
            self.sorted = merged;
        }
        self.sorted_upto = total;
    }

    pub fn refresh(&mut self) {
        if !self.dirty && !self.grown {
            return;
        }
        let keep = self.current_id();
        self.order_items();
        let needle = self.filter.to_lowercase();
        let items = &self.items;
        let by_project = self.grouping == Grouping::Project;
        let slots = if by_project {
            self.project_groups.len()
        } else {
            self.rule_groups.len()
        };
        let mut buckets: Vec<Vec<u32>> = vec![Vec::new(); slots];
        let mut bytes = vec![0u64; slots];
        let mut first = vec![usize::MAX; slots];
        for &index in &self.sorted {
            let position = index as usize;
            let item = &items[position];
            if !self.matches(item, &needle) {
                continue;
            }
            let slot = if by_project {
                item.project_group
            } else {
                item.rule_group
            } as usize;
            buckets[slot].push(index);
            bytes[slot] += item.candidate.usage.allocated;
            first[slot] = first[slot].min(position);
        }
        let category_of = |slot: usize| (!by_project).then(|| self.rule_groups[slot].0);
        let name_of = |slot: usize| -> Cow<'_, str> {
            if by_project {
                self.project_groups[slot].to_string_lossy()
            } else {
                Cow::Borrowed(self.rule_groups[slot].1.as_str())
            }
        };
        let mut order: Vec<usize> = (0..slots)
            .filter(|&slot| !buckets[slot].is_empty())
            .collect();
        order.sort_by(|&a, &b| {
            let by_key = category_of(a)
                .cmp(&category_of(b))
                .then_with(|| name_of(a).cmp(&name_of(b)));
            match self.sort {
                SortKey::Size | SortKey::Idle => bytes[b].cmp(&bytes[a]).then(by_key),
                SortKey::Path | SortKey::Category => by_key,
            }
        });
        let matched: usize = order.iter().map(|&slot| buckets[slot].len()).sum();
        let mut rows = Vec::with_capacity(matched + order.len());
        let mut view = Vec::with_capacity(matched);
        let mut view_row = Vec::with_capacity(matched);
        let mut view_pos = vec![u32::MAX; items.len()];
        let mut largest = 0;
        let mut groups = Vec::with_capacity(order.len());
        for (rank, &slot) in order.iter().enumerate() {
            rows.push(Row::Group(rank));
            for &index in &buckets[slot] {
                view_row.push(rows.len());
                rows.push(Row::Item(view.len()));
                view_pos[index as usize] = view.len() as u32;
                view.push(index as usize);
                largest = largest.max(items[index as usize].candidate.usage.allocated);
            }
            groups.push(Group {
                category: category_of(slot),
                name: name_of(slot).into_owned(),
                count: buckets[slot].len(),
                bytes: bytes[slot],
                first: first[slot],
            });
        }
        self.cursor = keep
            .and_then(|id| self.index.get(&id))
            .and_then(|&index| view_pos.get(index))
            .filter(|&&position| position != u32::MAX)
            .map_or(0, |&position| position as usize)
            .min(view.len().saturating_sub(1));
        self.view = view;
        self.view_pos = view_pos;
        self.rows = rows;
        self.view_row = view_row;
        self.largest = largest;
        self.groups = groups;
        self.dirty = false;
        self.grown = false;
    }

    pub fn move_by(&mut self, delta: isize) {
        if self.view.is_empty() {
            return;
        }
        let last = self.view.len() - 1;
        self.cursor = self.cursor.saturating_add_signed(delta).min(last);
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

    fn position(&self, id: u64) -> Option<usize> {
        let index = *self.index.get(&id)?;
        self.view_pos
            .get(index)
            .copied()
            .filter(|&position| position != u32::MAX)
            .map(|position| position as usize)
    }

    pub fn move_to(&mut self, index: usize) {
        self.cursor = index.min(self.view.len().saturating_sub(1));
    }

    fn mark(&mut self, index: usize, on: bool) {
        let item = &mut self.items[index];
        if item.selected == on {
            return;
        }
        item.selected = on;
        let bytes = item.candidate.usage.allocated;
        if on {
            self.selected_count += 1;
            self.selected_bytes += bytes;
        } else {
            self.selected_count -= 1;
            self.selected_bytes -= bytes;
        }
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
        for position in 0..self.view.len() {
            let index = self.view[position];
            let item = &self.items[index];
            if item.status == Status::Ready && project_of(&item.candidate) == project {
                self.mark(index, true);
            }
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
        if !self.changes.contains_key(&project) {
            self.unchecked.insert(project);
        }
    }

    pub fn set_statuses(&mut self, updates: impl IntoIterator<Item = (PathBuf, Option<usize>)>) {
        for (project, count) in updates {
            self.unchecked.remove(&project);
            if let Some(count) = count {
                self.changes.insert(project, count);
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
        for item in &mut self.items {
            item.selected = false;
        }
        self.selected_count = 0;
        self.selected_bytes = 0;
    }

    pub fn cycle_sort(&mut self) {
        self.sort = match self.sort {
            SortKey::Size => SortKey::Path,
            SortKey::Path => SortKey::Category,
            SortKey::Category | SortKey::Idle => SortKey::Size,
        };
        self.dirty = true;
    }

    pub fn cycle_category(&mut self) {
        self.category = match self.category {
            None => Some(Category::ALL[0]),
            Some(current) => Category::ALL
                .iter()
                .position(|&category| category == current)
                .and_then(|index| Category::ALL.get(index + 1))
                .copied(),
        };
        self.dirty = true;
    }

    pub fn toggle_safe_only(&mut self) {
        self.safe_only = !self.safe_only;
        self.dirty = true;
    }

    pub fn reset_filters(&mut self) {
        self.category = None;
        self.safe_only = false;
        self.filter.clear();
        self.dirty = true;
    }

    pub fn filtered(&self) -> bool {
        self.category.is_some() || self.safe_only || !self.filter.is_empty()
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
        self.selected_bytes
    }

    pub fn selected_count(&self) -> usize {
        self.selected_count
    }

    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
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
        self.confirm_stats = self.compute_confirm_stats();
    }

    fn compute_confirm_stats(&self) -> Option<ConfirmStats> {
        let Overlay::Confirm { ids, .. } = &self.overlay else {
            return None;
        };
        let mut stats = ConfirmStats::default();
        let mut dirty_seen = HashSet::new();
        let mut checking = HashSet::new();
        for index in ids.iter().filter_map(|id| self.index.get(id).copied()) {
            let candidate = &self.items[index].candidate;
            stats.count += 1;
            stats.bytes += candidate.usage.allocated;
            if candidate.rule.safety == Safety::Review {
                stats.review += 1;
            }
            if stats.shown.len() < SHOWN {
                stats.shown.push(index);
            }
            let project = project_of(candidate);
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
        if self.confirm_before_delete || self.mode == Mode::Permanent || self.pending_needs_review()
        {
            return Vec::new();
        }
        self.confirm()
    }

    fn pending_needs_review(&self) -> bool {
        let Overlay::Confirm { ids, .. } = &self.overlay else {
            return false;
        };
        ids.iter().filter_map(|&id| self.item(id)).any(|item| {
            item.candidate.rule.safety == Safety::Review
                || self.changes_in(&item.candidate).is_some()
                || self.unchecked.contains(project_of(&item.candidate))
        })
    }

    pub fn delete_selected(&mut self) -> Vec<Job> {
        if self.selected_count == 0 {
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
        self.index.get(&id).map(|&index| &self.items[index])
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
            self.deleting_count += 1;
            jobs.push((
                id,
                Target {
                    path: item.candidate.path.clone(),
                    identity: item.candidate.identity,
                },
                mode,
            ));
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
            if before == Status::Deleting {
                self.deleting_count -= 1;
            }
            match (before == Status::Deleted, now_deleted) {
                (false, true) => self.total_bytes -= bytes,
                (true, false) => self.total_bytes += bytes,
                _ => {}
            }
            if now_deleted {
                removed = bytes;
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
        self.deleting_count
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
            project_last_active: None,
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
    fn shift_motions_select_from_the_anchor_to_the_cursor() {
        let mut app = app();
        app.push(candidate(4, "/p/d/node_modules", 5, Safety::Safe));
        app.refresh();
        app.move_by(1);
        app.toggle();
        app.extend(-1);
        app.extend(-1);
        let ids = |app: &App| -> Vec<u64> {
            app.view
                .iter()
                .map(|&index| &app.items[index])
                .filter(|item| item.selected)
                .map(|item| item.candidate.id)
                .collect()
        };
        assert_eq!(ids(&app), [2, 3, 1]);
        app.extend(1);
        app.extend(1);
        assert_eq!(app.cursor, 2);
        assert_eq!(
            ids(&app),
            [3, 1],
            "moving back keeps only what was selected before"
        );
        app.end_range();
        app.extend(1);
        assert_eq!(ids(&app), [3, 1, 4]);
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
    fn review_items_always_ask() {
        let mut app = app_with(Mode::Trash, false);
        app.move_to(1);
        assert!(app.delete().is_empty());
        assert!(matches!(&app.overlay, Overlay::Confirm { ids, .. } if ids == &[3]));
        assert_eq!(app.item(3).unwrap().status, Status::Ready);
    }

    #[test]
    fn projects_with_uncommitted_changes_always_ask() {
        let mut app = app_with(Mode::Trash, false);
        app.changes.insert(PathBuf::from("/p/a"), 2);
        app.select_safe();
        assert!(app.delete_selected().is_empty());
        assert!(matches!(&app.overlay, Overlay::Confirm { ids, .. } if ids.len() == 2));
        assert_eq!(app.confirm().len(), 2);
    }

    #[test]
    fn a_project_still_being_checked_for_changes_asks() {
        let mut app = app_with(Mode::Trash, false);
        app.expect_status(PathBuf::from("/p/b"));
        app.toggle();
        assert!(app.delete_selected().is_empty());
        assert_eq!(
            app.confirm_stats.as_ref().map(|stats| stats.checking),
            Some(1)
        );
        app.set_statuses([(PathBuf::from("/p/b"), Some(0))]);
        assert_eq!(
            app.confirm_stats.as_ref().map(|stats| stats.checking),
            Some(0)
        );
        app.overlay = Overlay::None;
        assert_eq!(app.delete_selected().len(), 1);
    }

    #[test]
    fn running_counters_match_a_recount() {
        let mut app = app();
        app.push(candidate(4, "/p/d/node_modules", 5, Safety::Safe));
        app.refresh();
        let counters = |app: &App| {
            (
                app.selected_count(),
                app.selected_bytes(),
                app.deleting(),
                app.total_bytes(),
            )
        };
        let recount = |app: &App| {
            let bytes = |item: &Item| item.candidate.usage.allocated;
            (
                app.selected().count(),
                app.selected().map(bytes).sum::<u64>(),
                app.items
                    .iter()
                    .filter(|item| item.status == Status::Deleting)
                    .count(),
                app.items
                    .iter()
                    .filter(|item| item.status != Status::Deleted)
                    .map(bytes)
                    .sum::<u64>(),
            )
        };
        app.select_safe();
        assert_eq!(counters(&app), recount(&app));
        app.move_to(0);
        app.extend(1);
        app.extend(1);
        app.extend(-1);
        app.toggle();
        assert_eq!(counters(&app), recount(&app));
        app.open_confirm();
        app.confirm();
        assert_eq!(counters(&app), recount(&app));
        app.finish(2, Mode::Trash, Ok(()));
        app.finish(1, Mode::Permanent, Err("refused".to_owned()));
        assert_eq!(counters(&app), recount(&app));
        app.select_project();
        app.toggle();
        assert_eq!(counters(&app), recount(&app));
        app.clear_selection();
        assert_eq!(counters(&app), recount(&app));
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
    fn ordering_matches_a_full_sort_after_every_batch_for_every_sort_and_grouping() {
        let rules: Vec<Arc<RuleInfo>> = (0..4)
            .map(|index| {
                Arc::new(RuleInfo {
                    name: format!("rule-{}", index % 3),
                    category: Category::ALL[index % Category::ALL.len()],
                    safety: if index == 0 {
                        Safety::Review
                    } else {
                        Safety::Safe
                    },
                    ecosystem: "x".to_owned(),
                })
            })
            .collect();
        let build = |id: u64| {
            let mut made = candidate(
                id,
                &format!("/p/{}/{id}/out", id * 7 % 13),
                id * 37 % 11 * 100,
                Safety::Safe,
            );
            made.rule = Arc::clone(&rules[id as usize % rules.len()]);
            made.project = Some(PathBuf::from(format!("/p/proj{}", id % 9)));
            made
        };
        let expected = |app: &App| -> Vec<u64> {
            let key = |candidate: &Candidate| match app.grouping {
                Grouping::Rule => (Some(candidate.rule.category), candidate.rule.name.clone()),
                Grouping::Project => (None, project_of(candidate).to_string_lossy().into_owned()),
            };
            let shown: Vec<&Candidate> = app
                .items
                .iter()
                .map(|item| &item.candidate)
                .filter(|candidate| !app.safe_only || candidate.rule.safety == Safety::Safe)
                .collect();
            let mut bytes: HashMap<_, u64> = HashMap::new();
            for candidate in &shown {
                *bytes.entry(key(candidate)).or_default() += candidate.usage.allocated;
            }
            let mut ordered = shown.clone();
            ordered.sort_by(|a, b| {
                let (left, right) = (key(a), key(b));
                let group = match app.sort {
                    SortKey::Size | SortKey::Idle => {
                        bytes[&right].cmp(&bytes[&left]).then(left.cmp(&right))
                    }
                    SortKey::Path | SortKey::Category => left.cmp(&right),
                };
                group.then_with(|| match app.sort {
                    SortKey::Path => a.path.cmp(&b.path),
                    _ => b
                        .usage
                        .allocated
                        .cmp(&a.usage.allocated)
                        .then_with(|| a.path.cmp(&b.path)),
                })
            });
            ordered.iter().map(|candidate| candidate.id).collect()
        };
        let mut app = App::new(Mode::Trash, true);
        for batch in [1..41u64, 41..61, 61..64] {
            for id in batch {
                app.push(build(id));
            }
            for safe_only in [false, true] {
                app.safe_only = safe_only;
                for sort in [
                    SortKey::Size,
                    SortKey::Path,
                    SortKey::Category,
                    SortKey::Size,
                ] {
                    for grouping in [Grouping::Rule, Grouping::Project] {
                        app.sort = sort;
                        app.grouping = grouping;
                        app.dirty = true;
                        app.refresh();
                        let order: Vec<u64> = app
                            .view
                            .iter()
                            .map(|&index| app.items[index].candidate.id)
                            .collect();
                        assert_eq!(
                            order,
                            expected(&app),
                            "{sort:?} {grouping:?} safe={safe_only}"
                        );
                        let positions: Vec<Option<usize>> =
                            order.iter().map(|&id| app.position(id)).collect();
                        assert_eq!(positions, (0..order.len()).map(Some).collect::<Vec<_>>());
                    }
                }
            }
        }
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
    fn category_and_safe_filters_narrow_view() {
        let mut app = app();
        app.toggle_safe_only();
        app.refresh();
        assert_eq!(app.view.len(), 2);
        app.cycle_category();
        app.refresh();
        assert_eq!(app.view.len(), 2);
        app.cycle_category();
        app.refresh();
        assert_eq!(app.view.len(), 0);
        app.reset_filters();
        app.refresh();
        assert_eq!(app.view.len(), 3);
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
