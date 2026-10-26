use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use futures::StreamExt;
use futures::channel::{mpsc, oneshot};

use gpui::{
    AnyElement, App, Bounds, ClickEvent, ClipboardItem, Context, CursorStyle, DispatchPhase, Div,
    ElementId, Entity, FocusHandle, FontWeight, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PathPromptOptions, Pixels, Point, ScrollHandle, ScrollWheelEvent,
    SharedString, Stateful, Task, UniformListScrollHandle, Window, anchored, canvas, deferred, div,
    point, prelude::*, px, uniform_list,
};
use kuizer::config::{Config, Sidebar};
use kuizer::delete::{Removal, Status};
use kuizer::detect::{Category, Safety};
use kuizer::dupes::{self, Duplicates};
use kuizer::history::Action;
use kuizer::output::display_path;
use kuizer::platform;
use kuizer::scan::{self, Cancel, Candidate};

use crate::activity::{self, Activity, State, Step, Tracker, plural};
use crate::chart::{self, Measure, Segment};
use crate::commands::{self, Command, Quit, ResetZoom, TogglePalette, Undo, ZoomIn, ZoomOut};
use crate::field;
use crate::frame;
use crate::glossary::Term;
use crate::icon::{self, Icon};
use crate::model::{
    self, Batch, Confirmation, HistoryView, Loaded, Outcome, Part, Report, Restored, Scope, Source,
    TrashState,
};
use crate::motion;
use crate::palette::Palette;
use crate::reveal;
use crate::snapshot::{self, Recorder, Snapshots};
use crate::start::{self, Recent, ScanOptions};
use crate::stats::{Charts, Index, ReportStats, Selection, SelectionStats, Tally};
use crate::tab::Tab;
use crate::theme::{self, col, font, size, space, unit};
use crate::tooltip::{self, Tip, Tooltip};
use crate::ui::{self, ButtonKind};
use crate::zoom::Zoom;

const POLL: Duration = Duration::from_millis(100);
const LIST_LIMIT: usize = 6;
const DUPLICATE_GROUPS: usize = 200;
const ISSUE_LIMIT: usize = 500;
const HISTORY_BARS: usize = 24;
const WHEEL_NOTCH: Pixels = px(20.);

const TOAST_TIME: Duration = Duration::from_secs(10);
const TOAST_PROBLEMS: usize = 3;
mod chrome;
mod clean;
mod duplicates;
mod history;
mod keys;
mod launch;
mod lists;
mod map;
mod menu;
mod overlay;
mod overview;
mod pane;
mod repos;
mod session;
mod settings;
#[cfg(test)]
mod tests;

use clean::selection_details;
use keys::{found, found_path, keep_visible, shows_run};
use lists::Lists;
use map::SpaceMap;
use pane::{Chrome, Pane};
use repos::{GitJob, GitScope};
use session::spawn_worker;

pub use crate::commands::bind_keys;

enum Phase {
    Start,
    Loading(Box<Activity>),
    Ready(Box<Report>),
    Failed(String),
}

struct Finished {
    removed: usize,
    removed_bytes: u64,
    deleted: usize,
    failures: Vec<(String, String)>,
    history_error: Option<String>,
}

enum Update {
    Cached(Box<Report>),
    Activity(Box<Activity>),
    Loaded(Loaded),
    Done(Result<SystemTime, String>),
}

enum Searched {
    Stage(dupes::Stage),
    Done(Result<Duplicates, String>),
}

/// A duplicate search running beside the results: how far it got and the task that hears
/// from it. Dropping it stops listening.
struct Search {
    roots: Vec<PathBuf>,
    stage: Option<dupes::Stage>,
    _task: Task<()>,
}

enum Notice {
    Trashed {
        session: u64,
        items: usize,
        bytes: u64,
    },
    Restoring {
        session: u64,
    },
    Restored(Restored),
    Failed(String),
    Info(String),
}

impl Notice {
    fn lingers(&self) -> bool {
        match self {
            Self::Trashed { .. } | Self::Info(_) => true,
            Self::Restored(restored) => {
                restored.problems.is_empty() && restored.history_error.is_none()
            }
            Self::Restoring { .. } | Self::Failed(_) => false,
        }
    }
}

struct Toast {
    notice: Notice,
    shown: Duration,
}

enum Dialog {
    Confirm {
        rule: Confirmation,
        typed: String,
        only: Option<(Source, u64)>,
        stats: SelectionStats,
        preview: Vec<(Source, u64)>,
    },
    Running {
        permanent: bool,
    },
    Finished(Finished),
    Branches(GitScope),
}

/// One row a context menu can act on.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    Item(Source, u64, PathBuf),
    Duplicate(PathBuf),
    Issue(PathBuf),
    Run(u64, String),
    Repo(PathBuf),
}

impl Target {
    fn tab(&self) -> Tab {
        match self {
            Self::Item(Source::Artifact, ..) => Tab::Artifacts,
            Self::Item(Source::Cache, ..) => Tab::Caches,
            Self::Item(Source::Toolchain, ..) => Tab::Toolchains,
            Self::Item(Source::Docker, ..) => Tab::Docker,
            Self::Duplicate(_) => Tab::Duplicates,
            Self::Issue(_) => Tab::Issues,
            Self::Run(..) => Tab::History,
            Self::Repo(_) => Tab::Repos,
        }
    }

