use std::path::PathBuf;

use super::{
    App, Feed, GitDone, GitScope, GitTally, GitTask, GitWork, HistorySpot, IDLE, Load, Overlay,
    RestoreDone, RestoreWork, Spot, Status, Tab, counted, filter::issue_matches,
};
use crate::dupes::Duplicates;
use crate::history::{self, Action as HistoryAction, History, ItemState, TrashSnapshot};
use crate::repos::{Inspected, Repository};
use crate::scan::Issue;
use crate::source::Source;
use crate::units::ByteSize;

impl App {
    pub fn set_history(&mut self, history: History, trash: TrashSnapshot) {
        self.history.history = history;
        self.history.trash = trash;
        self.history.load = Load::Done;
        self.history.stale = false;
        self.history.problem = None;
        self.rebuild_history();
    }

    pub fn history_failed(&mut self, problem: String) {
        self.history.load = Load::Done;
        self.history.stale = false;
        self.history.problem = Some(problem);
    }

    pub fn current_history_spot(&self) -> Option<HistorySpot> {
        self.history.rows.get(self.history.cursor).copied()
    }

    pub fn toggle_history_run(&mut self) {
        let Some(spot) = self.current_history_spot() else {
            return;
        };
        let run_index = match spot {
            HistorySpot::Run(run_index) => run_index,
            HistorySpot::Item(run_index, _) => run_index,
        };
        let Some(run) = self.history.history.runs.get(run_index) else {
            return;
        };
        if run.deleted.is_empty() {
            self.message = Some(format!("{} has no recorded items", run.session()));
            return;
        }
        let id = run.id;
        if !self.history.expanded.remove(&id) {
            self.history.expanded.insert(id);
        }
        self.rebuild_history();
        if !self.history.expanded.contains(&id)
            && let Some(pos) = self
                .history
                .rows
                .iter()
                .position(|&row| row == HistorySpot::Run(run_index))
        {
            self.history.cursor = pos;
        }
    }

    pub fn expand_history_run(&mut self) {
        let Some(HistorySpot::Run(run_index)) = self.current_history_spot() else {
            return;
        };
        let Some(run) = self.history.history.runs.get(run_index) else {
            return;
        };
        if run.deleted.is_empty() {
            return;
        }
        if self.history.expanded.insert(run.id) {
            self.rebuild_history();
        }
    }

    pub fn collapse_history_run(&mut self) {
        let Some(spot) = self.current_history_spot() else {
            return;
        };
        let run_index = match spot {
            HistorySpot::Run(run_index) | HistorySpot::Item(run_index, _) => run_index,
        };
        let Some(run) = self.history.history.runs.get(run_index) else {
            return;
        };
        let changed = self.history.expanded.remove(&run.id);
        if changed {
            self.rebuild_history();
        }
        if let Some(pos) = self
            .history
            .rows
            .iter()
            .position(|&row| row == HistorySpot::Run(run_index))
        {
            self.history.cursor = pos;
        }
    }

