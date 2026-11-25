use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use futures::StreamExt;
use futures::channel::{mpsc, oneshot};

use gpui::{
    AnyElement, App, Bounds, ClickEvent, ClipboardItem, Context, CursorStyle, DispatchPhase, Div,
    ElementId, Entity, FocusHandle, FontWeight, KeyDownEvent, KeyUpEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PathPromptOptions, Pixels, Point, ScrollHandle,
    ScrollWheelEvent, SharedString, Stateful, Task, UniformListScrollHandle, Window, anchored,
    canvas, deferred, div, point, prelude::*, px, relative, uniform_list,
};
use kuizer::config::{Config, DeleteMode, Sidebar};
use kuizer::detect::{Category, Safety};
use kuizer::dupes::{self, Duplicates};
use kuizer::history::Action;
use kuizer::keymap::{Action as Key, Keymap, Scope};
use kuizer::platform::{self, display_path};
use kuizer::scan::{self, Cancel, Candidate};

use crate::activity::{self, Activity, State, Step, Tracker, plural};
use crate::chart::{self, Measure, Segment};
use crate::commands::{self, Command, Quit, ResetZoom, TogglePalette, ZoomIn, ZoomOut};
use crate::field;
use crate::frame;
use crate::glossary::Term;
use crate::icon::{self, Icon};
use crate::model::{
    self, Batch, HistoryView, Loaded, Outcome, Part, Report, Restored, Source, TrashState,
};
use crate::motion;
use crate::palette::Palette;
use crate::reveal;
use crate::snapshot::{self, Forgotten, Recorder, Snapshots};
use crate::start::{self, Recent, ScanOptions};
use crate::stats::{Charts, Index, ReportStats, Selection, SelectionStats, Tally};
use crate::tab::Tab;
use crate::theme::{self, col, font, size, space, unit};
use crate::tooltip::{self, Tip, Tooltip};
use crate::ui::{self, ButtonKind};
use crate::zoom;

const POLL: Duration = Duration::from_millis(100);
const LIST_LIMIT: usize = 6;
const HISTORY_BARS: usize = 24;
const WHEEL_NOTCH: Pixels = px(20.);

const TOAST_TIME: Duration = Duration::from_secs(10);
const TOAST_PROBLEMS: usize = 3;
mod chrome;
mod clean;
mod duplicates;
mod flow;
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
mod ring;
mod session;
mod settings;
#[cfg(test)]
mod tests;

use keys::{found, found_path, keep_visible, shows_run};
use lists::{Line, Lists};
use map::SpaceMap;
use pane::{Chrome, Pane};
use repos::{GitJob, GitScope};
use ring::{Region, Stops};
use session::spawn_worker;

#[cfg(test)]
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
    trashed: usize,
    trashed_bytes: u64,
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
/// from it. Dropping it stops listening and cancels the search.
struct Search {
    roots: Vec<PathBuf>,
    excluded: usize,
    stage: Option<dupes::Stage>,
    cancel: Cancel,
    _task: Task<()>,
}