    fn path(&self) -> Option<&Path> {
        match self {
            Self::Item(_, _, path)
            | Self::Duplicate(path)
            | Self::Issue(path)
            | Self::Repo(path) => Some(path),
            Self::Run(..) => None,
        }
    }
}

/// A Shift+motion selection: the row it started on and the items it selected that were not
/// selected before, so moving back toward the anchor deselects only those.
#[derive(Debug)]
struct Span {
    tab: Tab,
    anchor: Target,
    anchor_row: Cell<usize>,
    added: HashSet<(Source, u64)>,
}

#[derive(Debug, Clone, Copy)]
struct Reclaimable {
    repos: u64,
    duplicates: u64,
}

/// The filters the artifact rows were last built with, so a filter that only narrows can start
/// from those rows instead of the whole report.
struct RowsFilter {
    report: u64,
    needle: Option<String>,
    safety: Option<Safety>,
    hidden: HashSet<Category>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Choice {
    Reveal,
    CopyPath,
    Toggle,
    Trash,
    Exclude,
    Restore,
    CopyCommand,
    Gc,
    Branches,
}

struct MenuItem {
    choice: Choice,
    label: &'static str,
    key: &'static str,
    enabled: bool,
    tip: Option<&'static str>,
    divided: bool,
}

impl MenuItem {
    fn new(choice: Choice, label: &'static str, key: &'static str) -> Self {
        Self {
            choice,
            label,
            key,
            enabled: true,
            tip: None,
            divided: false,
        }
    }

    fn tip(mut self, tip: &'static str) -> Self {
        self.tip = Some(tip);
        self
    }

    fn unless(mut self, reason: Option<&'static str>) -> Self {
        if let Some(reason) = reason {
            self.enabled = false;
            self.tip = Some(reason);
        }
        self
    }

    fn divided(mut self) -> Self {
        self.divided = true;
        self
    }