    pub fn ask_restore(&mut self) {
        if self.history.restoring {
            self.message = Some("a restore is already running".to_owned());
            return;
        }
        let Some(spot) = self.current_history_spot() else {
            self.message = Some("nothing to restore".to_owned());
            return;
        };
        if !self.history.trash.known {
            self.message = Some(
                "restore needs a trash kuizer can read; on macOS use Put Back in Finder".to_owned(),
            );
            return;
        }
        let history = &self.history.history;
        let trash = &self.history.trash;
        match spot {
            HistorySpot::Run(run_index) => {
                let run = &history.runs[run_index];
                if run.action == HistoryAction::Restore {
                    self.message = Some(format!(
                        "{} is a restore run, not a deletion",
                        run.session()
                    ));
                    return;
                }
                let targets = history::restorable(history, trash, run_index);
                if targets.is_empty() {
                    self.message = Some(format!(
                        "nothing from {} is still in the trash",
                        run.session()
                    ));
                    return;
                }
                let bytes: u64 = targets
                    .iter()
                    .map(|&(r, e)| history.runs[r].deleted[e].bytes)
                    .sum();
                self.overlay = Overlay::Restore {
                    label: run.session(),
                    targets,
                    bytes,
                    confirm: false,
                };
            }
            HistorySpot::Item(run_index, entry_index) => {
                let id = history.item_id(run_index, entry_index);
                let state = history::item_state(history, trash, run_index, entry_index);
                if state != ItemState::InTrash {
                    let reason = match state {
                        ItemState::RestoredHere => "is a restore, not a deletion",
                        ItemState::Restored => "was already restored",
                        ItemState::Deleted => "was deleted permanently",
                        ItemState::DryRun => "was a dry run, nothing was removed",
                        ItemState::LeftTrash => "is no longer in the trash",
                        ItemState::Trashed => "cannot be verified in the trash on this platform",
                        ItemState::InTrash => unreachable!(),
                    };
                    self.message = Some(format!("{id} {reason}"));
                    return;
                }
                let bytes = history.runs[run_index].deleted[entry_index].bytes;
                self.overlay = Overlay::Restore {
                    label: id,
                    targets: vec![(run_index, entry_index)],
                    bytes,
                    confirm: false,
                };
            }
        }
    }

    pub fn toggle_restore_focus(&mut self) {
        if let Overlay::Restore { confirm, .. } = &mut self.overlay {
            *confirm = !*confirm;
        }
    }

    pub fn answer_restore(&mut self) -> Option<RestoreWork> {
        let Overlay::Restore {
            label,
            targets,
            confirm,
            ..
        } = std::mem::replace(&mut self.overlay, Overlay::None)
        else {
            return None;
        };
        if !confirm || targets.is_empty() {
            return None;
        }
        self.history.restoring = true;
        self.message = Some(format!("restoring {label}…"));
        Some(RestoreWork {
            label,
            history: self.history.history.clone(),
            targets,
        })
    }

    pub fn restore_done(&mut self, done: RestoreDone) {
        self.history.restoring = false;
        if let Some((history, trash)) = done.updated {
            self.history.history = history;
            self.history.trash = trash;
            self.history.load = Load::Done;
            self.history.stale = false;
            self.rebuild_history();
        }
        self.message = Some(match done.result {
            Ok(summary) => {
                let mut parts = Vec::new();
                if summary.restored > 0 {
                    parts.push(format!(
                        "restored {} ({}) from {}",
                        counted(summary.restored, "item", "items"),
                        ByteSize(summary.bytes),
                        done.label,
                    ));
                }
                if summary.skipped > 0 {
                    parts.push(format!("{} skipped", summary.skipped));
                }
                if summary.failed > 0 {
                    parts.push(format!("{} failed", summary.failed));
                }
                if let Some(problem) = summary.problem {
                    parts.push(problem);
                }
                if let Some(error) = summary.history_error {
                    parts.push(format!("history: {error}"));
                }
                if parts.is_empty() {
                    format!("nothing restored from {}", done.label)
                } else {
                    parts.join(", ")
                }
            }
            Err(error) => format!("could not restore {}: {error}", done.label),
        });
        if self.overlay == Overlay::Quit && self.unfinished() == 0 {
            self.overlay = Overlay::None;
            self.message = Some(format!("restore finished, {} quits", self.quit_key()));
        }
    }

    pub fn restore_unavailable(&mut self) {
        self.history.restoring = false;
        if self.overlay == Overlay::Quit && self.unfinished() == 0 {
            self.overlay = Overlay::None;
        }
        self.message = Some("restore worker unavailable".to_owned());
    }

    pub fn push_issue(&mut self, issue: Issue) {
        if self.is_excluded(&issue.path) {
            return;
        }
        let needle = self.filter.to_lowercase();
        if issue_matches(&issue, &needle) {
            self.issues.rows.push(self.issues.entries.len());
        }
        self.issues.entries.push(issue);
    }

