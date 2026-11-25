use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::jobs::GitState;
use crate::cli::SortKey;
use crate::delete::{Confirmation, Mode, Risk};
use crate::detect::{Category, Safety};
use crate::dupes::{Duplicates, Stage};
use crate::keymap::{Action as Key, Keymap, Scope};
use crate::repos::{Collected, Inspected, Pruned, RepoError, Repository};
use crate::scan::{Candidate, Issue, Progress, Summary};
use crate::source::Source;
use crate::units::ByteSize;
use ratatui::crossterm::event::KeyEvent;

const SHOWN: usize = 8;
const KEY_HOLD: Duration = Duration::from_millis(1200);
const TYPED_DIGITS: usize = 9;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Artifacts,
    Caches,
    Duplicates,
    Issues,
    Toolchains,
    Docker,
    Repos,
}

impl Tab {
    pub const ALL: [Tab; 7] = [
        Self::Artifacts,
        Self::Caches,
        Self::Duplicates,
        Self::Issues,
        Self::Toolchains,
        Self::Docker,
        Self::Repos,
    ];

    pub fn source(self) -> Option<Source> {
        match self {
            Self::Artifacts => Some(Source::Artifact),
            Self::Caches => Some(Source::Cache),
            Self::Toolchains => Some(Source::Toolchain),
            Self::Docker => Some(Source::Docker),
            Self::Duplicates | Self::Issues | Self::Repos => None,
        }
    }

    pub fn action(self) -> Key {
        match self {
            Self::Artifacts => Key::Artifacts,
            Self::Caches => Key::Caches,
            Self::Duplicates => Key::Duplicates,
            Self::Issues => Key::Issues,
            Self::Toolchains => Key::Toolchains,
            Self::Docker => Key::Docker,
            Self::Repos => Key::Repos,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Artifacts => "artifacts",
            Self::Caches => "caches",
            Self::Duplicates => "duplicates",
            Self::Issues => "issues",
            Self::Toolchains => "toolchains",
            Self::Docker => "docker",
            Self::Repos => "git",
        }
    }
}