    fn answers(&self, key: &str) -> bool {
        match self.key {
            "Del" => key == "delete",
            letter => letter.eq_ignore_ascii_case(key),
        }
    }
}

struct Menu {
    target: Target,
    position: Point<Pixels>,
    active: Option<usize>,
}

const WAIT_FOR_REFRESH: &str = "Available once the fresh scan finishes";
const WAIT_FOR_RESTORE: &str = "Available once the restore finishes";
const NOT_ON_DISK: &str = "Docker and Podman objects are not files on disk";
const EXCLUDE_TIP: &str =
    "Skip this path in every future scan. It is added to scan.exclude in the config file";

type Reveal = fn(&Path) -> Result<(), String>;

static CONFIG_WRITE: Mutex<()> = Mutex::new(());

pub struct Kuizer {
    config: Config,
    roots: Vec<PathBuf>,
    options: ScanOptions,
    folder: String,
    whole_home: bool,
    collapsed: bool,
    icon_hovers: HashMap<&'static str, u64>,
    folder_error: Option<String>,
    folder_check: Option<Task<()>>,
    recent: Recent,
    snapshots: Snapshots,
    stale: Option<SystemTime>,
    pending: HashSet<Part>,
    early: Vec<Loaded>,
    streamed: bool,
    progress: Activity,
    search: Option<Search>,
    search_error: Option<String>,
    zoom: Zoom,
    wheel: Pixels,
    moving: bool,
    cancel: Option<Cancel>,
    started: Instant,
    font: SharedString,
    focus: FocusHandle,
    phase: Phase,
    tab: Tab,
    safety: Option<Safety>,
    hidden: HashSet<Category>,
    rows: Vec<usize>,
    rows_bytes: u64,
    rows_filter: Option<RowsFilter>,
    rows_generation: u64,
    all_checked: Cell<Option<(u64, u64, bool)>>,
    artifact_row: Cell<usize>,
    selection: Selection,
    index: Index,
    report_stats: ReportStats,
    report_generation: u64,
    reclaimable: Cell<Option<(u64, Reclaimable)>>,
    range: Option<Span>,
    dialog: Option<Dialog>,
    task: Option<Task<()>>,
    toast: Option<Toast>,
    toast_task: Option<Task<()>>,
    restore_task: Option<Task<()>>,
    menu: Option<Menu>,
    palette: Option<Palette>,
    shortcuts: bool,
    settings: Option<settings::Settings>,
    filtering: bool,
    filters: HashMap<Tab, String>,
    cursors: HashMap<Tab, Target>,
    cursor_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    artifact_scroll: UniformListScrollHandle,
    list_scrolls: HashMap<Tab, UniformListScrollHandle>,
    lists: Lists,
    scrolls: HashMap<Tab, ScrollHandle>,
    reveal_cursor: Rc<Cell<bool>>,
    config_path: Option<PathBuf>,
    excluded: Vec<PathBuf>,
    excluding: HashSet<PathBuf>,
    reveal: Reveal,
    map: SpaceMap,
    git: GitJob,
    git_task: Option<Task<()>>,
    content: Option<Entity<Pane>>,
    chrome: Option<Entity<Chrome>>,
    cache_content: bool,
    loader: Option<Entity<chart::Loader>>,
}

impl Kuizer {
    #[allow(
        clippy::too_many_arguments,
        reason = "each saved state is passed in so tests can point it at temporary files"
    )]
    pub fn new(
        config: Config,
        roots: Vec<PathBuf>,
        recent: Recent,
        snapshots: Snapshots,
        zoom: Zoom,
        font: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus);
        let folder = recent.default_folder(&config, &roots);
        Self::probe_recent(recent.paths.clone(), cx);
        cx.set_global(tooltip::Typeface(font.clone()));
        motion::configure(config.appearance.motion);
        theme::configure(config.appearance.theme);
        let collapsed = config.appearance.sidebar == Sidebar::Collapsed;
        let mut app = Self {
            options: ScanOptions::from_config(&config),
            config,
            roots,
            folder,
            whole_home: false,
            collapsed,
            icon_hovers: HashMap::new(),
            folder_error: None,
            folder_check: None,
            recent,
            snapshots,
            stale: None,
            pending: HashSet::new(),
            early: Vec::new(),
            streamed: false,
            progress: Activity::default(),
            search: None,
            search_error: None,
            zoom,
            wheel: px(0.),
            moving: false,
            cancel: None,
            started: Instant::now(),
            font,
            focus,
            phase: Phase::Start,
            tab: Tab::Overview,
            safety: None,
            hidden: HashSet::new(),
            rows: Vec::new(),
            rows_bytes: 0,
            rows_filter: None,
            rows_generation: 0,
            all_checked: Cell::new(None),
            artifact_row: Cell::new(0),
            selection: Selection::default(),
            index: Index::default(),
            report_stats: ReportStats::default(),
            report_generation: 0,
            reclaimable: Cell::new(None),
            range: None,
            dialog: None,
            task: None,
            toast: None,
            toast_task: None,
            restore_task: None,
            menu: None,
            palette: None,
            shortcuts: false,
            settings: None,
            filtering: false,
            filters: HashMap::new(),
            cursors: HashMap::new(),
            cursor_bounds: Rc::new(Cell::new(None)),
            artifact_scroll: UniformListScrollHandle::new(),
            list_scrolls: [Tab::Caches, Tab::Toolchains, Tab::Docker, Tab::Repos]
                .into_iter()
                .map(|tab| (tab, UniformListScrollHandle::new()))
                .collect(),
            lists: Lists::default(),
            scrolls: [Tab::Duplicates, Tab::History, Tab::Issues]
                .into_iter()
                .map(|tab| (tab, ScrollHandle::new()))
                .collect(),
            reveal_cursor: Rc::new(Cell::new(false)),
            config_path: kuizer::config::default_path(),
            excluded: Vec::new(),
            excluding: HashSet::new(),
            reveal: reveal::reveal,
            map: SpaceMap::default(),
            git: GitJob::default(),
            git_task: None,
            content: None,
            chrome: None,
            cache_content: cfg!(not(test)),
            loader: None,
        };
        if !app.roots.is_empty() {
            app.recent.remember(&app.roots);
            app.phase = Phase::Loading(Box::default());
            let view = cx.weak_entity();
            window.on_next_frame(move |_, cx| {
                view.update(cx, |app, cx| app.rescan(cx)).ok();
            });
        }
        app
    }

    fn probe_recent(paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let probed = cx.background_spawn(async move { Recent::probe(&paths) });
        cx.spawn(async move |this, cx| {
            let gone = probed.await;
            this.update(cx, |app, cx| app.mark_recent_gone(gone, cx))
                .ok();
        })
        .detach();
    }

    fn mark_recent_gone(&mut self, gone: Vec<PathBuf>, cx: &mut Context<Self>) {
        let unchanged = self
            .recent
            .paths
            .iter()
            .all(|path| self.recent.is_gone(path) == gone.contains(path));
        if unchanged {
            return;
        }
        let before = self.recent.default_folder(&self.config, &self.roots);
        self.recent.mark_gone(gone);
        if self.folder == before {
            self.folder = self.recent.default_folder(&self.config, &self.roots);
        }
        cx.notify();
    }

    fn report(&self) -> Option<&Report> {
        match &self.phase {
            Phase::Ready(report) => Some(report),
            _ => None,
        }
    }

    fn busy(&self) -> bool {
        matches!(self.phase, Phase::Loading(_))
            || matches!(self.dialog, Some(Dialog::Running { .. }))
            || self.restoring()
    }

    fn restoring(&self) -> bool {
        matches!(
            self.toast,
            Some(Toast {
                notice: Notice::Restoring { .. },
                ..
            })
        )
    }

    fn can_restore(&self) -> bool {
        self.report().is_some() && !self.busy() && !self.refreshing() && self.dialog.is_none()
    }

    fn show_toast(&mut self, notice: Notice, cx: &mut Context<Self>) {
        self.toast = Some(Toast {
            notice,
            shown: Duration::ZERO,
        });
        self.toast_task = Some(cx.spawn(async move |this, cx| {
            let mut wait = TOAST_TIME;
            loop {
                cx.background_executor().timer(wait).await;
                let next = this.update(cx, |app, cx| {
                    let paused = app.dialog.is_some();
                    let toast = app.toast.as_mut()?;
                    if !toast.notice.lingers() {
                        return None;
                    }
                    if paused {
                        return Some(POLL);
                    }
                    toast.shown += wait;
                    if toast.shown < TOAST_TIME {
                        return Some(TOAST_TIME - toast.shown);
                    }
                    app.toast = None;
                    app.redraw_chrome(cx);
                    None
                });
                match next {
                    Ok(Some(next)) => wait = next,
                    _ => break,
                }
            }
        }));
        cx.notify();
    }

    fn dismiss_toast(&mut self, cx: &mut Context<Self>) {
        if self.toast.is_some() && !self.restoring() {
            self.toast = None;
            cx.notify();
        }
    }

    fn quit(&mut self, _: &Quit, _window: &mut Window, cx: &mut Context<Self>) {
        cx.quit();
    }

    /// Whether the results on screen are still being replaced by a running scan, so nothing in
    /// them may be removed yet: some sections come from an earlier scan or are not in yet.
    fn refreshing(&self) -> bool {
        self.stale.is_some() || (self.cancel.is_some() && matches!(self.phase, Phase::Ready(_)))
    }

    fn refresh_rows(&mut self) {
        self.rows_generation += 1;
        let Phase::Ready(report) = &self.phase else {
            self.rows.clear();
            self.rows_bytes = 0;
            self.rows_filter = None;
            return;
        };
        let filter = RowsFilter {
            report: self.report_generation,
            needle: self.needle(Tab::Artifacts),
            safety: self.safety,
            hidden: self.hidden.clone(),
        };
        let shows = |candidate: &Candidate| {
            filter
                .safety
                .is_none_or(|safety| candidate.rule.safety == safety)
                && !filter.hidden.contains(&candidate.rule.category)
                && (filter.needle.is_none()
                    || found(
                        filter.needle.as_deref(),
                        &[
                            candidate.path.to_string_lossy().as_ref(),
                            candidate.rule.name.as_str(),
                            candidate.rule.category.as_str(),
                        ],
                    ))
        };
        let narrows = self.rows_filter.as_ref().is_some_and(|last| {
            last.report == filter.report
                && last.safety == filter.safety
                && last.hidden == filter.hidden
                && match (&last.needle, &filter.needle) {
                    (None, _) => true,
                    (Some(last), Some(next)) => next.contains(last.as_str()),
                    (Some(_), None) => false,
                }
        });
        if narrows {
            self.rows.retain(|&index| shows(&report.candidates[index]));
        } else {
            self.rows = report
                .candidates
                .iter()
                .enumerate()
                .filter(|(_, candidate)| shows(candidate))
                .map(|(index, _)| index)
                .collect();
        }
        self.rows_bytes = self
            .rows
            .iter()
            .map(|&index| report.candidates[index].usage.allocated)
            .sum();
        self.rows_filter = Some(filter);
    }

    fn grow_stats(&mut self) {
        self.report_generation += 1;
        self.index = Index::default();
        self.report_stats = match &self.phase {
            Phase::Ready(report) => ReportStats::new(report),
            _ => ReportStats::default(),
        };
    }

    /// Recomputes what the views add up from the report and weighs the selection again. Runs
    /// whenever the report changes.
    fn refresh_stats(&mut self) {
        self.report_generation += 1;
        let Phase::Ready(report) = &self.phase else {
            self.index = Index::default();
            self.report_stats = ReportStats::default();
            return;
        };
        self.index = Index::default();
        self.report_stats = ReportStats::new(report);
        if !self.selection.is_empty() {
            let index = &self.index;
            self.selection.reweigh(|key| index.find(report, key));
        }
    }

    /// Every selected item, in the order the lists show them.
    fn selected(&self) -> Vec<(Source, &Candidate)> {
        let Some(report) = self.report() else {
            return Vec::new();
        };
        let mut items: Vec<(Source, usize, &Candidate)> = self
            .selection
            .keys()
            .filter_map(|&key| {
                let (position, candidate) = self.index.locate(report, key)?;
                Some((key.0, position, candidate))
            })
            .collect();
        items.sort_unstable_by_key(|&(source, position, _)| (source, position));
        items
            .into_iter()
            .map(|(source, _, candidate)| (source, candidate))
            .collect()
    }

    /// Selects an item of the report, and says whether it was not selected before.
    fn select(&mut self, key: (Source, u64)) -> bool {
        let Phase::Ready(report) = &self.phase else {
            return false;
        };
        self.index
            .find(report, key)
            .is_some_and(|candidate| self.selection.insert(key, candidate))
    }

    /// Whether every artifact row on screen is selected, cached until the rows or the selection
    /// change.
    fn all_rows_checked(&self) -> bool {
        let generation = (self.rows_generation, self.selection.generation());
        if let Some((rows, selection, checked)) = self.all_checked.get()
            && (rows, selection) == generation
        {
            return checked;
        }
        let checked = self.report().is_some_and(|report| {
            !self.rows.is_empty()
                && self.selection.stats().source(Source::Artifact).all.count >= self.rows.len()
                && self.rows.iter().all(|&index| {
                    self.selection
                        .contains(&(Source::Artifact, report.candidates[index].id))
                })
        });
        self.all_checked
            .set(Some((generation.0, generation.1, checked)));
        checked
    }

    fn toggle(&mut self, source: Source, id: u64, cx: &mut Context<Self>) {
        self.range = None;
        if !self.selection.remove(&(source, id)) {
            self.select((source, id));
        }
        cx.notify();
    }

    fn select_safe(&mut self, cx: &mut Context<Self>) {
        let Some(report) = self.report() else {
            return;
        };
        let keys: Vec<(Source, u64)> = match self.tab {
            Tab::Caches => self
                .listed(Tab::Caches)
                .slots
                .iter()
                .filter_map(|&(at, _)| report.caches.get(at))
                .filter(|(_, candidate)| candidate.rule.safety == Safety::Safe)
                .map(|(_, candidate)| (Source::Cache, candidate.id))
                .collect(),
            Tab::Toolchains => self
                .listed(Tab::Toolchains)
                .slots
                .iter()
                .filter_map(|&(at, _)| report.toolchains.get(at))
                .filter(|(_, candidate)| candidate.rule.safety == Safety::Safe)
                .map(|(_, candidate)| (Source::Toolchain, candidate.id))
                .collect(),
            Tab::Docker => self
                .listed(Tab::Docker)
                .slots
                .iter()
                .filter_map(|&(at, _)| report.docker.get(at))
                .filter(|(_, candidate)| candidate.rule.safety == Safety::Safe)
                .map(|(_, candidate)| (Source::Docker, candidate.id))
                .collect(),
            _ => self
                .rows
                .iter()
                .map(|&index| &report.candidates[index])
                .filter(|candidate| candidate.rule.safety == Safety::Safe)
                .map(|candidate| (Source::Artifact, candidate.id))
                .collect(),
        };
        for key in keys {
            self.select(key);
        }
        cx.notify();
    }

    fn toggle_visible(&mut self, cx: &mut Context<Self>) {
        let Some(report) = self.report() else {
            return;
        };
        let keys: Vec<(Source, u64)> = self
            .rows
            .iter()
            .map(|&index| (Source::Artifact, report.candidates[index].id))
            .collect();
        if self.all_rows_checked() {
            for key in &keys {
                self.selection.remove(key);
            }
        } else {
            for key in keys {
                self.select(key);
            }
        }
        cx.notify();
    }

    fn item(&self, key: (Source, u64)) -> Option<(Source, &Candidate)> {
        let report = self.report()?;
        Some((key.0, self.index.find(report, key)?))
    }

    fn zoom_in(&mut self, _: &ZoomIn, _window: &mut Window, cx: &mut Context<Self>) {
        if self.zoom.zoom_in() {
            cx.notify();
        }
    }

    fn zoom_out(&mut self, _: &ZoomOut, _window: &mut Window, cx: &mut Context<Self>) {
        if self.zoom.zoom_out() {
            cx.notify();
        }
    }

    fn reset_zoom(&mut self, _: &ResetZoom, _window: &mut Window, cx: &mut Context<Self>) {
        if self.zoom.reset() {
            cx.notify();
        }
    }

    fn wheel_zoom(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        self.wheel += event.delta.pixel_delta(WHEEL_NOTCH).y;
        if self.wheel.abs() < WHEEL_NOTCH {
            return;
        }
        let changed = if self.wheel > px(0.) {
            self.zoom.zoom_in()
        } else {
            self.zoom.zoom_out()
        };
        self.wheel = px(0.);
        if changed {
            cx.notify();
        }
    }

    /// Shows a message unless the restore message is up, which tracks a running restore.
    fn tell(&mut self, notice: Notice, cx: &mut Context<Self>) {
        if !self.restoring() {
            self.show_toast(notice, cx);
        }
    }

    fn trash_lock(&self) -> Option<&'static str> {
        if self.restoring() {
            Some(WAIT_FOR_RESTORE)
        } else if self.refreshing() {
            Some(
                "Available once the fresh scan finishes. These results come from an earlier scan, so every item is checked again first",
            )
        } else {
            None
        }
    }

    fn render_content(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.ensure_map();
        match &self.phase {
            Phase::Start => self.render_start(cx),
            Phase::Loading(activity) => self.render_loading(activity),
            Phase::Failed(error) => centered(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(unit(space::S))
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .child("The scan failed"),
                    )
                    .child(div().text_color(theme::danger()).child(error.clone())),
            ),
            Phase::Ready(_) => {
                let pending = self
                    .tab
                    .part()
                    .filter(|part| self.pending.contains(part))
                    .map(|_| {
                        let status = self.tab.status(&self.progress, &self.options);
                        div()
                            .debug_selector(|| "section-pending".to_owned())
                            .flex()
                            .items_center()
                            .gap(unit(space::S))
                            .px(unit(space::GUTTER))
                            .pt(unit(space::S))
                            .text_size(unit(font::SMALL))
                            .text_color(theme::muted())
                            .child(ui::tag(
                                "section-scanning",
                                "Still scanning",
                                ui::Tone::Neutral,
                            ))
                            .child(status)
                    });
                let list = match self.tab {
                    Tab::Overview => return self.render_overview(cx),
                    Tab::Artifacts => self.render_artifacts(cx),
                    Tab::Caches => self.render_caches(cx),
                    Tab::Toolchains => self.render_toolchains(cx),
                    Tab::Docker => self.render_docker(cx),
                    Tab::Repos => self.render_repos(cx),
                    Tab::Duplicates => self.render_duplicates(cx),
                    Tab::History => self.render_history(cx),
                    Tab::Issues => self.render_issues(cx),
                };
                div()
                    .flex()
                    .flex_col()
                    .size_full()
                    .children(pending)
                    .child(div().flex().flex_col().flex_1().min_h_0().child(list))
                    .into_any_element()
            }
        }
    }

    /// A tooltip about an item that is built from the report only when it shows, so a row on
    /// screen holds just the item key.
    fn item_tip(
        key: (Source, u64),
        cx: &Context<Self>,
        describe: fn(&Candidate, &Report) -> Tooltip,
    ) -> impl Fn(&mut Window, &mut App) -> gpui::AnyView + use<> {
        let app = cx.weak_entity();
        move |window, cx| {
            let tooltip = app.upgrade().and_then(|app| {
                let app = app.read(cx);
                let (_, candidate) = app.item(key)?;
                Some(describe(candidate, app.report()?))
            });
            tooltip.unwrap_or_else(|| Tooltip::new("")).build()(window, cx)
        }
    }

    fn render_artifacts(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.report().is_none() {
            return div().into_any_element();
        }
        let shown_bytes = self.rows_bytes;
        let all_checked = self.all_rows_checked();
        let chip = |category: Category| {
            let shown = !self.hidden.contains(&category);
            div()
                .id(category.as_str())
                .flex()
                .items_center()
                .gap(unit(space::S))
                .px(unit(space::M))
                .h(unit(size::SEGMENT))
                .rounded(theme::radius_control())
                .cursor_pointer()
                .text_color(if shown { theme::text() } else { theme::faint() })
                .hover(|style| style.bg(theme::hover()))
                .tip(tooltip::term(Term::category(category)))
                .child(ui::checkbox(shown))
                .child(category.as_str())
                .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                    if !app.hidden.remove(&category) {
                        app.hidden.insert(category);
                    }
                    app.refresh_rows();
                    cx.notify();
                }))
        };
        let chips: Vec<Stateful<Div>> = Category::ALL.into_iter().map(chip).collect();
        let toolbar = self
            .list_toolbar(cx)
            .child(div().flex().gap(unit(space::XXS)).children(chips))
            .child(div().flex_1())
            .child(self.tally(Tab::Artifacts, Some(shown_bytes)));
        let header = table_header()
            .child(
                div()
                    .id("select-visible")
                    .cursor_pointer()
                    .tip(tooltip::plain("Select or clear every item in this list"))
                    .child(ui::checkbox(all_checked))
                    .on_click(cx.listener(|app, _: &ClickEvent, _, cx| {
                        if !app.busy() {
                            app.toggle_visible(cx);
                        }
                    })),
            )
            .child(head(col::M, "Size", Term::Size).text_right())
            .child(head(col::S, "Safety", Term::Safety))
            .child(head(col::L, "Category", Term::Category))
            .child(head(col::XL, "Rule", Term::Rule))
            .child(head(col::XS, "Changed", Term::Changed).text_right())
            .child(div().flex_1().child("Path"));
        let list = uniform_list(
            "artifacts",
            self.rows.len(),
            cx.processor(|app, range: Range<usize>, _window, cx| {
                let Some(report) = app.report() else {
                    return Vec::new();
                };
                let cursor = app.cursor_target();
                range
                    .filter_map(|row| {
                        let candidate = &report.candidates[*app.rows.get(row)?];
                        let id = candidate.id;
                        let key = (Source::Artifact, id);
                        let checked = app.selection.contains(&key);
                        let review = candidate.rule.safety == Safety::Review;
                        let target = Target::Item(Source::Artifact, id, candidate.path.clone());
                        let row = table_row(("artifact", id), checked)
                            .debug_selector(move || format!("artifact-row-{row}"));
                        Some(
                            app.menu_row(row, target, cursor.as_ref(), cx)
                                .child(ui::checkbox(checked))
                                .child(
                                    column(col::M)
                                        .id(("artifact-size", id))
                                        .text_right()
                                        .tip(Self::item_tip(key, cx, |candidate, report| {
                                            size_tooltip(candidate, &report.roots)
                                        }))
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(theme::bytes(candidate.usage.allocated)),
                                )
                                .child(
                                    column(col::S)
                                        .flex()
                                        .child(ui::safety_label(("artifact-safety", id), review)),
                                )
                                .child(
                                    column(col::L)
                                        .id(("artifact-category", id))
                                        .text_color(theme::muted())
                                        .tip(tooltip::term(Term::category(candidate.rule.category)))
                                        .child(candidate.rule.category.as_str()),
                                )
                                .child(
                                    column(col::XL)
                                        .id(("artifact-rule", id))
                                        .truncate()
                                        .tip(Self::item_tip(key, cx, |candidate, _| {
                                            Tooltip::new(format!(
                                                "{} Ecosystem: {}.",
                                                Term::Rule.text(),
                                                candidate.rule.ecosystem
                                            ))
                                            .title(candidate.rule.name.clone())
                                        }))
                                        .child(candidate.rule.name.clone()),
                                )
                                .child(
                                    column(col::XS)
                                        .text_right()
                                        .text_color(theme::muted())
                                        .child(theme::age(candidate.newest)),
                                )
                                .child(
                                    div()
                                        .id(("artifact-path", id))
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_color(theme::muted())
                                        .tip(Self::item_tip(key, cx, |candidate, _| {
                                            Tooltip::new(display_path(&candidate.path))
                                        }))
                                        .child(model::short_path(&candidate.path, &report.roots)),
                                )
                                .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                                    if !app.busy() {
                                        app.toggle(Source::Artifact, id, cx);
                                    }
                                })),
                        )
                    })
                    .collect()
            }),
        )
        .track_scroll(self.artifact_scroll.clone())
        .debug_selector(|| "artifact-list".to_owned())
        .flex_1()
        .w_full();
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(toolbar)
            .child(header)
            .child(if self.rows.is_empty() {
                ui::empty("No items match these filters").into_any_element()
            } else {
                list.into_any_element()
            })
            .into_any_element()
    }

    fn render_issues(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(report) = self.report() else {
            return div().into_any_element();
        };
        let toolbar = self
            .list_toolbar(cx)
            .child(div().flex_1())
            .child(self.tally(Tab::Issues, None));
        if report.issues.is_empty() {
            return framed(toolbar, centered(ui::empty("The scan reported no issues")));
        }
        let cursor = self.cursor_target();
        let needle = self.needle(Tab::Issues);
        let list = scroll("issues")
            .debug_selector(|| "issue-list".to_owned())
            .track_scroll(&self.scroll_for(Tab::Issues))
            .child(
                div()
                    .text_color(theme::muted())
                    .child("Folders that could not be read or were skipped"),
            )
            .children(
                report
                    .issues
                    .iter()
                    .take(ISSUE_LIMIT)
                    .enumerate()
                    .filter(|(_, issue)| {
                        found(
                            needle.as_deref(),
                            &[
                                issue.path.to_string_lossy().as_ref(),
                                issue.message.as_str(),
                            ],
                        )
                    })
                    .map(|(index, issue)| {
                        let row = div()
                            .flex()
                            .flex_col()
                            .gap(unit(space::XXS))
                            .py(unit(space::S))
                            .border_b_1()
                            .border_color(theme::border())
                            .debug_selector(move || format!("issue-row-{index}"));
                        self.menu_row(row, Target::Issue(issue.path.clone()), cursor.as_ref(), cx)
                            .child(
                                div()
                                    .id(("issue", index))
                                    .truncate()
                                    .tip(tooltip::path(&issue.path))
                                    .child(display_path(&issue.path)),
                            )
                            .child(
                                div()
                                    .text_size(unit(font::SMALL))
                                    .text_color(theme::faint())
                                    .child(issue.message.clone()),
                            )
                    }),
            )
            .into_any_element();
        framed(toolbar, list)
    }

    fn shown_path(&self, path: &Path) -> String {
        match self.report() {
            Some(report) => model::short_path(path, &report.roots),
            None => display_path(path),
        }
    }
}