    pub fn set_duplicates(&mut self, found: Duplicates) {
        self.duplicates.found = found;
        self.prune_duplicates();
        self.duplicates.load = Load::Done;
        self.duplicates.stage = None;
        self.rebuild_duplicates();
    }

    fn prune_duplicates(&mut self) {
        if self.excluded.is_empty() {
            return;
        }
        let excluded = &self.excluded;
        self.duplicates.found.groups.retain_mut(|group| {
            group
                .members
                .retain(|member| !excluded.iter().any(|root| member.path.starts_with(root)));
            group.members.len() >= 2
        });
    }

    pub fn duplicates_failed(&mut self, problem: String) {
        self.duplicates.load = Load::Done;
        self.duplicates.stage = None;
        self.duplicates.problem = Some(problem);
    }

    pub fn loading(&self) -> bool {
        self.feeds.values().any(|feed| feed.load == Load::Loading)
            || self.duplicates.load == Load::Loading
            || self.repos.load == Load::Loading
            || !self.repos.running.is_empty()
            || self.history.load == Load::Loading
            || self.history.restoring
    }

    pub fn feed(&self, source: Source) -> &Feed {
        self.feeds.get(&source).unwrap_or(&IDLE)
    }

    pub fn feed_mut(&mut self, source: Source) -> &mut Feed {
        self.feeds.entry(source).or_default()
    }

    pub fn set_repos(&mut self, inspected: Inspected) {
        let excluded = &self.excluded;
        let list = &mut self.repos;
        list.found = inspected
            .repositories
            .into_iter()
            .filter(|repo| !excluded.iter().any(|root| repo.root.starts_with(root)))
            .collect();
        list.unreadable = inspected.issues.len();
        list.load = Load::Done;
        self.rebuild_repos();
    }

    pub fn current_repo(&self) -> Option<&Repository> {
        let list = &self.repos;
        list.rows.get(list.cursor).map(|&index| &list.found[index])
    }

    fn git_scope(&mut self, all: bool) -> Option<GitScope> {
        if all {
            return Some(GitScope::All);
        }
        let scope = self
            .current_repo()
            .map(|repository| GitScope::One(repository.root.clone()));
        if scope.is_none() {
            self.message = Some("no repository here".to_owned());
        }
        scope
    }

    fn git_ready(&self, task: GitTask, repository: &Repository) -> bool {
        repository.busy.is_none()
            && !self.repos.running.contains(&repository.root)
            && match task {
                GitTask::Gc => repository.collectable(self.min_gain),
                GitTask::Branches => !repository.merged.is_empty(),
            }
    }

    pub fn git_targets(&self, task: GitTask, scope: &GitScope) -> Vec<&Repository> {
        let list = &self.repos;
        let shown = list.rows.iter().map(|&index| &list.found[index]);
        shown
            .filter(|repository| match scope {
                GitScope::One(root) => &repository.root == root,
                GitScope::All => true,
            })
            .filter(|repository| self.git_ready(task, repository))
            .collect()
    }

    fn git_blocked(&self, task: GitTask, scope: &GitScope) -> String {
        let one = match scope {
            GitScope::One(root) => self.repos.found.iter().find(|found| &found.root == root),
            GitScope::All => None,
        };
        match (one, task) {
            (Some(repository), _) if self.repos.running.contains(&repository.root) => {
                "git is still running in this repository".to_owned()
            }
            (Some(repository), _) if repository.busy.is_some() => format!(
                "skipped because {}",
                repository.busy.map_or("", |busy| busy.describe())
            ),
            (Some(_), GitTask::Gc) => "git gc would free less than git.min_gain here".to_owned(),
            (Some(_), GitTask::Branches) => "no merged branches here".to_owned(),
            (None, GitTask::Gc) => "nothing to pack in the repositories shown".to_owned(),
            (None, GitTask::Branches) => "no merged branches in the repositories shown".to_owned(),
        }
    }