impl Drop for Search {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// A removal running beside the lists. Its rows are shown as leaving until it reports back.
struct Cleaning {
    paths: HashMap<Source, HashSet<PathBuf>>,
    started: Instant,
    _task: Task<()>,
}

enum Notice {
    Deleting(usize),
    Restoring { session: u64 },
    Restored(Restored),
    Failed(String),
    Info(String),
}

impl Notice {
    fn lingers(&self) -> bool {
        match self {
            Self::Info(_) => true,
            Self::Restored(restored) => {
                restored.problems.is_empty() && restored.history_error.is_none()
            }
            Self::Deleting(_) | Self::Restoring { .. } | Self::Failed(_) => false,
        }
    }
}

struct Toast {
    notice: Notice,
    shown: Duration,
}

enum Dialog {
    Finished(Finished),
    Branches {
        scope: GitScope,
        cancel: bool,
    },
    Exclude {
        path: PathBuf,
        pattern: String,
        cancel: bool,
    },
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
    enabled: bool,
    tip: Option<&'static str>,
    divided: bool,
}

impl MenuItem {
    fn new(choice: Choice, label: &'static str) -> Self {
        Self {
            choice,
            label,
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
    keymap: Keymap,
    roots: Vec<PathBuf>,
    options: ScanOptions,
    folder: String,
    picked_folder: Option<(String, PathBuf)>,
    whole_home: bool,
    collapsed: bool,
    folder_error: Option<String>,
    folder_check: Option<Task<()>>,
    recent: Recent,
    snapshots: Snapshots,
    forgotten: Forgotten,
    setting_writes: settings::SettingWrites,
    stale: Option<SystemTime>,
    pending: HashSet<Part>,
    early: Vec<Loaded>,
    streamed: bool,
    refreshed: Vec<Candidate>,
    progress: Activity,
    search: Option<Search>,
    search_error: Option<String>,
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
    rows_max: u64,
    path_room: Rc<Cell<f32>>,
    rows_filter: Option<RowsFilter>,
    rows_generation: u64,
    all_checked: Cell<Option<(u64, u64, bool)>>,
    row_hints: [Cell<usize>; Tab::ALL.len()],
    selection: Selection,
    index: Index,
    report_stats: ReportStats,
    report_generation: u64,
    reclaimable: Cell<Option<(u64, Reclaimable)>>,
    range: Option<Span>,
    visual: bool,
    count: String,
    asked: Option<keys::Asked>,
    dialog: Option<Dialog>,
    held: keys::Held,
    cleaning: Option<Cleaning>,
    flow: flow::Flow,
    gone: HashMap<Source, HashSet<PathBuf>>,
    history_fresh: bool,
    task: Option<Task<()>>,
    toast: Option<Toast>,
    toast_task: Option<Task<()>>,
    restore_task: Option<Task<()>>,
    menu: Option<Menu>,
    palette: Option<Palette>,
    palette_cache: RefCell<Option<(keys::PaletteKey, Rc<[keys::Entry]>)>>,
    shortcuts: bool,
    settings: Option<settings::Settings>,
    filtering: bool,
    filters: HashMap<Tab, String>,
    cursors: HashMap<Tab, Target>,
    cursor_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    artifact_scroll: UniformListScrollHandle,
    list_scrolls: HashMap<Tab, UniformListScrollHandle>,
    lists: Lists,
    stops: RefCell<Stops>,
    ring: Option<ElementId>,
    reveal_ring: Rc<Cell<bool>>,
    start_scroll: ScrollHandle,
    sidebar_scroll: ScrollHandle,
    shortcuts_scroll: ScrollHandle,
    config_path: Option<PathBuf>,
    excluded: Vec<PathBuf>,
    cached_excluded: usize,
    excluding: HashSet<PathBuf>,
    reveal: Reveal,
    map: SpaceMap,
    git: GitJob,
    git_task: Option<Task<()>>,
    branch_leaving: Option<repos::Leaving>,
    exclude_leaving: Option<menu::Leaving>,
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
        theme::configure(config.appearance.theme, config.appearance.accent);
        theme::set_density(config.appearance.density);
        let collapsed = config.appearance.sidebar == Sidebar::Collapsed;
        let mut app = Self {
            options: ScanOptions::from_config(&config),
            keymap: Keymap::resolve(&config.keys).0,
            config,
            roots,
            folder,
            picked_folder: None,
            whole_home: false,
            collapsed,
            folder_error: None,
            folder_check: None,
            recent,
            snapshots,
            forgotten: Forgotten::default(),
            setting_writes: Default::default(),
            stale: None,
            pending: HashSet::new(),
            early: Vec::new(),
            streamed: false,
            refreshed: Vec::new(),
            progress: Activity::default(),
            search: None,
            search_error: None,
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
            rows_max: 0,
            path_room: Rc::new(Cell::new(0.)),
            rows_filter: None,
            rows_generation: 0,
            all_checked: Cell::new(None),
            row_hints: Default::default(),
            selection: Selection::default(),
            index: Index::default(),
            report_stats: ReportStats::default(),
            report_generation: 0,
            reclaimable: Cell::new(None),
            range: None,
            visual: false,
            count: String::new(),
            asked: None,
            dialog: None,
            held: keys::Held::default(),
            cleaning: None,
            flow: flow::Flow::default(),
            gone: HashMap::new(),
            history_fresh: false,
            task: None,
            toast: None,
            toast_task: None,
            restore_task: None,
            menu: None,
            palette: None,
            palette_cache: RefCell::default(),
            shortcuts: false,
            settings: None,
            filtering: false,
            filters: HashMap::new(),
            cursors: HashMap::new(),
            cursor_bounds: Rc::new(Cell::new(None)),
            artifact_scroll: UniformListScrollHandle::new(),
            list_scrolls: [
                Tab::Caches,
                Tab::Toolchains,
                Tab::Docker,
                Tab::Repos,
                Tab::Duplicates,
                Tab::History,
                Tab::Issues,
            ]
            .into_iter()
            .map(|tab| (tab, UniformListScrollHandle::new()))
            .collect(),
            lists: Lists::default(),
            stops: RefCell::default(),
            ring: None,
            reveal_ring: Rc::new(Cell::new(false)),
            start_scroll: ScrollHandle::new(),
            sidebar_scroll: ScrollHandle::new(),
            shortcuts_scroll: ScrollHandle::new(),
            config_path: kuizer::config::default_path(),
            excluded: Vec::new(),
            cached_excluded: 0,
            excluding: HashSet::new(),
            reveal: reveal::reveal,
            map: SpaceMap::default(),
            git: GitJob::default(),
            git_task: None,
            branch_leaving: None,
            exclude_leaving: None,
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
        matches!(self.phase, Phase::Loading(_)) || self.restoring()
    }

    fn working(&self) -> bool {
        self.busy() || self.refreshing() || self.cleaning.is_some()
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
        self.report().is_some() && !self.working() && self.dialog.is_none()
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
        let pinned = matches!(
            self.toast,
            Some(Toast {
                notice: Notice::Deleting(_) | Notice::Restoring { .. },
                ..
            })
        );
        if self.toast.is_some() && !pinned {
            self.toast = None;
            cx.notify();
        }
    }

    fn quit(&mut self, _: &Quit, _window: &mut Window, cx: &mut Context<Self>) {
        if self.recording_keys() {
            cx.propagate();
            return;
        }
        cx.quit();
    }

    /// Whether a scan is still filling or replacing the results on screen.
    fn refreshing(&self) -> bool {
        self.stale.is_some() || (self.cancel.is_some() && matches!(self.phase, Phase::Ready(_)))
    }

    /// Whether a list still shows rows of an earlier scan. Those may not be removed until the
    /// running scan replaces them; rows the running scan found itself may.
    fn outdated(&self, source: Source) -> bool {
        if self.cancel.is_none() {
            return self.stale.is_some();
        }
        let part = match source {
            Source::Artifact => Part::Artifacts,
            Source::Cache => Part::Caches,
            Source::Toolchain => Part::Toolchains,
            Source::Docker => Part::Docker,
        };
        self.pending.contains(&part) && !(source == Source::Artifact && self.streamed)
    }

    /// Why items of these lists cannot be removed right now, if anything stands in the way.
    fn locked(&self, mut sources: impl Iterator<Item = Source>) -> Option<&'static str> {
        if self.restoring() {
            Some(WAIT_FOR_RESTORE)
        } else if self.cleaning.is_some() {
            Some(keys::WAIT_FOR_CLEANUP)
        } else if sources.any(|source| self.outdated(source)) {
            Some(
                "Available once the running scan replaces these results. They come from an earlier scan, so every item is checked again first",
            )
        } else {
            None
        }
    }

    fn refresh_rows(&mut self) {
        self.asked = None;
        self.rows_generation += 1;
        let Phase::Ready(report) = &self.phase else {
            self.rows.clear();
            self.rows_bytes = 0;
            self.rows_max = 0;
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
        (self.rows_bytes, self.rows_max) = self.rows.iter().fold((0, 0), |(sum, max), &index| {
            let bytes = report.candidates[index].usage.allocated;
            (sum + bytes, max.max(bytes))
        });
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
        self.flow.mark((source, id));
        cx.notify();
    }

    fn select_safe(&mut self, cx: &mut Context<Self>) {
        self.asked = None;
        for (key, safety) in self.list_items() {
            if safety == Safety::Safe {
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
        if self.recording_keys() {
            cx.propagate();
            return;
        }
        self.set_zoom(zoom::zoomed_in(self.config.appearance.zoom), cx);
    }

    fn zoom_out(&mut self, _: &ZoomOut, _window: &mut Window, cx: &mut Context<Self>) {
        if self.recording_keys() {
            cx.propagate();
            return;
        }
        self.set_zoom(zoom::zoomed_out(self.config.appearance.zoom), cx);
    }

    fn reset_zoom(&mut self, _: &ResetZoom, _window: &mut Window, cx: &mut Context<Self>) {
        if self.recording_keys() {
            cx.propagate();
            return;
        }
        self.set_zoom(kuizer::config::DEFAULT_ZOOM, cx);
    }

    fn wheel_zoom(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        self.wheel += event.delta.pixel_delta(WHEEL_NOTCH).y;
        if self.wheel.abs() < WHEEL_NOTCH {
            return;
        }
        let percent = if self.wheel > px(0.) {
            zoom::zoomed_in(self.config.appearance.zoom)
        } else {
            zoom::zoomed_out(self.config.appearance.zoom)
        };
        self.wheel = px(0.);
        self.set_zoom(percent, cx);
    }

    /// Shows a message unless the restore message is up, which tracks a running restore.
    fn tell(&mut self, notice: Notice, cx: &mut Context<Self>) {
        if !self.restoring() {
            self.show_toast(notice, cx);
        }
    }

    /// Why the selection, or the cursor row when nothing is selected, cannot be removed now.
    fn trash_lock(&self) -> Option<&'static str> {
        let stats = self.selection.stats();
        if stats.total().all.count > 0 {
            let selected = Source::ALL
                .into_iter()
                .filter(|source| stats.source(*source).all.count > 0);
            return self.locked(selected);
        }
        match self.cursor_target() {
            Some(Target::Item(source, ..)) => self.locked([source].into_iter()),
            _ => self.locked([].into_iter()),
        }
    }

    fn render_content(&self, cx: &mut Context<Self>) -> AnyElement {
        match &self.phase {
            Phase::Start => self.render_start(cx),
            Phase::Loading(activity) => self.render_loading(activity),
            Phase::Failed(error) => self.render_failed(error, cx),
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
                    Tab::Overview => self.render_overview(cx),
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
                    .child(page_header(self.tab, self.tab_bytes(self.tab)))
                    .children(pending)
                    .child(div().flex().flex_col().flex_1().min_h_0().child(list))
                    .into_any_element()
            }
        }
    }

    /// The failed scan: what went wrong, and the two ways on from it with their keys.
    fn render_failed(&self, error: &str, cx: &Context<Self>) -> AnyElement {
        let action = |id: &'static str,
                      label: &'static str,
                      kind: ButtonKind,
                      command: Command,
                      tip: &'static str| {
            let key = self.hint(&command);
            let reason = self.blocked(&command);
            self.button(
                id,
                label,
                kind,
                reason.is_none(),
                cx,
                move |app, window, cx| {
                    app.run(command.clone(), window, cx);
                },
            )
            .debug_selector(move || id.to_owned())
            .when(!key.is_empty(), |this| this.child(ui::kbd(key.clone())))
            .tip(
                match reason {
                    Some(reason) => Tooltip::new(reason),
                    None => Tooltip::new(tip).keys(&[&key]),
                }
                .build(),
            )
        };
        centered(ui::appear(
            "scan-failed",
            div()
                .debug_selector(|| "scan-failed".to_owned())
                .flex()
                .flex_col()
                .items_center()
                .gap(unit(space::L))
                .child(
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
                        .child(div().text_color(theme::danger()).child(error.to_owned())),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(unit(space::S))
                        .child(action(
                            "failed-rescan",
                            "Rescan",
                            ButtonKind::Secondary,
                            Command::Rescan,
                            "Scan the same folders again with the same options",
                        ))
                        .child(action(
                            "failed-new-scan",
                            "New scan",
                            ButtonKind::Ghost,
                            Command::NewScan,
                            "Choose another folder or change the scan options",
                        )),
                ),
        ))
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

    /// The strip that splits the project artifacts by category, and a chip per category that
    /// hides or shows it in the list.
    fn render_categories(&self, cx: &mut Context<Self>) -> Div {
        let categories: Vec<(usize, Category, u64)> = self
            .report_stats
            .categories
            .iter()
            .enumerate()
            .filter(|(_, (category, bytes))| *bytes > 0 || self.hidden.contains(category))
            .map(|(rank, (category, bytes))| (rank, *category, *bytes))
            .collect();
        let whole = categories.iter().map(|(_, _, bytes)| bytes).sum::<u64>();
        let tone = |rank: usize, shown: bool| {
            if !shown {
                return theme::track();
            }
            match rank {
                0 => theme::bar_strong(),
                1 => theme::bar_mid(),
                2 => theme::bar_soft(),
                _ => theme::bar_rest(),
            }
        };
        let strip = div()
            .flex()
            .gap(unit(space::XXS))
            .h(unit(size::STRIP))
            .children(categories.iter().filter(|(_, _, bytes)| *bytes > 0).map(
                |&(rank, category, bytes)| {
                    div()
                        .h_full()
                        .w(relative(theme::share(bytes, whole)))
                        .min_w(unit(size::MARKER))
                        .rounded(theme::radius_bar())
                        .bg(tone(rank, !self.hidden.contains(&category)))
                },
            ));
        let chips = categories.iter().map(|&(rank, category, bytes)| {
            let shown = !self.hidden.contains(&category);
            let chip = div()
                .id(category.as_str())
                .flex()
                .flex_none()
                .items_center()
                .gap(unit(space::S))
                .px(unit(space::S))
                .h(unit(size::SEGMENT))
                .rounded(theme::radius_control())
                .cursor_pointer()
                .text_size(unit(font::SMALL))
                .text_color(if shown {
                    theme::muted()
                } else {
                    theme::faint()
                })
                .hover(|style| style.bg(theme::hover()).text_color(theme::text()))
                .tip(tooltip::term(Term::category(category)))
                .child(
                    div()
                        .flex_none()
                        .size(unit(size::SWATCH))
                        .rounded(theme::radius_dot())
                        .when(shown, |this| this.bg(tone(rank, true)))
                        .when(!shown, |this| {
                            this.border_1().border_color(theme::border_strong())
                        }),
                )
                .child(theme::mono(div()).child(category.as_str()))
                .child(
                    ui::tabular(div())
                        .text_color(theme::faint())
                        .child(theme::bytes(bytes)),
                );
            self.control(chip, cx, move |app, _, cx| {
                if !app.hidden.remove(&category) {
                    app.hidden.insert(category);
                }
                app.refresh_rows();
                cx.notify();
            })
        });
        div()
            .flex()
            .flex_col()
            .flex_none()
            .gap(unit(space::S))
            .px(unit(space::GUTTER))
            .pt(unit(space::M))
            .child(strip)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(unit(space::XXS))
                    .ml(unit(-space::S))
                    .children(chips),
            )
    }

    fn render_artifacts(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.report().is_none() {
            return div().into_any_element();
        }
        let shown_bytes = self.rows_bytes;
        let all_checked = self.all_rows_checked();
        let toolbar = self
            .list_toolbar(cx)
            .child(div().flex_1())
            .child(self.tally(Tab::Artifacts, Some(shown_bytes)));
        let header = table_header()
            .child(
                self.control(
                    div()
                        .id("select-visible")
                        .debug_selector(|| "select-visible".to_owned())
                        .cursor_pointer()
                        .tip(tooltip::plain(
                            "Select the safe items, then the review items, then clear",
                        ))
                        .child(ui::checkbox(all_checked)),
                    cx,
                    |app, _, cx| {
                        if !app.busy() {
                            app.select_step(cx);
                        }
                    },
                ),
            )
            .child(
                head(col::XL, "Size", Term::Size)
                    .text_right()
                    .pr(unit(size::GAUGE + space::M))
                    .text_color(theme::muted()),
            )
            .child(head(col::S, "Safety", Term::Safety))
            .child(head(col::M, "Category", Term::Category))
            .child(head(col::L, "Rule", Term::Rule))
            .child(head(col::XS, "Changed", Term::Changed).text_right())
            .child(div().relative().flex_1().child("PATH").child({
                let room = Rc::clone(&self.path_room);
                canvas(
                    move |area, window, _| {
                        let width = f32::from(area.size.width);
                        if (room.get() - width).abs() > 0.5 {
                            room.set(width);
                            window.on_next_frame(|window, _| window.refresh());
                        }
                    },
                    |_, (), _, _| {},
                )
                .absolute()
                .inset_0()
            }));
        let list = uniform_list(
            "artifacts",
            self.rows.len(),
            cx.processor(|app, range: Range<usize>, window, cx| {
                let Some(report) = app.report() else {
                    return Vec::new();
                };
                let cursor = app.cursor_target();
                let largest = app.rows_max.max(1);
                range
                    .filter_map(|row| {
                        let candidate = &report.candidates[*app.rows.get(row)?];
                        let id = candidate.id;
                        let key = (Source::Artifact, id);
                        let checked = app.selection.contains(&key);
                        let review = candidate.rule.safety == Safety::Review;
                        let target = Target::Item(Source::Artifact, id, candidate.path.clone());
                        let look = app.look(Source::Artifact, id, &candidate.path);
                        let bright = checked || cursor.as_ref() == Some(&target);
                        let bytes = candidate.usage.allocated;
                        let figure = theme::bytes(bytes);
                        let (number, suffix) =
                            figure.split_once(' ').unwrap_or((figure.as_str(), ""));
                        let large = suffix.starts_with(['G', 'T']);
                        let amount = if bright || large {
                            theme::text()
                        } else if suffix.starts_with('M') {
                            theme::muted()
                        } else {
                            theme::faint()
                        };
                        let age = theme::age(candidate.newest);
                        let old = age.ends_with("mo") || age.ends_with('y');
                        let path = model::short_path(&candidate.path, &report.roots);
                        let cut = path.rfind('/').map_or(0, |at| at + 1);
                        let (folder, leaf) = path.split_at(cut);
                        let advance = f32::from(mono_width(
                            window,
                            "0",
                            zoom::rem_size(app.config.appearance.zoom),
                        ));
                        let (folder, leaf) = fit_path(folder, leaf, app.path_room.get(), advance);
                        let row = table_row(("artifact", id), checked)
                            .debug_selector(move || format!("artifact-row-{row}"));
                        Some(
                            app.menu_row(row, target, cursor.as_ref(), cx)
                                .child(if look.deleting {
                                    ui::deleting_mark()
                                } else {
                                    ui::check_mark(look.checked)
                                        .when(bright && look.checked < 0.5, |this| {
                                            this.border_color(theme::muted())
                                        })
                                })
                                .child(
                                    column(col::XL)
                                        .id(("artifact-size", id))
                                        .flex()
                                        .items_center()
                                        .justify_end()
                                        .gap(unit(space::M))
                                        .tip(Self::item_tip(key, cx, |candidate, report| {
                                            size_tooltip(candidate, &report.roots)
                                        }))
                                        .child(
                                            div()
                                                .flex()
                                                .items_baseline()
                                                .gap(unit(space::XS))
                                                .child(
                                                    div()
                                                        .text_color(amount)
                                                        .font_weight(if large {
                                                            FontWeight::SEMIBOLD
                                                        } else {
                                                            FontWeight::NORMAL
                                                        })
                                                        .child(number.to_owned()),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(unit(font::CAPTION))
                                                        .text_color(theme::faint())
                                                        .child(suffix.to_owned()),
                                                ),
                                        )
                                        .child(if checked {
                                            ui::gauge(
                                                theme::share(bytes, largest).sqrt(),
                                                theme::bar(),
                                                theme::tile_selected(),
                                            )
                                        } else {
                                            ui::gauge(
                                                theme::share(bytes, largest).sqrt(),
                                                ui::tone_of(bytes),
                                                theme::track(),
                                            )
                                        }),
                                )
                                .child(column(col::S).flex().child(if look.deleting {
                                    ui::tag(("artifact-safety", id), "deleting", ui::Tone::Danger)
                                } else {
                                    ui::safety_label(("artifact-safety", id), review)
                                        .when(bright && !review, |this| {
                                            this.text_color(theme::muted())
                                        })
                                }))
                                .child(
                                    theme::mono(column(col::M))
                                        .id(("artifact-category", id))
                                        .text_size(unit(font::SMALL))
                                        .text_color(theme::muted())
                                        .tip(tooltip::term(Term::category(candidate.rule.category)))
                                        .child(candidate.rule.category.as_str()),
                                )
                                .child(
                                    column(col::L)
                                        .id(("artifact-rule", id))
                                        .truncate()
                                        .font_weight(FontWeight::MEDIUM)
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
                                        .text_color(if old {
                                            theme::muted()
                                        } else {
                                            theme::faint()
                                        })
                                        .child(age),
                                )
                                .child(
                                    theme::mono(div())
                                        .id(("artifact-path", id))
                                        .flex()
                                        .flex_1()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .text_size(unit(font::SMALL))
                                        .when(look.deleting, |this| {
                                            this.line_through()
                                                .text_decoration_color(theme::faint())
                                        })
                                        .tip(Self::item_tip(key, cx, |candidate, _| {
                                            Tooltip::new(display_path(&candidate.path))
                                        }))
                                        .child(
                                            div()
                                                .flex_none()
                                                .text_color(if bright {
                                                    theme::muted()
                                                } else {
                                                    theme::faint()
                                                })
                                                .child(folder),
                                        )
                                        .child(
                                            div()
                                                .flex_none()
                                                .text_color(if bright {
                                                    theme::text()
                                                } else {
                                                    theme::muted()
                                                })
                                                .child(leaf),
                                        ),
                                )
                                .on_click(cx.listener(move |app, event: &ClickEvent, _, cx| {
                                    app.click_row(Source::Artifact, id, event, cx);
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
            .child(self.render_categories(cx))
            .child(toolbar)
            .child(table(
                header,
                if self.rows.is_empty() {
                    empty_list(
                        Tab::Artifacts,
                        self.report().map_or(0, |report| report.candidates.len()),
                        "No project artifacts found",
                        "No project artifacts match these filters",
                    )
                } else {
                    list.into_any_element()
                },
            ))
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
            return framed(
                toolbar,
                empty_list(
                    Tab::Issues,
                    0,
                    "The scan reported no issues",
                    "No issues match this filter",
                ),
            );
        }
        let count = self.listed(Tab::Issues).slots.len();
        let header = table_header().child(
            div()
                .flex_1()
                .child("Folders that could not be read or were skipped".to_uppercase()),
        );
        let list = self
            .table_list(
                "issues",
                Tab::Issues,
                count,
                |app, range, cx| app.issue_rows(range, cx),
                cx,
            )
            .debug_selector(|| "issue-list".to_owned());
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(toolbar)
            .child(table(
                header,
                if count == 0 {
                    empty_list(
                        Tab::Issues,
                        report.issues.len(),
                        "The scan reported no issues",
                        "No issues match this filter",
                    )
                } else {
                    list.into_any_element()
                },
            ))
            .into_any_element()
    }

    fn issue_rows(&self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<Stateful<Div>> {
        let Some(report) = self.report() else {
            return Vec::new();
        };
        let listed = self.listed(Tab::Issues);
        let cursor = self.cursor_target();
        range
            .filter_map(|row| {
                let index = listed.slots.get(row)?.0;
                let issue = report.issues.get(index)?;
                let line = div()
                    .id(("issue-row", index))
                    .flex()
                    .flex_col()
                    .justify_center()
                    .gap(unit(space::XXS))
                    .w_full()
                    .h(unit(size::row_tall()))
                    .px(unit(space::GUTTER))
                    .debug_selector(move || format!("issue-row-{row}"));
                Some(
                    self.menu_row(line, Target::Issue(issue.path.clone()), cursor.as_ref(), cx)
                        .child(
                            div()
                                .id(("issue", index))
                                .truncate()
                                .tip(tooltip::path(&issue.path))
                                .child(display_path(&issue.path)),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(unit(font::SMALL))
                                .text_color(theme::faint())
                                .child(issue.message.clone()),
                        ),
                )
            })
            .collect()
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
        window.set_rem_size(zoom::rem_size(self.config.appearance.zoom));
        self.animate_rows(window);
        tooltip::focus(self.ring.clone().filter(|_| !self.typing()), cx);
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
        self.region(Region::Toast);
        let toast = self.render_toast(cx);
        let menu = self.render_menu(cx);
        let palette = self.render_palette(cx);
        let shortcuts = self.render_shortcuts(cx);
        let settings = self.render_settings(cx);
        let start = matches!(self.phase, Phase::Start);
        self.region(Region::Titlebar);
        let titlebar = self.render_titlebar(window, cx);
        self.region(Region::Sidebar);
        let sidebar = (!start).then(|| self.render_sidebar(window, cx));
        self.region(Region::Statusbar);
        let statusbar = (!start).then(|| self.render_statusbar());
        let summary = if start { None } else { self.render_summary(cx) };
        let branches = if start {
            None
        } else {
            self.render_branch_confirm(cx)
        };
        let exclude = if start {
            None
        } else {
            self.render_exclude_confirm(cx)
        };
        let floating =
            (toast.is_some() || summary.is_some() || branches.is_some() || exclude.is_some()).then(
                || {
                    div()
                        .absolute()
                        .bottom(unit(space::L))
                        .left_0()
                        .right_0()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(unit(space::S))
                        .px(unit(space::XL))
                        .children(toast)
                        .children(branches)
                        .children(exclude)
                        .children(summary)
                },
            );
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
            .on_key_up(cx.listener(Self::on_key_up))
            .capture_any_mouse_down(cx.listener(|app, _: &MouseDownEvent, _, cx| {
                if app.ring.take().is_some() {
                    cx.notify();
                }
            }))
            .on_action(cx.listener(Self::zoom_in))
            .on_action(cx.listener(Self::zoom_out))
            .on_action(cx.listener(Self::reset_zoom))
            .on_action(cx.listener(Self::toggle_palette))
            .on_action(cx.listener(Self::quit))
            .child(wheel)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .pt(unit(space::SURFACE))
                    .when(sidebar.is_none(), |this| this.pl(unit(space::SURFACE)))
                    .children(sidebar)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .mr(unit(space::SURFACE))
                            .mb(unit(space::SURFACE))
                            .rounded(theme::radius_surface())
                            .bg(theme::panel())
                            .border_1()
                            .border_color(theme::border())
                            .overflow_hidden()
                            .child(titlebar)
                            .child(
                                div()
                                    .relative()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_h_0()
                                    .child(content)
                                    .when(self.working(), |this| {
                                        this.child(
                                            div().absolute().top_0().left_0().right_0().child(
                                                self.loader
                                                    .get_or_insert_with(|| {
                                                        cx.new(|_| chart::Loader)
                                                    })
                                                    .clone(),
                                            ),
                                        )
                                    })
                                    .children(floating),
                            )
                            .children(statusbar),
                    ),
            )
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

impl Kuizer {
    fn key_hint(&self, scope: Scope, action: Key) -> String {
        self.keymap
            .keys(scope, action)
            .next()
            .map(commands::label)
            .unwrap_or_default()
    }

    fn hint(&self, command: &Command) -> String {
        command
            .binding()
            .map_or_else(String::new, |(scope, action)| self.key_hint(scope, action))
    }
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

/// What a list shows in place of rows: `none` when the scan found nothing for it, `hidden`
/// when it found `total` rows and the filters hide every one of them.
fn empty_list(
    tab: Tab,
    total: usize,
    none: impl Into<SharedString>,
    hidden: &'static str,
) -> AnyElement {
    let (kind, text) = if total == 0 {
        ("none", none.into())
    } else {
        ("filtered", SharedString::from(hidden))
    };
    let name = format!("empty-{tab:?}-{kind}");
    let selector = name.clone();
    ui::appear(
        SharedString::from(name),
        div()
            .flex_1()
            .debug_selector(move || selector)
            .child(ui::empty(text)),
    )
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

/// The title of a section and one line on what it holds, above its toolbar, with the space the
/// section can free on the right.
fn page_header(tab: Tab, bytes: Option<u64>) -> Div {
    div()
        .debug_selector(|| "page-header".to_owned())
        .flex()
        .flex_none()
        .items_end()
        .gap(unit(space::XL))
        .px(unit(space::GUTTER))
        .pt(unit(space::section()))
        .pb(unit(space::XS))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap(unit(space::XXS))
                .child(
                    div()
                        .text_size(unit(font::HEADING))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(tab.label()),
                )
                .child(
                    div()
                        .text_size(unit(font::SMALL))
                        .text_color(theme::faint())
                        .child(tab.term().text()),
                ),
        )
        .children(bytes.filter(|&bytes| bytes > 0).map(|bytes| {
            div()
                .flex()
                .flex_col()
                .flex_none()
                .items_end()
                .child(ui::caption("Reclaimable"))
                .child(ui::amount(bytes, font::FIGURE))
        }))
}

/// A table: the column header on top and the rows filling what is left below it, edge to edge
/// in the content surface.
fn table(header: Div, body: AnyElement) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .child(header)
        .child(body)
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
        .child(label.to_uppercase())
}

fn table_header() -> Div {
    ui::tabular(theme::label(div()))
        .flex()
        .flex_none()
        .items_center()
        .gap(unit(space::M))
        .h(unit(size::row()))
        .px(unit(space::GUTTER))
        .border_t_1()
        .border_b_1()
        .border_color(theme::border())
        .text_color(theme::faint())
}

fn table_row(id: (&'static str, u64), checked: bool) -> gpui::Stateful<Div> {
    ui::tabular(div())
        .id(gpui::ElementId::NamedInteger(id.0.into(), id.1))
        .flex()
        .items_center()
        .gap(unit(space::M))
        .w_full()
        .h(unit(size::row()))
        .px(unit(space::GUTTER))
        .cursor_pointer()
        .when(checked, |this| this.bg(theme::selected()))
        .when(!checked, |this| {
            this.hover(|style| style.bg(theme::hover_row()))
        })
}

/// The width `text` takes in the monospace typeface at the small type size.
fn mono_width(window: &Window, text: &str, rem: Pixels) -> Pixels {
    let font = theme::mono_family().map_or_else(|| window.text_style().font(), gpui::font);
    let run = gpui::TextRun {
        len: text.len(),
        font,
        color: theme::text().into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let font_size = rem * (font::SMALL / theme::REM);
    window
        .text_system()
        .layout_line(text, font_size, &[run], None)
        .width
}

/// Cuts a path to the characters that fit in `room` pixels of a monospace face whose glyphs
/// are `advance` wide. The folder part gives way first and ends in an ellipsis; the last part
/// stays whole unless it alone is too long.
fn fit_path(folder: &str, leaf: &str, room: f32, advance: f32) -> (String, String) {
    if room <= 0. || advance <= 0. {
        return (folder.to_owned(), leaf.to_owned());
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a column holds a few hundred characters at most"
    )]
    let fits = (room / advance).floor() as usize;
    let cut = |text: &str, keep: usize| -> String {
        let mut kept: String = text.chars().take(keep).collect();
        kept.push('…');
        kept
    };
    let (folder_len, leaf_len) = (folder.chars().count(), leaf.chars().count());
    if folder_len + leaf_len <= fits {
        (folder.to_owned(), leaf.to_owned())
    } else if leaf_len + 1 < fits {
        (cut(folder, fits - leaf_len - 1), leaf.to_owned())
    } else {
        (String::new(), cut(leaf, fits.saturating_sub(1)))
    }
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
        details = details.row("Last change", kuizer::units::since(candidate.newest));
    }
    if let Some(project) = &candidate.project {
        details = details.row("Project", model::short_path(project, roots));
    }
    if candidate.project_last_active.is_some() {
        details = details.row(
            "Project active",
            kuizer::units::since(candidate.project_last_active),
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