impl Render for Kuizer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.set_rem_size(self.zoom.rem_size());
        let view = cx.entity().downgrade();
        let wheel = canvas(
            |_, _, _| {},
            move |_, (), window, _| {
                window.on_mouse_event(move |event: &ScrollWheelEvent, phase, _, cx| {
                    if phase != DispatchPhase::Capture || !event.modifiers.secondary() {
                        return;
                    }
                    cx.stop_propagation();
                    view.update(cx, |app, cx| app.wheel_zoom(event, cx)).ok();
                });
            },
        )
        .absolute()
        .size_full();
        let content = self.content_pane(cx);
        let chrome = self.chrome_view(cx);
        let dialog = self.render_dialog(cx);
        let toast = self.render_toast(cx);
        let menu = self.render_menu(cx);
        let palette = self.render_palette(cx);
        let shortcuts = self.render_shortcuts(cx);
        let settings = self.render_settings(cx);
        let start = matches!(self.phase, Phase::Start);
        let titlebar = self.render_titlebar(window, cx);
        let root = div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme::background())
            .text_color(theme::text())
            .font_family(self.font.clone())
            .text_size(unit(font::BODY))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_action(cx.listener(Self::zoom_in))
            .on_action(cx.listener(Self::zoom_out))
            .on_action(cx.listener(Self::reset_zoom))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::toggle_palette))
            .on_action(cx.listener(Self::quit))
            .child(wheel)
            .child(titlebar)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .when(!start, |this| this.child(self.render_sidebar(cx)))
                    .child(div().flex().flex_col().flex_1().min_w_0().child(content))
                    .when(self.busy() || self.refreshing(), |this| {
                        this.child(
                            div().absolute().top_0().left_0().right_0().child(
                                self.loader
                                    .get_or_insert_with(|| cx.new(|_| chart::Loader))
                                    .clone(),
                            ),
                        )
                    })
                    .when(
                        matches!(self.dialog, Some(Dialog::Confirm { .. })),
                        |this| this.child(div().id("confirm-guard").absolute().inset_0().occlude()),
                    )
                    .children(toast),
            )
            .when(!start, |this| this.child(self.render_statusbar(cx)))
            .children(dialog)
            .children(menu)
            .children(palette)
            .children(shortcuts)
            .children(settings)
            .child(div().absolute().size_0().child(chrome))
            .child(tooltip::layer(cx));
        frame::frame(root, window)
    }
}