    fn start_git(&mut self, task: GitTask, targets: Vec<Repository>) -> GitWork {
        let list = &mut self.repos;
        if list.running.is_empty() {
            list.tally = GitTally::default();
        }
        list.running
            .extend(targets.iter().map(|repository| repository.root.clone()));
        let what = match task {
            GitTask::Gc => "packing",
            GitTask::Branches => "removing merged branches in",
        };
        self.message = Some(format!(
            "{what} {}",
            counted(targets.len(), "repository", "repositories")
        ));
        (task, targets)
    }

    pub fn collect_garbage(&mut self, all: bool) -> Option<GitWork> {
        let scope = self.git_scope(all)?;
        let targets: Vec<Repository> = self
            .git_targets(GitTask::Gc, &scope)
            .into_iter()
            .cloned()
            .collect();
        if targets.is_empty() {
            self.message = Some(self.git_blocked(GitTask::Gc, &scope));
            return None;
        }
        Some(self.start_git(GitTask::Gc, targets))
    }

    pub fn ask_branches(&mut self, all: bool) {
        let Some(scope) = self.git_scope(all) else {
            return;
        };
        if self.git_targets(GitTask::Branches, &scope).is_empty() {
            self.message = Some(self.git_blocked(GitTask::Branches, &scope));
            return;
        }
        self.overlay = Overlay::Branches {
            scope,
            delete: false,
        };
    }

    pub fn toggle_branch_focus(&mut self) {
        if let Overlay::Branches { delete, .. } = &mut self.overlay {
            *delete = !*delete;
        }
    }

    pub fn answer_branches(&mut self) -> Option<GitWork> {
        let Overlay::Branches { scope, delete } =
            std::mem::replace(&mut self.overlay, Overlay::None)
        else {
            return None;
        };
        if !delete {
            return None;
        }
        let targets: Vec<Repository> = self
            .git_targets(GitTask::Branches, &scope)
            .into_iter()
            .cloned()
            .collect();
        if targets.is_empty() {
            self.message = Some(self.git_blocked(GitTask::Branches, &scope));
            return None;
        }
        Some(self.start_git(GitTask::Branches, targets))
    }

