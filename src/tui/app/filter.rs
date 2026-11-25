use std::borrow::Cow;
use std::cmp::Ordering;
use std::path::Path;

use super::{App, Group, Grouping, HistorySpot, Item, Row, Spot, Status};
use crate::cli::SortKey;
use crate::detect::{Category, Safety};
use crate::history;
use crate::repos::Repository;
use crate::scan::Issue;
use crate::source::Source;

impl App {
    fn matches(&self, item: &Item, needle: &str) -> bool {
        if item.status == Status::Excluded {
            return false;
        }
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

    pub(super) fn compare_items(items: &[Item], by_path: bool, a: u32, b: u32) -> Ordering {
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
        if self.dirty {
            self.rebuild_duplicates();
            self.rebuild_issues();
            self.rebuild_repos();
            self.rebuild_history();
        }
        let keep = self.keep.take().or_else(|| self.current_id());
        self.order_items();
        let needle = self.filter.to_lowercase();
        let items = &self.items;
        let source = self.listed;
        let flat = source != Source::Artifact;
        let by_project = !flat && self.grouping == Grouping::Project;
        let slots = if flat {
            1
        } else if by_project {
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
            if item.source != source || !self.matches(item, &needle) {
                continue;
            }
            let slot = if flat {
                0
            } else if by_project {
                item.project_group as usize
            } else {
                item.rule_group as usize
            };
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
            if !flat {
                rows.push(Row::Group(rank));
            }
            for &index in &buckets[slot] {
                view_row.push(rows.len());
                rows.push(Row::Item(view.len()));
                view_pos[index as usize] = view.len() as u32;
                view.push(index as usize);
                largest = largest.max(items[index as usize].candidate.usage.allocated);
            }
            if !flat {
                groups.push(Group {
                    category: category_of(slot),
                    name: name_of(slot).into_owned(),
                    count: buckets[slot].len(),
                    bytes: bytes[slot],
                    first: first[slot],
                });
            }
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

    pub(super) fn rebuild_duplicates(&mut self) {
        let needle = self.filter.to_lowercase();
        let list = &mut self.duplicates;
        list.rows.clear();
        for (index, group) in list.found.groups.iter().enumerate() {
            let shown = needle.is_empty()
                || group
                    .members
                    .iter()
                    .any(|member| contains(&member.path, &needle));
            if shown {
                list.rows.push(Spot::Group(index));
                list.rows
                    .extend((0..group.members.len()).map(|member| Spot::Member(index, member)));
            }
        }
        list.cursor = list.cursor.min(list.rows.len().saturating_sub(1));
    }

    pub(super) fn rebuild_issues(&mut self) {
        let needle = self.filter.to_lowercase();
        let list = &mut self.issues;
        list.rows = (0..list.entries.len())
            .filter(|&index| issue_matches(&list.entries[index], &needle))
            .collect();
        list.cursor = list.cursor.min(list.rows.len().saturating_sub(1));
    }

    pub(super) fn rebuild_repos(&mut self) {
        let needle = self.filter.to_lowercase();
        let list = &mut self.repos;
        list.rows = (0..list.found.len())
            .filter(|&index| repo_matches(&list.found[index], &needle))
            .collect();
        list.cursor = list.cursor.min(list.rows.len().saturating_sub(1));
    }

    pub(super) fn rebuild_history(&mut self) {
        let needle = self.filter.to_lowercase();
        let list = &mut self.history;
        let current = list.rows.get(list.cursor).copied();
        list.rows.clear();
        for (run_index, run) in list.history.runs.iter().enumerate().rev() {
            let run_hits = needle.is_empty() || history_run_matches(run, &needle);
            let matching_items: Vec<usize> = run
                .deleted
                .iter()
                .enumerate()
                .filter(|(_, entry)| needle.is_empty() || history_entry_matches(entry, &needle))
                .map(|(entry_index, _)| entry_index)
                .collect();
            if !run_hits && matching_items.is_empty() {
                continue;
            }
            list.rows.push(HistorySpot::Run(run_index));
            if !needle.is_empty() {
                if run_hits && !list.expanded.contains(&run.id) {
                    if matching_items.len() < run.deleted.len() {
                        for entry_index in matching_items {
                            list.rows.push(HistorySpot::Item(run_index, entry_index));
                        }
                    }
                } else {
                    for entry_index in matching_items {
                        list.rows.push(HistorySpot::Item(run_index, entry_index));
                    }
                }
            } else if list.expanded.contains(&run.id) {
                for entry_index in 0..run.deleted.len() {
                    list.rows.push(HistorySpot::Item(run_index, entry_index));
                }
            }
        }
        if let Some(spot) = current
            && let Some(pos) = list.rows.iter().position(|&row| row == spot)
        {
            list.cursor = pos;
        } else {
            list.cursor = list.cursor.min(list.rows.len().saturating_sub(1));
        }
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
}

fn contains(path: &Path, needle: &str) -> bool {
    path.to_string_lossy().to_lowercase().contains(needle)
}

fn repo_matches(repository: &Repository, needle: &str) -> bool {
    needle.is_empty()
        || contains(&repository.root, needle)
        || repository
            .merged
            .iter()
            .any(|branch| branch.name.to_lowercase().contains(needle))
}

pub(super) fn issue_matches(issue: &Issue, needle: &str) -> bool {
    needle.is_empty()
        || contains(&issue.path, needle)
        || issue.message.to_lowercase().contains(needle)
}

fn history_run_matches(run: &history::Run, needle: &str) -> bool {
    needle.is_empty()
        || run.session().to_lowercase().contains(needle)
        || run.command.to_lowercase().contains(needle)
        || run.started_at.to_lowercase().contains(needle)
}

fn history_entry_matches(entry: &history::Entry, needle: &str) -> bool {
    needle.is_empty()
        || contains(&entry.path, needle)
        || entry.rule.to_lowercase().contains(needle)
        || entry.category.to_lowercase().contains(needle)
}