fn hint(command: &Command) -> &'static str {
    static HINTS: OnceLock<Vec<(Command, String)>> = OnceLock::new();
    HINTS
        .get_or_init(|| {
            commands::table()
                .iter()
                .filter_map(|spec| {
                    Some((spec.command.clone(), commands::label(spec.keys.first()?)))
                })
                .collect()
        })
        .iter()
        .find(|(known, _)| known == command)
        .map_or("", |(_, key)| key.as_str())
}

fn centered(child: impl IntoElement) -> AnyElement {
    div()
        .flex()
        .size_full()
        .items_center()
        .justify_center()
        .child(child)
        .into_any_element()
}

/// A section with its toolbar on top and the rest filling what is left below it.
fn framed(toolbar: Div, body: AnyElement) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .size_full()
        .child(toolbar)
        .child(div().flex().flex_col().flex_1().min_h_0().child(body))
        .into_any_element()
}

fn scroll(id: &'static str) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_col()
        .gap(unit(space::XL))
        .size_full()
        .p(unit(space::GUTTER))
        .overflow_y_scroll()
}

fn column(width: f32) -> Div {
    div().w(unit(width)).flex_none()
}

fn head(width: f32, label: &'static str, term: Term) -> gpui::Stateful<Div> {
    column(width)
        .id(label)
        .tip(tooltip::term(term))
        .child(label)
}