    fn current_excludable_path(&self) -> Result<PathBuf, &'static str> {
        match self.tab {
            Tab::Docker => Err("docker objects are not on disk and cannot be excluded"),
            Tab::Artifacts | Tab::Caches | Tab::Toolchains => self
                .current()
                .map(|item| item.candidate.path.clone())
                .ok_or("nothing under the cursor to exclude"),
            Tab::Duplicates => match self.duplicates.rows.get(self.duplicates.cursor) {
                Some(&Spot::Member(group, member)) => Ok(self.duplicates.found.groups[group]
                    .members[member]
                    .path
                    .clone()),
                Some(&Spot::Group(_)) => Err("move to a file to exclude it"),
                None => Err("nothing under the cursor to exclude"),
            },
            Tab::Issues => self
                .issues
                .rows
                .get(self.issues.cursor)
                .map(|&index| self.issues.entries[index].path.clone())
                .ok_or("nothing under the cursor to exclude"),
            Tab::Repos => self
                .current_repo()
                .map(|repo| repo.root.clone())
                .ok_or("nothing under the cursor to exclude"),
            Tab::History => Err("history entries cannot be excluded"),
        }
    }

    pub fn ask_exclude(&mut self) {
        let path = match self.current_excludable_path() {
            Ok(path) => path,
            Err(message) => {
                self.message = Some(message.to_owned());
                return;
            }
        };
        if self.config_path.is_none() {
            self.message = Some(format!(
                "could not exclude {}: this system has no config folder",
                crate::output::display_path(&path)
            ));
            return;
        }
        let pattern = crate::config::exclude_pattern(&path);
        self.overlay = Overlay::Exclude {
            path,
            pattern,
            confirm: false,
        };
    }

    pub fn toggle_exclude_focus(&mut self) {
        if let Overlay::Exclude { confirm, .. } = &mut self.overlay {
            *confirm = !*confirm;
        }
    }

    pub fn answer_exclude(&mut self) -> Option<(PathBuf, String)> {
        let Overlay::Exclude {
            path,
            pattern,
            confirm,
        } = std::mem::replace(&mut self.overlay, Overlay::None)
        else {
            return None;
        };
        confirm.then_some((path, pattern))
    }

    pub fn apply_exclude(&mut self, path: PathBuf) {
        self.excluded.push(path.clone());
        for index in 0..self.items.len() {
            let matches = self.items[index].source != Source::Docker
                && self.items[index].status != Status::Excluded
                && self.items[index].candidate.path.starts_with(&path);
            if !matches {
                continue;
            }
            self.mark(index, false);
            let item = &mut self.items[index];
            let bytes = item.candidate.usage.allocated;
            let before = std::mem::replace(&mut item.status, Status::Excluded);
            let tally = self.tallies.entry(item.source).or_default();
            tally.items = tally.items.saturating_sub(1);
            if before != Status::Deleted {
                tally.bytes = tally.bytes.saturating_sub(bytes);
            }
        }
        self.issues
            .entries
            .retain(|issue| !issue.path.starts_with(&path));
        self.repos
            .found
            .retain(|repo| !repo.root.starts_with(&path));
        self.prune_duplicates();
        self.dirty = true;
    }

    pub fn git_unavailable(&mut self, roots: &[PathBuf]) {
        for root in roots {
            self.repos.running.remove(root);
        }
        self.message = Some("could not run git, nothing was changed".to_owned());
    }

    pub fn git_done(&mut self, done: GitDone) {
        let list = &mut self.repos;
        list.running.remove(&done.root);
        let tally = &mut list.tally;
        match &done.collected {
            Some(Ok(collected)) => {
                tally.packed += 1;
                tally.freed += collected.freed();
            }
            Some(Err(error)) => {
                tally.failed += 1;
                tally.problem = Some(error.to_string());
            }
            None => {}
        }
        for pruned in &done.pruned {
            match &pruned.result {
                Ok(()) => tally.removed += 1,
                Err(error) => {
                    tally.failed += 1;
                    tally.problem = Some(error.to_string());
                }
            }
        }
        let position = list
            .found
            .iter()
            .position(|repository| repository.root == done.root);
        match (position, done.after) {
            (Some(index), Some(after)) if after.worth(self.min_gain) => list.found[index] = after,
            (Some(index), Some(_)) => {
                list.found.remove(index);
            }
            _ => {}
        }
        self.message = Some(self.git_summary());
        self.dirty = true;
        if self.overlay == Overlay::Quit && self.unfinished() == 0 {
            self.overlay = Overlay::None;
            self.message = Some(format!("git finished, {} quits", self.quit_key()));
        }
    }

    fn git_summary(&self) -> String {
        let list = &self.repos;
        let tally = &list.tally;
        let mut parts = Vec::new();
        if tally.packed > 0 {
            parts.push(format!(
                "git gc freed {} in {}",
                ByteSize(tally.freed),
                counted(tally.packed, "repository", "repositories")
            ));
        }
        if tally.removed > 0 {
            parts.push(format!(
                "removed {}",
                counted(tally.removed, "merged branch", "merged branches")
            ));
        }
        if tally.failed > 0 {
            parts.push(format!(
                "{} failed: {}",
                tally.failed,
                tally.problem.as_deref().unwrap_or("unknown error")
            ));
        }
        if !list.running.is_empty() {
            parts.push(format!(
                "still running in {}",
                counted(list.running.len(), "repository", "repositories")
            ));
        }
        parts.join(", ")
    }
}
