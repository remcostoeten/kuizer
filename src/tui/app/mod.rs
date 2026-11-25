mod deletion;
mod filter;
mod selection;
mod tabs;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::cli::SortKey;
use crate::delete::Mode;
use crate::detect::Category;
use crate::dupes::{Duplicates, Stage};
use crate::history::{History, TrashSnapshot};
use crate::keymap::{Action as Key, Keymap};
use crate::repos::{Collected, Pruned, RepoError, Repository};
use crate::scan::{Candidate, Issue, Progress, Summary};
use crate::source::Source;
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
    History,
}

impl Tab {
    pub const ALL: [Tab; 8] = [
        Self::Artifacts,
        Self::Caches,
        Self::Duplicates,
        Self::Issues,
        Self::Toolchains,
        Self::Docker,
        Self::Repos,
        Self::History,
    ];

    pub fn source(self) -> Option<Source> {
        match self {
            Self::Artifacts => Some(Source::Artifact),
            Self::Caches => Some(Source::Cache),
            Self::Toolchains => Some(Source::Toolchain),
            Self::Docker => Some(Source::Docker),
            Self::Duplicates | Self::Issues | Self::Repos | Self::History => None,
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
            Self::History => Key::History,
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
            Self::History => "history",
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

pub(super) static IDLE: Feed = Feed {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistorySpot {
    Run(usize),
    Item(usize, usize),
}

#[derive(Debug)]
pub struct RestoreWork {
    pub label: String,
    pub history: History,
    pub targets: Vec<(usize, usize)>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RestoreSummary {
    pub restored: usize,
    pub bytes: u64,
    pub skipped: usize,
    pub failed: usize,
    pub problem: Option<String>,
    pub history_error: Option<String>,
}

#[derive(Debug)]
pub struct RestoreDone {
    pub label: String,
    pub result: Result<RestoreSummary, String>,
    pub updated: Option<(History, TrashSnapshot)>,
}

#[derive(Debug, Default)]
pub struct HistoryList {
    pub load: Load,
    pub stale: bool,
    pub restoring: bool,
    pub history: History,
    pub trash: TrashSnapshot,
    pub problem: Option<String>,
    pub expanded: HashSet<u64>,
    pub rows: Vec<HistorySpot>,
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
    Restore {
        label: String,
        targets: Vec<(usize, usize)>,
        bytes: u64,
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
    pub history: HistoryList,
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
            history: HistoryList::default(),
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
}