fn table_header() -> Div {
    ui::tabular(div())
        .flex()
        .flex_none()
        .items_center()
        .gap(unit(space::L))
        .h(unit(size::ROW))
        .px(unit(space::GUTTER))
        .bg(theme::panel())
        .border_b_1()
        .border_color(theme::border())
        .text_size(unit(font::SMALL))
        .text_color(theme::muted())
}

fn table_row(id: (&'static str, u64), checked: bool) -> gpui::Stateful<Div> {
    ui::tabular(div())
        .id(gpui::ElementId::NamedInteger(id.0.into(), id.1))
        .flex()
        .items_center()
        .gap(unit(space::L))
        .w_full()
        .h(unit(size::ROW))
        .px(unit(space::GUTTER))
        .cursor_pointer()
        .border_b_1()
        .border_color(theme::border())
        .when(checked, |this| this.bg(theme::selected()))
        .hover(|style| style.bg(theme::hover()))
}

fn size_details(
    candidate: &Candidate,
    roots: &[PathBuf],
) -> impl Fn(&mut Window, &mut App) -> gpui::AnyView + use<> {
    size_tooltip(candidate, roots).build()
}

fn size_tooltip(candidate: &Candidate, roots: &[PathBuf]) -> Tooltip {
    let usage = candidate.usage;
    let mut details = Tooltip::details(theme::bytes(usage.allocated))
        .row("On disk", theme::bytes(usage.allocated))
        .row("File sizes", theme::bytes(usage.apparent))
        .row("Entries", usage.entries.to_string());
    if candidate.newest.is_some() {
        details = details.row(
            "Last change",
            format!("{} ago", theme::age(candidate.newest)),
        );
    }
    if let Some(project) = &candidate.project {
        details = details.row("Project", model::short_path(project, roots));
    }
    if candidate.project_last_active.is_some() {
        details = details.row(
            "Project active",
            format!("{} ago", theme::age(candidate.project_last_active)),
        );
    }
    if usage.partial {
        details =
            details.note("Some folders inside could not be read, so the real size is larger.");
    }
    details
}

fn toolchain_label(toolchain: &kuizer::toolchains::Toolchain) -> String {
    if toolchain.tool == kuizer::toolchains::Tool::Sdkman {
        format!("{} {}", toolchain.runtime, toolchain.version)
    } else {
        toolchain.version.clone()
    }
}

fn cache_label(cache: &kuizer::caches::KnownCache) -> String {
    if cache.name == cache.tool {
        cache.name.to_owned()
    } else {
        format!("{}  {}", cache.tool, cache.name)
    }
}

fn short_time(stamp: &str) -> String {
    stamp.get(..16).unwrap_or(stamp).replacen('T', " ", 1)
}