fn tagged(source: Source, id: u64) -> u64 {
    id | match source {
        Source::Artifact => 0,
        Source::Cache => 1 << 62,
        Source::Toolchain => 1 << 61,
        Source::Docker => 1 << 60,
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Load {
    #[default]
    Idle,
    Loading,
    Done,
}

#[derive(Debug, Default)]
pub struct Feed {
    pub load: Load,
    pub problem: Option<String>,
}

static IDLE: Feed = Feed {
    load: Load::Idle,
    problem: None,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitScope {
    One(PathBuf),
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitTask {
    Gc,
    Branches,
}

pub type GitWork = (GitTask, Vec<Repository>);

#[derive(Debug)]
pub struct GitDone {
    pub root: PathBuf,
    pub collected: Option<Result<Collected, RepoError>>,
    pub pruned: Vec<Pruned>,
    pub after: Option<Repository>,
}

#[derive(Debug, Default)]
struct GitTally {
    packed: usize,
    freed: u64,
    removed: usize,
    failed: usize,
    problem: Option<String>,
}

#[derive(Debug, Default)]
pub struct RepoList {
    pub load: Load,
    pub checking: usize,
    pub found: Vec<Repository>,
    pub unreadable: usize,
    pub problem: Option<String>,
    pub rows: Vec<usize>,
    pub cursor: usize,
    pub running: HashSet<PathBuf>,
    tally: GitTally,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spot {
    Group(usize),
    Member(usize, usize),
}

#[derive(Debug, Default)]
pub struct DuplicateList {
    pub load: Load,
    pub stage: Option<Stage>,
    pub found: Duplicates,
    pub problem: Option<String>,
    pub rows: Vec<Spot>,
    pub cursor: usize,
}

#[derive(Debug, Default)]
pub struct IssueList {
    pub entries: Vec<Issue>,
    pub rows: Vec<usize>,
    pub cursor: usize,
}

#[derive(Debug, Clone, Copy, Default)]
struct Tally {
    items: usize,
    bytes: u64,
    selected: usize,
    selected_bytes: u64,
    review: usize,
    deleting: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Ready,
    Deleting,
    Deleted,
    Excluded,
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
    pub source: Source,
    pub note: String,
    pub label: String,
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
    Confirm {
        ids: Vec<u64>,
        mode: Mode,
    },
    Branches {
        scope: GitScope,
        delete: bool,
    },
    Exclude {
        path: PathBuf,
        pattern: String,
        confirm: bool,
    },
    Quit,
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
    pub unknown: bool,
}

type Job = (u64, Mode);

fn repeats(last: &mut Option<Instant>, now: Instant) -> bool {
    let repeat = last.is_some_and(|at| now.duration_since(at) < KEY_HOLD);
    *last = Some(now);
    repeat
}

fn review_items(count: usize) -> String {
    format!("{count} review item{}", if count == 1 { "" } else { "s" })
}

pub fn counted(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{count} {many}")
    }
}

pub struct App {
    last_enter: Option<Instant>,
    last_action_key: Option<(KeyEvent, Instant)>,
    pub keymap: Keymap,
    last_project_press: Option<Instant>,
    widen_armed: Option<PathBuf>,
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
    pub tab: Tab,
    pub duplicates: DuplicateList,
    pub issues: IssueList,
    pub repos: RepoList,
    pub min_gain: u64,
    pub overlay: Overlay,
    pub confirm_stats: Option<ConfirmStats>,
    pub typed: String,
    pub typed_wrong: bool,
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
    unknown_projects: HashSet<PathBuf>,
    listed: Source,
    remembered: HashMap<Source, u64>,
    keep: Option<u64>,
    tallies: HashMap<Source, Tally>,
    feeds: HashMap<Source, Feed>,
    pub config_path: Option<PathBuf>,
    excluded: Vec<PathBuf>,
    changes_unknown: bool,
    dirty: bool,
    grown: bool,
}

impl App {
    pub fn new(mode: Mode, confirm_before_delete: bool) -> Self {
        Self {
            last_enter: None,
            last_action_key: None,
            keymap: Keymap::default(),
            last_project_press: None,
            widen_armed: None,
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
            tab: Tab::Artifacts,
            duplicates: DuplicateList::default(),
            issues: IssueList::default(),
            repos: RepoList::default(),
            min_gain: 0,
            overlay: Overlay::None,
            confirm_stats: None,
            typed: String::new(),
            typed_wrong: false,
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
            unknown_projects: HashSet::new(),
            listed: Source::Artifact,
            remembered: HashMap::new(),
            keep: None,
            tallies: HashMap::new(),
            feeds: HashMap::new(),
            config_path: None,
            excluded: Vec::new(),
            changes_unknown: false,
            dirty: false,
            grown: false,
        }
    }

    fn is_excluded(&self, path: &Path) -> bool {
        self.excluded.iter().any(|root| path.starts_with(root))
    }

    pub fn push(&mut self, candidate: Candidate) {
        self.add(Source::Artifact, String::new(), String::new(), candidate);
    }

    pub fn push_listed(
        &mut self,
        source: Source,
        mut candidate: Candidate,
        note: String,
        label: String,
    ) {
        candidate.id = tagged(source, candidate.id);
        self.add(source, note, label, candidate);
    }

    fn add(&mut self, source: Source, note: String, label: String, candidate: Candidate) {
        if source != Source::Docker && self.is_excluded(&candidate.path) {
            return;
        }
        self.index.insert(candidate.id, self.items.len());
        let tally = self.tallies.entry(source).or_default();
        tally.items += 1;
        tally.bytes += candidate.usage.allocated;
        let lower_path = if label.is_empty() {
            candidate.path.to_string_lossy().to_lowercase()
        } else {
            format!("{label} {}", candidate.path.to_string_lossy()).to_lowercase()
        };
        let rule_group = self.rule_group_of(&candidate);
        let project_group = self.project_group_of(&candidate);
        self.items.push(Item {
            candidate,
            source,
            note,
            label,
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
        if self.dirty {
            self.rebuild_duplicates();
            self.rebuild_issues();
            self.rebuild_repos();
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

    fn rebuild_duplicates(&mut self) {
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

    fn rebuild_issues(&mut self) {
        let needle = self.filter.to_lowercase();
        let list = &mut self.issues;
        list.rows = (0..list.entries.len())
            .filter(|&index| issue_matches(&list.entries[index], &needle))
            .collect();
        list.cursor = list.cursor.min(list.rows.len().saturating_sub(1));
    }

    fn rebuild_repos(&mut self) {
        let needle = self.filter.to_lowercase();
        let list = &mut self.repos;
        list.rows = (0..list.found.len())
            .filter(|&index| repo_matches(&list.found[index], &needle))
            .collect();
        list.cursor = list.cursor.min(list.rows.len().saturating_sub(1));
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

    pub fn switch(&mut self, tab: Tab) {
        if tab == self.tab {
            return;
        }
        self.end_range();
        self.disarm_widen();
        self.tab = tab;
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

    fn position(&self, id: u64) -> Option<usize> {
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

    fn mark(&mut self, index: usize, on: bool) {
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

    fn tally(&self) -> Tally {
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

    fn quit_key(&self) -> &str {
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

    fn compute_confirm_stats(&self) -> Option<ConfirmStats> {
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
        self.deleting() + self.repos.running.len()
    }

    pub fn deleting_in(&self, source: Source) -> usize {
        self.tallies.get(&source).map_or(0, |tally| tally.deleting)
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

fn issue_matches(issue: &Issue, needle: &str) -> bool {
    needle.is_empty()
        || contains(&issue.path, needle)
        || issue.message.to_lowercase().contains(needle)
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
        app_with(Mode::Permanent, true)
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
    fn deleting_skips_the_dialog_when_confirmation_is_off() {
        let mut app = app_with(Mode::Permanent, false);
        app.toggle();
        let jobs = app.delete_selected();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].1, Mode::Permanent);
        assert!(matches!(app.overlay, Overlay::None));
        assert_eq!(app.item(2).unwrap().status, Status::Deleting);
    }

    #[test]
    fn review_items_always_ask() {
        let mut app = app_with(Mode::Permanent, false);
        app.move_to(1);
        assert!(app.delete().is_empty());
        assert!(matches!(&app.overlay, Overlay::Confirm { ids, .. } if ids == &[3]));
        assert_eq!(app.item(3).unwrap().status, Status::Ready);
    }

    #[test]
    fn projects_with_uncommitted_changes_always_ask() {
        let mut app = app_with(Mode::Permanent, false);
        app.changes.insert(PathBuf::from("/p/a"), 2);
        app.select_safe();
        assert!(app.delete_selected().is_empty());
        assert!(matches!(&app.overlay, Overlay::Confirm { ids, .. } if ids.len() == 2));
        assert_eq!(app.confirm().len(), 2);
    }

    #[test]
    fn a_project_still_being_checked_for_changes_asks() {
        let mut app = app_with(Mode::Permanent, false);
        app.expect_status(PathBuf::from("/p/b"));
        app.toggle();
        assert!(app.delete_selected().is_empty());
        assert_eq!(
            app.confirm_stats.as_ref().map(|stats| stats.checking),
            Some(1)
        );
        app.set_statuses([(PathBuf::from("/p/b"), GitState::Changes(0))]);
        assert_eq!(
            app.confirm_stats.as_ref().map(|stats| stats.checking),
            Some(0)
        );
        app.overlay = Overlay::None;
        assert_eq!(app.delete_selected().len(), 1);
    }

    #[test]
    fn a_project_whose_git_check_failed_asks() {
        let mut app = app_with(Mode::Permanent, false);
        app.expect_status(PathBuf::from("/p/b"));
        app.set_statuses([(PathBuf::from("/p/b"), GitState::Unknown)]);
        app.toggle();
        assert!(app.delete_selected().is_empty());
        assert_eq!(app.pending_confirmation(), Confirmation::YesNo);
        assert_eq!(
            app.confirm_stats.as_ref().map(|stats| stats.unknown),
            Some(true)
        );
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
        app.finish(2, Mode::Permanent, Ok(()));
        app.finish(1, Mode::Permanent, Err("refused".to_owned()));
        assert_eq!(counters(&app), recount(&app));
        app.select_project();
        app.toggle();
        assert_eq!(counters(&app), recount(&app));
        app.clear_selection();
        assert_eq!(counters(&app), recount(&app));
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
                mode: Mode::Permanent
            }
        );
        let jobs = app.confirm();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].1, Mode::Permanent);
        assert_eq!(app.item(2).unwrap().status, Status::Deleting);
        app.finish(2, Mode::Permanent, Ok(()));
        assert_eq!(app.freed, 30);
        assert_eq!(app.item(2).unwrap().status, Status::Deleted);
    }

    #[test]
    fn trashed_items_do_not_count_as_freed_space() {
        let mut app = app_with(Mode::Trash, true);
        app.free = Some(100);
        assert_eq!(app.free_after(10, Mode::Permanent), Some(110));
        assert_eq!(app.free_after(10, Mode::Trash), Some(100));
        app.open_confirm();
        let jobs = app.confirm();
        assert_eq!(jobs[0].1, Mode::Trash);
        app.finish(2, Mode::Trash, Ok(()));
        assert_eq!(app.trashed, 30);
        assert_eq!(app.freed, 0);
    }

    #[test]
    fn project_grouping_orders_by_project_and_selects_whole_project() {
        let mut app = App::new(Mode::Permanent, true);
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
        let selected =
            |app: &App| -> Vec<u64> { app.selected().map(|item| item.candidate.id).collect() };
        app.select_project();
        assert_eq!(selected(&app), [1]);
        app.select_project();
        assert_eq!(selected(&app), [1, 4]);
        assert_eq!(app.selected_bytes(), 60);
        assert_eq!(app.selected_review(), 1);
        assert!(app.message.clone().unwrap_or_default().contains("widened"));
        app.select_project();
        assert!(selected(&app).is_empty());
        assert_eq!(app.selected_review(), 0);
    }

    #[test]
    fn a_review_only_project_warns_before_selecting() {
        let mut app = app();
        app.move_to(1);
        app.select_project();
        assert_eq!(app.selected_count(), 0);
        assert!(app.message.clone().unwrap_or_default().contains("review"));
        app.disarm_widen();
        app.select_project();
        assert_eq!(app.selected_count(), 0);
        app.select_project();
        assert_eq!(app.selected_review(), 1);
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
        let mut app = App::new(Mode::Permanent, true);
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
        app.finish(2, Mode::Permanent, Err("refused".to_owned()));
        assert_eq!(app.freed, 0);
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

    #[test]
    fn duplicate_rows_follow_the_search_and_keep_the_cursor_in_range() {
        let group = |paths: [&str; 2]| crate::dupes::Group {
            size: 10,
            allocated: 10,
            hash: String::new(),
            verified: false,
            members: paths
                .iter()
                .map(|path| crate::dupes::Member {
                    path: PathBuf::from(path),
                    links: Vec::new(),
                    modified: None,
                })
                .collect(),
        };
        let mut app = app();
        app.switch(Tab::Duplicates);
        app.set_duplicates(Duplicates {
            groups: vec![group(["/a/x", "/a/y"]), group(["/b/x", "/b/Photo"])],
            ..Duplicates::default()
        });
        app.move_to(usize::MAX);
        assert_eq!(app.duplicates.cursor, 5);
        app.set_filter("photo".to_owned());
        app.refresh();
        assert_eq!(
            app.duplicates.rows,
            [Spot::Group(1), Spot::Member(1, 0), Spot::Member(1, 1)]
        );
        assert_eq!(app.duplicates.cursor, 2);
    }
}
