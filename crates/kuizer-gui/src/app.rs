use std::cell::Cell;
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
use crate::commands::{self, Command, ResetZoom, TogglePalette, Undo, ZoomIn, ZoomOut};
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
const UNDO_KEY: &str = if cfg!(target_os = "macos") {
    "Cmd+Z"
} else {
    "Ctrl+Z"
};
const SIDEBAR_KEY: &str = if cfg!(target_os = "macos") {
    "Cmd+B"
} else {
    "Ctrl+B"
};

mod keys;
mod map;
mod overlay;
mod pane;
mod repos;

use keys::{found, found_path, keep_visible, shows_run};
use map::SpaceMap;
use pane::Pane;
use repos::{GitJob, GitScope};

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
    range: Option<Span>,
    dialog: Option<Dialog>,
    task: Option<Task<()>>,
    toast: Option<Toast>,
    toast_task: Option<Task<()>>,
    restore_task: Option<Task<()>>,
    menu: Option<Menu>,
    palette: Option<Palette>,
    shortcuts: bool,
    filtering: bool,
    filters: HashMap<Tab, String>,
    cursors: HashMap<Tab, Target>,
    cursor_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    artifact_scroll: UniformListScrollHandle,
    scrolls: HashMap<Tab, ScrollHandle>,
    reveal_cursor: Rc<Cell<bool>>,
    config_path: Option<PathBuf>,
    excluded: Vec<PathBuf>,
    reveal: Reveal,
    map: SpaceMap,
    git: GitJob,
    git_task: Option<Task<()>>,
    content: Option<Entity<Pane>>,
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
        let unchecked = recent.paths.clone();
        let probed = cx.background_spawn(async move { Recent::probe(&unchecked) });
        cx.spawn(async move |this, cx| {
            let gone = probed.await;
            this.update(cx, |app, cx| app.mark_recent_gone(gone, cx))
                .ok();
        })
        .detach();
        cx.set_global(tooltip::Typeface(font.clone()));
        motion::configure(config.appearance.motion);
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
            range: None,
            dialog: None,
            task: None,
            toast: None,
            toast_task: None,
            restore_task: None,
            menu: None,
            palette: None,
            shortcuts: false,
            filtering: false,
            filters: HashMap::new(),
            cursors: HashMap::new(),
            cursor_bounds: Rc::new(Cell::new(None)),
            artifact_scroll: UniformListScrollHandle::new(),
            scrolls: [
                Tab::Caches,
                Tab::Toolchains,
                Tab::Docker,
                Tab::Repos,
                Tab::Duplicates,
                Tab::History,
                Tab::Issues,
            ]
            .into_iter()
            .map(|tab| (tab, ScrollHandle::new()))
            .collect(),
            reveal_cursor: Rc::new(Cell::new(false)),
            config_path: kuizer::config::default_path(),
            excluded: Vec::new(),
            reveal: reveal::reveal,
            map: SpaceMap::default(),
            git: GitJob::default(),
            git_task: None,
            content: None,
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

    fn mark_recent_gone(&mut self, gone: Vec<PathBuf>, cx: &mut Context<Self>) {
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
                    cx.notify();
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

    fn undo(&mut self, _: &Undo, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.typing() {
            self.undo_last(cx);
        }
    }

    fn undo_last(&mut self, cx: &mut Context<Self>) {
        if let Some(Toast {
            notice: Notice::Trashed { session, .. },
            ..
        }) = self.toast
        {
            self.restore(session, Scope::Undo, cx);
        }
    }

    fn restore(&mut self, session: u64, scope: Scope, cx: &mut Context<Self>) {
        if !self.can_restore() {
            return;
        }
        self.show_toast(Notice::Restoring { session }, cx);
        let (sender, receiver) = oneshot::channel();
        std::thread::Builder::new()
            .name("kuizer-gui-restore".to_owned())
            .spawn(move || {
                let restored = model::restore(session, scope);
                let history = HistoryView::load();
                let _ = sender.send((restored, history));
            })
            .expect("spawn restore thread");
        self.restore_task = Some(cx.spawn(async move |this, cx| {
            let message = receiver.await.ok();
            let _ = this.update(cx, |app, cx| app.restored(message, cx));
        }));
    }

    fn restored(
        &mut self,
        message: Option<(Result<Restored, String>, Result<HistoryView, String>)>,
        cx: &mut Context<Self>,
    ) {
        let Some((restored, history)) = message else {
            self.show_toast(
                Notice::Failed("Could not restore: the restore stopped unexpectedly".to_owned()),
                cx,
            );
            return;
        };
        if let Phase::Ready(report) = &mut self.phase {
            report.history = history;
        }
        self.refresh_stats();
        let notice = match restored {
            Ok(restored) => {
                if restored.items > 0 && matches!(self.phase, Phase::Ready(_)) {
                    self.rescan(cx);
                }
                Notice::Restored(restored)
            }
            Err(error) => Notice::Failed(format!("Could not restore: {error}")),
        };
        self.show_toast(notice, cx);
    }

    /// Whether the results on screen are still being replaced by a running scan, so nothing in
    /// them may be removed yet: some sections come from an earlier scan or are not in yet.
    fn refreshing(&self) -> bool {
        self.stale.is_some() || (self.cancel.is_some() && matches!(self.phase, Phase::Ready(_)))
    }

    fn rescan(&mut self, cx: &mut Context<Self>) {
        let shown = if let Phase::Ready(report) = &self.phase {
            self.stale = Some(report.scanned);
            true
        } else {
            self.set_phase(Phase::Loading(Box::default()), cx);
            self.stale = None;
            self.selection.clear();
            self.rows.clear();
            false
        };
        if let Some(cancel) = self.cancel.take() {
            cancel.cancel();
        }
        self.pending = Part::expected(&self.options);
        self.early.clear();
        self.streamed = false;
        self.progress = Activity::default();
        self.started = Instant::now();
        let cancel = Cancel::new();
        self.cancel = Some(cancel.clone());
        let (sender, mut receiver) = mpsc::unbounded();
        {
            let config = self.config.clone();
            let roots = self.roots.clone();
            let options = self.options.clone();
            let snapshots = self.snapshots.clone();
            std::thread::Builder::new()
                .name("kuizer-gui-scan".to_owned())
                .spawn(move || {
                    scan_parts(
                        &config, &roots, &options, &cancel, &snapshots, &sender, shown,
                    )
                })
                .expect("spawn scan thread");
        }
        self.task = Some(cx.spawn(async move |this, cx| {
            while let Some(first) = receiver.next().await {
                let mut updates = vec![first];
                while let Ok(update) = receiver.try_recv() {
                    updates.push(update);
                }
                let done = updates
                    .iter()
                    .any(|update| matches!(update, Update::Done(_)));
                let updated = this.update(cx, |app, cx| app.take(updates, cx));
                if done || updated.is_err() {
                    return;
                }
            }
            let _ = this.update(cx, |app, cx| {
                app.take(
                    vec![Update::Done(
                        Err("the scan stopped unexpectedly".to_owned()),
                    )],
                    cx,
                );
            });
        }));
    }

    /// Applies everything a running scan sent since the window last heard from it, then
    /// refreshes what depends on the sections that changed, once.
    fn take(&mut self, updates: Vec<Update>, cx: &mut Context<Self>) {
        let mut dropped = Vec::new();
        let (mut rows, mut map, mut sections) = (false, false, false);
        for update in updates {
            match update {
                Update::Activity(activity) => match &mut self.phase {
                    Phase::Loading(shown) => *shown = activity,
                    _ => self.progress = *activity,
                },
                Update::Cached(report) => {
                    if self.cancel.is_some() {
                        self.cached(*report, &mut dropped);
                        rows = true;
                        map = true;
                    }
                }
                Update::Loaded(loaded) => {
                    match &loaded {
                        Loaded::Found(_) => rows = true,
                        Loaded::Artifacts { .. } | Loaded::Caches(_) => {
                            rows = true;
                            map = true;
                        }
                        _ => sections = true,
                    }
                    self.loaded(loaded, &mut dropped);
                }
                Update::Done(Ok(scanned)) => {
                    if matches!(self.phase, Phase::Loading(_)) {
                        self.show_parts(&mut dropped);
                    }
                    if let Phase::Ready(report) = &mut self.phase {
                        report.scanned = scanned;
                    }
                    self.scan_over();
                    rows = true;
                    map = true;
                    if self.options.duplicates && self.search.is_none() {
                        self.find_duplicates(cx);
                    } else if self.tab == Tab::Duplicates {
                        self.load_duplicates(cx);
                    }
                }
                Update::Done(Err(error)) => {
                    self.scan_over();
                    let phase = if error == model::CANCELLED {
                        Phase::Start
                    } else {
                        Phase::Failed(error)
                    };
                    self.set_phase(phase, cx);
                }
            }
        }
        if rows || map || sections {
            if let Phase::Ready(report) = &mut self.phase {
                report.drop_paths(&self.excluded);
            }
            self.refresh_stats();
            if rows {
                self.refresh_rows();
            }
            if map || (rows && self.map.shown) {
                self.rebuild_map();
            }
            if self
                .menu
                .as_ref()
                .is_some_and(|menu| !self.known(&menu.target))
            {
                self.menu = None;
            }
        }
        if !dropped.is_empty() {
            cx.background_spawn(async move { drop(dropped) }).detach();
        }
        cx.notify();
    }

    fn scan_over(&mut self) {
        self.cancel = None;
        self.stale = None;
        self.pending.clear();
        self.early.clear();
        self.streamed = false;
    }

    /// Shows the saved scan of these folders while the fresh one runs. Before anything fresh
    /// is in, it is the whole report; after that it only fills the sections still pending.
    fn cached(&mut self, report: Report, dropped: &mut Vec<Loaded>) {
        if matches!(self.phase, Phase::Loading(_)) {
            if let Phase::Loading(activity) = &mut self.phase {
                self.progress = std::mem::take(&mut **activity);
            }
            self.stale = Some(report.scanned);
            self.adopt(report);
            self.apply_early(dropped);
            return;
        }
        let scanned = report.scanned;
        for part in report.into_parts() {
            let Some(kind) = part.part() else {
                continue;
            };
            let streaming = kind == Part::Artifacts
                && self.streamed
                && self
                    .report()
                    .is_some_and(|shown| !shown.candidates.is_empty());
            if !self.pending.contains(&kind) || streaming {
                dropped.push(part);
                continue;
            }
            if kind == Part::Artifacts {
                self.streamed = false;
            }
            self.stale = Some(scanned);
            self.replace(part, dropped);
        }
    }

    fn loaded(&mut self, loaded: Loaded, dropped: &mut Vec<Loaded>) {
        if matches!(self.phase, Phase::Loading(_)) {
            if !matches!(loaded, Loaded::Found(_) | Loaded::Artifacts { .. }) {
                self.early.push(loaded);
                return;
            }
            self.show_parts(dropped);
        }
        if let Loaded::Found(batch) = loaded {
            if let (true, Phase::Ready(report)) = (self.streamed, &mut self.phase) {
                dropped.push(report.apply(Loaded::Found(batch)));
            }
            return;
        }
        if let Some(part) = loaded.part() {
            self.pending.remove(&part);
            if part == Part::Artifacts {
                self.streamed = false;
            }
        }
        self.replace(loaded, dropped);
    }

    /// Leaves the activity view for the results once the first artifacts are in, with every
    /// part that arrived before them.
    fn show_parts(&mut self, dropped: &mut Vec<Loaded>) {
        if let Phase::Loading(activity) = &mut self.phase {
            self.progress = std::mem::take(&mut **activity);
        }
        self.phase = Phase::Ready(Box::new(Report::empty(&self.roots, &self.options)));
        self.streamed = true;
        self.apply_early(dropped);
    }

    fn apply_early(&mut self, dropped: &mut Vec<Loaded>) {
        for part in std::mem::take(&mut self.early) {
            if let Some(kind) = part.part() {
                self.pending.remove(&kind);
            }
            self.replace(part, dropped);
        }
    }

    /// Puts a part into its section of the report on screen and keeps the selection of that
    /// section on the same paths.
    fn replace(&mut self, loaded: Loaded, dropped: &mut Vec<Loaded>) {
        let Phase::Ready(report) = &mut self.phase else {
            dropped.push(loaded);
            return;
        };
        let source = loaded.part().and_then(Part::source);
        let kept: HashSet<PathBuf> = source.map_or_else(HashSet::new, |source| {
            report
                .section(source)
                .filter(|candidate| self.selection.contains(&(source, candidate.id)))
                .map(|candidate| candidate.path.clone())
                .collect()
        });
        dropped.push(report.apply(loaded));
        if let Some(source) = source {
            let stale: Vec<(Source, u64)> = self
                .selection
                .keys()
                .filter(|(selected, _)| *selected == source)
                .copied()
                .collect();
            for key in &stale {
                self.selection.remove(key);
            }
            for candidate in report
                .section(source)
                .filter(|candidate| kept.contains(&candidate.path))
            {
                self.selection.insert((source, candidate.id), candidate);
            }
        }
    }

    /// Changes the phase and drops the report it replaces off the UI thread.
    fn set_phase(&mut self, phase: Phase, cx: &mut Context<Self>) {
        if let Phase::Ready(report) = std::mem::replace(&mut self.phase, phase) {
            cx.background_spawn(async move { drop(report) }).detach();
        }
        if !matches!(self.phase, Phase::Ready(_)) {
            self.search = None;
        }
    }

    fn adopt(&mut self, report: Report) {
        let selection: HashSet<(Source, u64)> = {
            let kept: HashSet<(Source, &Path)> = self
                .selected()
                .into_iter()
                .map(|(source, candidate)| (source, candidate.path.as_path()))
                .collect();
            Source::ALL
                .into_iter()
                .flat_map(|source| {
                    report
                        .section(source)
                        .map(move |candidate| (source, candidate))
                })
                .filter(|(source, candidate)| kept.contains(&(*source, candidate.path.as_path())))
                .map(|(source, candidate)| (source, candidate.id))
                .collect()
        };
        self.selection = Selection::pending(selection);
        self.phase = Phase::Ready(Box::new(report));
        self.menu = None;
        self.prune();
    }

    /// @name Kuizer::find_duplicates
    /// @description Starts a duplicate search over the scanned folders beside the results on
    /// screen, and puts what it finds into the Duplicates section when it is done. Running it
    /// while a search is going stops listening to that search.
    ///
    /// @example
    /// app.find_duplicates(cx);
    fn find_duplicates(&mut self, cx: &mut Context<Self>) {
        if self.search.take().is_some() {
            cx.notify();
            return;
        }
        if self.report().is_none() {
            return;
        }
        self.search_error = None;
        let (sender, mut receiver) = mpsc::unbounded();
        let config = self.config.clone();
        let roots = self.roots.clone();
        let options = self.options.clone();
        let snapshots = self.snapshots.clone();
        std::thread::Builder::new()
            .name("kuizer-gui-duplicates".to_owned())
            .spawn(move || {
                let mut told: Option<Instant> = None;
                let found = model::find_duplicates(&config, &roots, &options, |stage| {
                    let quiet = matches!(stage, dupes::Stage::Walking { .. })
                        && told.is_some_and(|told| told.elapsed() < POLL);
                    if !quiet {
                        told = Some(Instant::now());
                        let _ = sender.unbounded_send(Searched::Stage(stage));
                    }
                });
                let staged = found.as_ref().ok().map(|found| {
                    let duplicate_options = dupes::Options::from_config(&options.apply(&config));
                    snapshots.stage_duplicates(&roots, duplicate_options, found)
                });
                let _ = sender.unbounded_send(Searched::Done(found));
                if let Some(staged) = staged {
                    staged.save();
                }
            })
            .expect("spawn duplicate search thread");
        let task = cx.spawn(async move |this, cx| {
            while let Some(first) = receiver.next().await {
                let mut last = first;
                while let Ok(next) = receiver.try_recv() {
                    last = next;
                }
                let done = matches!(last, Searched::Done(_));
                let updated = this.update(cx, |app, cx| app.searched(last, cx));
                if done || updated.is_err() {
                    return;
                }
            }
            let _ = this.update(cx, |app, cx| {
                app.searched(
                    Searched::Done(Err("the duplicate search stopped unexpectedly".to_owned())),
                    cx,
                );
            });
        });
        self.search = Some(Search {
            roots: self.roots.clone(),
            stage: None,
            _task: task,
        });
        cx.notify();
    }

    fn searched(&mut self, searched: Searched, cx: &mut Context<Self>) {
        let Some(search) = &mut self.search else {
            return;
        };
        match searched {
            Searched::Stage(stage) => search.stage = Some(stage),
            Searched::Done(found) => {
                let roots = std::mem::take(&mut search.roots);
                self.search = None;
                match found {
                    Ok(found) => self.show_duplicates(&roots, found, cx),
                    Err(error) => self.search_error = Some(error),
                }
            }
        }
        cx.notify();
    }

    fn show_duplicates(&mut self, roots: &[PathBuf], found: Duplicates, cx: &mut Context<Self>) {
        if let Phase::Ready(report) = &mut self.phase
            && report.roots == roots
        {
            if let Some(old) = report.duplicates.replace(found) {
                cx.background_spawn(async move { drop(old) }).detach();
            }
            report.drop_paths(&self.excluded);
        }
    }

    /// Reads the duplicates the last search under these folders found, off the UI thread, when
    /// the Duplicates section is opened and the report on screen has none yet.
    fn load_duplicates(&mut self, cx: &mut Context<Self>) {
        let Some(report) = self.report() else {
            return;
        };
        if report.duplicates.is_some() || self.search.is_some() {
            return;
        }
        let roots = report.roots.clone();
        let snapshots = self.snapshots.clone();
        let options = dupes::Options::from_config(&self.options.apply(&self.config));
        let (sender, receiver) = oneshot::channel();
        std::thread::Builder::new()
            .name("kuizer-gui-saved-duplicates".to_owned())
            .spawn(move || {
                let _ = sender.send(snapshots.load_duplicates(&roots, options));
            })
            .expect("spawn saved duplicates thread");
        let roots = self.roots.clone();
        cx.spawn(async move |this, cx| {
            if let Ok(Some(found)) = receiver.await {
                let _ = this.update(cx, |app, cx| {
                    if app.search.is_none()
                        && app
                            .report()
                            .is_some_and(|report| report.duplicates.is_none())
                    {
                        app.show_duplicates(&roots, found, cx);
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    fn start_scan(&mut self, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        let path = if self.whole_home {
            platform::home_dir()
        } else {
            start::expand(&self.folder)
        };
        let Some(path) = path else {
            self.folder_error = Some("Type a folder or choose one".to_owned());
            cx.notify();
            return;
        };
        let checked = cx.background_spawn(async move { start::check_folder(&path) });
        self.folder_check = Some(cx.spawn(async move |this, cx| {
            let result = checked.await;
            this.update(cx, |app, cx| match result {
                Ok(root) => {
                    app.folder_error = None;
                    app.roots = scan::normalize_roots(&[root]);
                    app.recent.remember(&app.roots);
                    app.rescan(cx);
                }
                Err(error) => {
                    app.folder_error = Some(error);
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    fn cancel_scan(&mut self, cx: &mut Context<Self>) {
        if let Some(cancel) = self.cancel.take() {
            cancel.cancel();
        }
        self.task = None;
        self.scan_over();
        self.set_phase(Phase::Start, cx);
        cx.notify();
    }

    fn new_scan(&mut self, cx: &mut Context<Self>) {
        if self.busy() || self.refreshing() {
            return;
        }
        self.selection.clear();
        self.rows.clear();
        self.toast = None;
        self.set_phase(Phase::Start, cx);
        cx.notify();
    }

    fn choose_folder(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Scan this folder".into()),
        });
        cx.spawn(async move |this, cx| {
            let picked = picked.await;
            let _ = this.update(cx, |app, cx| {
                match picked {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.first() {
                            app.folder = display_path(path);
                            app.whole_home = false;
                            app.folder_error = None;
                        }
                    }
                    Ok(Err(error)) => {
                        app.folder_error =
                            Some(format!("The folder picker is not available: {error}"));
                    }
                    Ok(Ok(None)) | Err(_) => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn pick_recent(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.folder = display_path(path);
        self.whole_home = false;
        self.folder_error = None;
        cx.notify();
    }

    fn type_folder(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.key == "enter" {
            self.start_scan(cx);
            return;
        }
        let paste = || cx.read_from_clipboard().and_then(|item| item.text());
        if !field::edit(&mut self.folder, keystroke, field::PATH, paste) {
            return;
        }
        self.whole_home = false;
        self.folder_error = None;
        cx.notify();
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
                && found(
                    filter.needle.as_deref(),
                    &[
                        candidate.path.to_string_lossy().as_ref(),
                        candidate.rule.name.as_str(),
                        candidate.rule.category.as_str(),
                    ],
                )
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
            Tab::Caches => report
                .caches
                .iter()
                .filter(|(cache, candidate)| self.shows_cache(cache, candidate))
                .filter(|(_, candidate)| candidate.rule.safety == Safety::Safe)
                .map(|(_, candidate)| (Source::Cache, candidate.id))
                .collect(),
            Tab::Toolchains => report
                .toolchains
                .iter()
                .filter(|(toolchain, candidate)| self.shows_toolchain(toolchain, candidate))
                .filter(|(_, candidate)| candidate.rule.safety == Safety::Safe)
                .map(|(_, candidate)| (Source::Toolchain, candidate.id))
                .collect(),
            Tab::Docker => report
                .docker
                .iter()
                .filter(|(item, candidate)| self.shows_docker(item, candidate))
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

    fn open_confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm(None, window, cx);
    }

    /// Opens the trash confirmation for the selection, or for one item when `only` is set.
    fn confirm(
        &mut self,
        only: Option<(Source, u64)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let items = match only {
            Some(key) => self.item(key).into_iter().collect(),
            None => self.selected(),
        };
        if items.is_empty() || self.busy() || self.refreshing() {
            return;
        }
        let rule = Confirmation::for_items(items.iter().map(|(_, candidate)| *candidate));
        let stats = SelectionStats::of(items.iter().copied());
        let preview = items
            .iter()
            .take(LIST_LIMIT)
            .map(|(source, candidate)| (*source, candidate.id))
            .collect();
        self.dialog = Some(Dialog::Confirm {
            rule,
            typed: String::new(),
            only,
            stats,
            preview,
        });
        window.focus(&self.focus);
        cx.notify();
    }

    fn item(&self, key: (Source, u64)) -> Option<(Source, &Candidate)> {
        let report = self.report()?;
        Some((key.0, self.index.find(report, key)?))
    }

    /// What the open confirmation acts on: one item from a context menu, or the selection.
    fn confirming(&self) -> Vec<(Source, &Candidate)> {
        match &self.dialog {
            Some(Dialog::Confirm {
                only: Some(key), ..
            }) => self.item(*key).into_iter().collect(),
            _ => self.selected(),
        }
    }

    fn accept(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::Confirm { rule, typed, .. }) = &self.dialog else {
            return;
        };
        if !rule.accepts(typed) || self.refreshing() {
            return;
        }
        let Some(report) = self.report() else {
            return;
        };
        let mut artifacts = Vec::new();
        let mut caches = Vec::new();
        let mut toolchains = Vec::new();
        let mut docker = Vec::new();
        for (source, candidate) in self.confirming() {
            match source {
                Source::Artifact => artifacts.push(candidate.clone()),
                Source::Cache => caches.push(candidate.clone()),
                Source::Toolchain => toolchains.push(candidate.clone()),
                Source::Docker => docker.push(candidate.clone()),
            }
        }
        let mut batches = Vec::new();
        if !artifacts.is_empty() {
            batches.push(Batch {
                source: Source::Artifact,
                roots: report.roots.clone(),
                candidates: artifacts,
            });
        }
        if !caches.is_empty() {
            batches.push(Batch {
                source: Source::Cache,
                roots: model::cache_roots(&caches),
                candidates: caches,
            });
        }
        if !toolchains.is_empty() {
            batches.push(Batch {
                source: Source::Toolchain,
                roots: kuizer::toolchains::roots(&toolchains),
                candidates: toolchains,
            });
        }
        if !docker.is_empty() {
            batches.push(Batch {
                source: Source::Docker,
                roots: Vec::new(),
                candidates: docker,
            });
        }
        let permanent = batches.iter().any(|batch| batch.source.permanent());
        self.dialog = Some(Dialog::Running { permanent });
        cx.notify();

        let (sender, receiver) = oneshot::channel();
        std::thread::Builder::new()
            .name("kuizer-gui-clean".to_owned())
            .spawn(move || {
                let outcome = model::clean(batches);
                let history = HistoryView::load();
                let _ = sender.send((outcome, history));
            })
            .expect("spawn trash thread");
        self.task = Some(cx.spawn(async move |this, cx| {
            let message = receiver.await.ok();
            let _ = this.update(cx, |app, cx| app.finish(message, cx));
        }));
    }

    fn finish(
        &mut self,
        message: Option<(Outcome, Result<HistoryView, String>)>,
        cx: &mut Context<Self>,
    ) {
        let Some((outcome, history)) = message else {
            self.dialog = Some(Dialog::Finished(Finished {
                removed: 0,
                removed_bytes: 0,
                deleted: 0,
                failures: vec![(String::new(), "the cleanup stopped unexpectedly".to_owned())],
                history_error: None,
            }));
            cx.notify();
            return;
        };
        let removed: HashSet<(Source, u64)> = outcome
            .removed()
            .map(|(source, removal)| (*source, removal.candidate.id))
            .collect();
        let finished = Finished {
            removed: removed.len(),
            removed_bytes: outcome
                .removed()
                .map(|(_, removal)| removal.candidate.usage.allocated)
                .sum(),
            deleted: outcome.deleted(),
            failures: outcome
                .failed()
                .map(|(_, removal)| {
                    (
                        display_path(&removal.candidate.path),
                        removal.error.clone().unwrap_or_default(),
                    )
                })
                .collect(),
            history_error: outcome.history_error.clone(),
        };
        if let Phase::Ready(report) = &mut self.phase {
            report
                .candidates
                .retain(|candidate| !removed.contains(&(Source::Artifact, candidate.id)));
            report
                .caches
                .retain(|(_, candidate)| !removed.contains(&(Source::Cache, candidate.id)));
            report
                .toolchains
                .retain(|(_, candidate)| !removed.contains(&(Source::Toolchain, candidate.id)));
            report
                .docker
                .retain(|(_, candidate)| !removed.contains(&(Source::Docker, candidate.id)));
            report.history = history;
        }
        for (source, removal) in &outcome.removals {
            if removal.status == Status::Removed {
                self.selection.remove(&(*source, removal.candidate.id));
            }
        }
        self.refresh_stats();
        self.refresh_rows();
        self.rebuild_map();
        let clean = finished.failures.is_empty()
            && finished.history_error.is_none()
            && finished.deleted == 0;
        let trashed: Vec<&Removal> = outcome
            .removed()
            .filter(|(source, _)| !source.permanent())
            .map(|(_, removal)| removal)
            .collect();
        if let Some(session) = outcome.session.filter(|_| !trashed.is_empty()) {
            self.show_toast(
                Notice::Trashed {
                    session,
                    items: trashed.len(),
                    bytes: trashed
                        .iter()
                        .map(|removal| removal.candidate.usage.allocated)
                        .sum(),
                },
                cx,
            );
        }
        self.dialog = (!clean || outcome.session.is_none()).then_some(Dialog::Finished(finished));
        cx.notify();
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

    fn menu_items(&self, target: &Target) -> Vec<MenuItem> {
        let reveal = MenuItem::new(Choice::Reveal, "Reveal in file manager", "R")
            .tip("Open the folder that holds this item");
        let copy = MenuItem::new(Choice::CopyPath, "Copy path", "C");
        let exclude = MenuItem::new(Choice::Exclude, "Always exclude", "X")
            .tip(EXCLUDE_TIP)
            .divided();
        match target {
            Target::Item(source, id, _) => {
                let selected = self.selection.contains(&(*source, *id));
                let external = *source == Source::Docker;
                let remove = if source.permanent() {
                    MenuItem::new(Choice::Trash, "Delete for good", "Del")
                        .tip("Delete only this item, after a confirmation. It skips the trash")
                } else {
                    MenuItem::new(Choice::Trash, "Move to trash", "Del")
                        .tip("Move only this item to the trash, after a confirmation")
                };
                vec![
                    reveal.unless(external.then_some(NOT_ON_DISK)),
                    copy,
                    MenuItem::new(
                        Choice::Toggle,
                        if selected { "Deselect" } else { "Select" },
                        "S",
                    )
                    .divided()
                    .unless(self.restoring().then_some(WAIT_FOR_RESTORE)),
                    remove.unless(self.trash_lock()),
                    exclude.unless(external.then_some(NOT_ON_DISK)),
                ]
            }
            Target::Duplicate(_) | Target::Issue(_) => vec![reveal, copy, exclude],
            Target::Repo(root) => {
                let scope = GitScope::One(root.clone());
                vec![
                    reveal.tip("Open the folder that holds this repository"),
                    copy,
                    MenuItem::new(Choice::Gc, "Run git gc", "G")
                        .tip("Pack loose objects now")
                        .divided()
                        .unless(self.gc_blocked(Some(&scope))),
                    MenuItem::new(Choice::Branches, "Delete merged branches", "B")
                        .tip("Delete the merged local branches, after a confirmation")
                        .unless(self.branches_blocked(Some(&scope))),
                    exclude,
                ]
            }
            Target::Run(session, _) => {
                let restorable = self.report().is_some_and(|report| {
                    report.history.as_ref().is_ok_and(|view| {
                        view.history
                            .runs
                            .iter()
                            .position(|run| run.id == *session)
                            .is_some_and(|index| {
                                matches!(
                                    self.report_stats.trash_state(report, index),
                                    TrashState::Restorable { .. }
                                )
                            })
                    })
                });
                let lock = if !restorable {
                    Some("Nothing from this cleanup is left in the trash")
                } else if self.restoring() {
                    Some(WAIT_FOR_RESTORE)
                } else if !self.can_restore() {
                    Some(WAIT_FOR_REFRESH)
                } else {
                    None
                };
                vec![
                    MenuItem::new(Choice::Restore, "Restore", "R")
                        .tip("Put what is still in the trash back where it was")
                        .unless(lock),
                    MenuItem::new(Choice::CopyCommand, "Copy command", "C"),
                ]
            }
        }
    }

    fn open_menu(
        &mut self,
        target: Target,
        position: Point<Pixels>,
        keyboard: bool,
        cx: &mut Context<Self>,
    ) {
        if self.dialog.is_some() || !self.known(&target) {
            return;
        }
        let active = if keyboard {
            self.menu_items(&target)
                .iter()
                .position(|item| item.enabled)
        } else {
            None
        };
        self.cursors.insert(target.tab(), target.clone());
        self.menu = Some(Menu {
            target,
            position,
            active,
        });
        cx.notify();
    }

    fn open_menu_at_cursor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.cursor_target() else {
            return;
        };
        let indent = unit(space::XXL).to_pixels(window.rem_size());
        let position = self.cursor_bounds.get().map_or_else(
            || {
                let size = window.viewport_size();
                point(size.width / 2., size.height / 3.)
            },
            |bounds| point(bounds.left() + indent, bounds.bottom()),
        );
        self.open_menu(target, position, true, cx);
    }

    fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.menu.take().is_some() {
            cx.notify();
        }
    }

    fn menu_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = &self.menu else {
            return;
        };
        let items = self.menu_items(&menu.target);
        let enabled: Vec<usize> = items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.enabled)
            .map(|(index, _)| index)
            .collect();
        let at = menu
            .active
            .and_then(|active| enabled.iter().position(|&index| index == active));
        let count = enabled.len();
        let keystroke = &event.keystroke;
        let plain = !keystroke.modifiers.control
            && !keystroke.modifiers.alt
            && !keystroke.modifiers.platform;
        let key = keystroke.key.as_str();
        let next = match key {
            "escape" | "menu" => {
                self.close_menu(cx);
                return;
            }
            "f10" if keystroke.modifiers.shift => {
                self.close_menu(cx);
                return;
            }
            "down" | "j" if count > 0 => Some(enabled[at.map_or(0, |at| (at + 1) % count)]),
            "up" | "k" if count > 0 => {
                Some(enabled[at.map_or(count - 1, |at| (at + count - 1) % count)])
            }
            "home" => enabled.first().copied(),
            "end" => enabled.last().copied(),
            "enter" | "space" => {
                if let Some(active) = menu.active {
                    self.choose(active, window, cx);
                }
                return;
            }
            _ => {
                if plain
                    && let Some(index) = items
                        .iter()
                        .position(|item| item.enabled && item.answers(key))
                {
                    self.choose(index, window, cx);
                }
                return;
            }
        };
        if let Some(menu) = &mut self.menu {
            menu.active = next;
        }
        cx.notify();
    }

    fn choose(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.menu.take() else {
            return;
        };
        let choice = self
            .menu_items(&menu.target)
            .get(index)
            .filter(|item| item.enabled)
            .map(|item| item.choice);
        let Some(choice) = choice else {
            self.menu = Some(menu);
            return;
        };
        cx.notify();
        let target = menu.target;
        let path = target.path().map(Path::to_path_buf);
        match (choice, target) {
            (Choice::Reveal, _) => {
                if let Some(path) = path {
                    self.reveal_path(path, cx);
                }
            }
            (Choice::CopyPath, _) => {
                if let Some(path) = path {
                    cx.write_to_clipboard(ClipboardItem::new_string(
                        path.to_string_lossy().into_owned(),
                    ));
                }
            }
            (Choice::Exclude, _) => {
                if let Some(path) = path {
                    self.exclude(path, cx);
                }
            }
            (Choice::Toggle, Target::Item(source, id, _)) => self.toggle(source, id, cx),
            (Choice::Trash, Target::Item(source, id, _)) => {
                self.confirm(Some((source, id)), window, cx);
            }
            (Choice::Restore, Target::Run(session, _)) => {
                self.restore(session, Scope::StillInTrash, cx);
            }
            (Choice::CopyCommand, Target::Run(_, command)) => {
                cx.write_to_clipboard(ClipboardItem::new_string(command));
            }
            (Choice::Gc, Target::Repo(root)) => self.collect_garbage(&GitScope::One(root), cx),
            (Choice::Branches, Target::Repo(root)) => {
                self.ask_branches(GitScope::One(root), cx);
            }
            _ => {}
        }
    }

    /// Opens the file manager on a worker so a slow or missing file manager never blocks the
    /// window, and explains a failure in the message bar.
    fn reveal_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let open = self.reveal;
        let opened = cx.background_executor().spawn(async move { open(&path) });
        cx.spawn(async move |this, cx| {
            if let Err(error) = opened.await {
                let _ = this.update(cx, |app, cx| {
                    app.tell(
                        Notice::Failed(format!("Could not open the file manager: {error}")),
                        cx,
                    );
                });
            }
        })
        .detach();
    }

    /// Adds the path to `scan.exclude` in the config file, so no later scan shows it, and
    /// removes it from the lists on screen.
    fn exclude(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let shown = self.shown_path(&path);
        let pattern = kuizer::config::exclude_pattern(&path);
        let written = match &self.config_path {
            Some(file) => {
                kuizer::config::add_exclude(file, &pattern).map_err(|error| error.to_string())
            }
            None => Err("this system has no config folder".to_owned()),
        };
        if let Err(error) = written {
            self.tell(
                Notice::Failed(format!("Could not exclude {shown}: {error}")),
                cx,
            );
            return;
        }
        if !self.config.scan.exclude.contains(&pattern) {
            self.config.scan.exclude.push(pattern);
        }
        self.excluded.push(path);
        self.prune();
        self.tell(
            Notice::Info(format!("{shown} is excluded from every scan")),
            cx,
        );
        cx.notify();
    }

    /// Removes excluded paths from the report on screen and forgets selections of items that
    /// are gone.
    fn prune(&mut self) {
        if let Phase::Ready(report) = &mut self.phase {
            report.drop_paths(&self.excluded);
        }
        self.refresh_stats();
        self.refresh_rows();
        self.rebuild_map();
        if self
            .menu
            .as_ref()
            .is_some_and(|menu| !self.known(&menu.target))
        {
            self.menu = None;
        }
    }

    /// Whether a menu target is still on screen: a row of its list, or an item of the report
    /// that the space map shows whatever the list filters are.
    fn known(&self, target: &Target) -> bool {
        self.target_exists(target)
            || matches!(target, Target::Item(source, id, _) if self.item((*source, *id)).is_some())
    }

    /// Adds the right-click menu to a row, and marks the row the keyboard menu would open for.
    fn menu_row<E: InteractiveElement + ParentElement + Styled + FluentBuilder>(
        &self,
        row: E,
        target: Target,
        cursor: Option<&Target>,
        cx: &mut Context<Self>,
    ) -> E {
        let open = self.menu.as_ref().is_some_and(|menu| menu.target == target);
        let tab = target.tab();
        let marked = cursor == Some(&target);
        let bounds = marked.then(|| Rc::clone(&self.cursor_bounds));
        let reveal = Rc::clone(&self.reveal_cursor);
        let scroll = self.scrolls.get(&tab).cloned();
        let clicked = target.clone();
        row.when_some(bounds, |this, bounds| {
            this.child(
                canvas(
                    move |area, window, _| {
                        bounds.set(Some(area));
                        if let Some(scroll) = scroll
                            && reveal.take()
                        {
                            keep_visible(&scroll, area, window);
                        }
                    },
                    |_, (), _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .child(ui::marker().debug_selector(|| "cursor".to_owned()))
        })
        .when(open, |this| this.bg(theme::hover()))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |app, _: &MouseDownEvent, _, cx| {
                app.cursors.insert(tab, clicked.clone());
                app.range = None;
                app.filtering = false;
                cx.notify();
            }),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |app, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                app.open_menu(target.clone(), event.position, false, cx);
            }),
        )
    }

    fn render_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.menu.as_ref()?;
        let items = self.menu_items(&menu.target);
        let active = menu.active;
        let panel = div()
            .id("menu")
            .debug_selector(|| "menu".to_owned())
            .flex()
            .flex_col()
            .min_w(unit(size::MENU))
            .py(unit(space::XS))
            .rounded(theme::radius_panel())
            .bg(theme::elevated())
            .border_1()
            .border_color(theme::border_strong())
            .text_size(unit(font::BODY))
            .font_family(self.font.clone())
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .children(items.into_iter().enumerate().map(|(index, item)| {
                let label = item.label;
                let enabled = item.enabled;
                let divider = item.divided.then(|| {
                    div()
                        .h(unit(size::HAIR))
                        .my(unit(space::XS))
                        .bg(theme::border())
                });
                let row = div()
                    .id(("menu-item", index))
                    .debug_selector(move || format!("menu-{label}"))
                    .flex()
                    .items_center()
                    .gap(unit(space::XL))
                    .h(unit(size::CONTROL))
                    .mx(unit(space::XS))
                    .px(unit(space::M))
                    .rounded(theme::radius_control())
                    .text_color(if enabled {
                        theme::text()
                    } else {
                        theme::faint()
                    })
                    .when(enabled && active == Some(index), |this| {
                        this.bg(theme::selected())
                    })
                    .when(enabled, |this| this.cursor_pointer())
                    .when_some(item.tip, |this, tip| this.tip(tooltip::plain(tip)))
                    .child(div().flex_1().child(label))
                    .child(ui::kbd(item.key))
                    .on_mouse_move(cx.listener(move |app, _: &MouseMoveEvent, _, cx| {
                        if let Some(menu) = &mut app.menu
                            && enabled
                            && menu.active != Some(index)
                        {
                            menu.active = Some(index);
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |app, _: &ClickEvent, window, cx| {
                        app.choose(index, window, cx);
                    }));
                div().flex().flex_col().children(divider).child(row)
            }));
        Some(
            div()
                .id("menu-backdrop")
                .absolute()
                .inset_0()
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|app, _: &MouseDownEvent, _, cx| app.close_menu(cx)),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(|app, _: &MouseDownEvent, _, cx| app.close_menu(cx)),
                )
                .child(
                    deferred(
                        anchored()
                            .position(menu.position)
                            .snap_to_window_with_margin(px(8.))
                            .child(panel),
                    )
                    .with_priority(1),
                )
                .into_any_element(),
        )
    }

    fn dialog_path(&self, candidate: &Candidate) -> String {
        self.shown_path(&candidate.path)
    }

    fn render_titlebar(&self, window: &Window, cx: &mut Context<Self>) -> gpui::Stateful<Div> {
        let roots = if matches!(self.phase, Phase::Start) {
            "Choose what to scan".to_owned()
        } else {
            self.roots
                .iter()
                .map(|root| display_path(root))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let busy = self.busy();
        let refreshing = self.refreshing();
        let scanning = matches!(self.phase, Phase::Loading(_)) || refreshing;
        let finished = matches!(self.phase, Phase::Ready(_) | Phase::Failed(_)) && !refreshing;
        let stale = self.stale.map(|saved| {
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(unit(space::S))
                .debug_selector(|| "stale".to_owned())
                .child(
                    ui::tag(
                        "stale-age",
                        snapshot::age_label(saved, SystemTime::now()),
                        ui::Tone::Review,
                    )
                    .tip(tooltip::plain(
                        "These results come from the last scan of these folders. A fresh scan is running and replaces them when it finishes",
                    )),
                )
                .child(
                    ui::tag("refreshing", "Refreshing", ui::Tone::Neutral)
                        .tip(Tooltip::new("Stop the fresh scan").keys(&["Esc"]).build()),
                )
        });
        let controls = frame::controls(window);
        let custom = controls.is_some();
        let aligned = !matches!(self.phase, Phase::Start)
            && !self.collapsed
            && frame::inset(window).is_none();
        let actions = div()
            .debug_selector(|| "titlebar-actions".to_owned())
            .flex()
            .flex_none()
            .items_center()
            .gap(unit(space::L))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .when(scanning, |this| {
                this.child(
                    ui::button("cancel-scan", "Cancel scan", ButtonKind::Secondary, true)
                        .tip(
                            Tooltip::new("Stop the scan and go back to the start screen")
                                .keys(&["Esc"])
                                .build(),
                        )
                        .on_click(cx.listener(|app, _: &ClickEvent, _, cx| {
                            app.cancel_scan(cx);
                        })),
                )
            })
            .when(finished, |this| {
                this.child(
                    ui::button("new-scan", "New scan", ButtonKind::Ghost, !busy)
                        .tip(tooltip::plain(
                            "Choose another folder or change the scan options",
                        ))
                        .on_click(cx.listener(|app, _: &ClickEvent, _, cx| {
                            app.new_scan(cx);
                        })),
                )
                .child(
                    ui::button("rescan", "Rescan", ButtonKind::Secondary, !busy)
                        .tip(tooltip::plain(
                            "Scan the same folders again with the same options",
                        ))
                        .on_click(cx.listener(|app, _: &ClickEvent, _, cx| {
                            if !app.busy() {
                                app.rescan(cx);
                            }
                        })),
                )
            });
        div()
            .id("titlebar")
            .debug_selector(|| "titlebar".to_owned())
            .flex()
            .flex_none()
            .items_center()
            .gap(unit(space::L))
            .h(unit(size::BAR))
            .min_h(frame::TITLEBAR)
            .px(unit(space::GUTTER))
            .when_some(frame::inset(window), |this, inset| this.pl(inset))
            .when(custom, |this| this.pr(unit(space::S)))
            .bg(theme::panel())
            .border_b_1()
            .border_color(theme::border())
            .on_mouse_down(MouseButton::Left, cx.listener(Self::titlebar_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::titlebar_up))
            .on_mouse_move(cx.listener(Self::titlebar_move))
            .when(custom, |this| {
                this.on_mouse_down(MouseButton::Right, |event, window, _| {
                    window.show_window_menu(event.position);
                })
            })
            .child(
                div()
                    .flex()
                    .flex_1()
                    .gap(unit(space::L))
                    .child(
                        div()
                            .debug_selector(|| "brand".to_owned())
                            .font_weight(FontWeight::SEMIBOLD)
                            .when(aligned, |this| {
                                this.flex_basis(unit(size::SIDEBAR - space::L))
                            })
                            .child("kuizer"),
                    )
                    // Absolute, so the path never sets how narrow this side can get.
                    .child(
                        div().relative().flex_1().child(
                            div()
                                .id("roots")
                                .absolute()
                                .inset_0()
                                .truncate()
                                .text_color(theme::muted())
                                .when(!matches!(self.phase, Phase::Start), |this| {
                                    this.tip(tooltip::plain(roots.clone()))
                                })
                                .child(roots),
                        ),
                    ),
            )
            .child(self.render_search(cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .items_center()
                    .justify_end()
                    .gap(unit(space::L))
                    .children(stale)
                    .child(actions)
                    .children(controls),
            )
    }

    fn titlebar_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        self.moving = frame::titlebar_down(event.click_count, window);
    }

    fn titlebar_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.moving = false;
    }

    fn titlebar_move(&mut self, _: &MouseMoveEvent, window: &mut Window, _: &mut Context<Self>) {
        if self.moving {
            self.moving = false;
            window.start_window_move();
        }
    }

    fn tab_count(&self, tab: Tab) -> Option<usize> {
        let report = self.report()?;
        match tab {
            Tab::Overview => None,
            Tab::Artifacts => Some(report.candidates.len()),
            Tab::Caches => Some(report.caches.len()),
            Tab::Toolchains => Some(report.toolchains.len()),
            Tab::Docker => Some(report.docker.len()),
            Tab::Repos => Some(report.repos.len()),
            Tab::Duplicates => report.duplicates.as_ref().map(|found| found.groups.len()),
            Tab::History => report
                .history
                .as_ref()
                .ok()
                .map(|view| view.delete_runs().count()),
            Tab::Issues => Some(report.issues.len()),
        }
    }

    /// The space a section can free, shown next to it in the sidebar.
    fn tab_bytes(&self, tab: Tab) -> Option<u64> {
        let report = self.report()?;
        let bytes = match tab {
            Tab::Overview | Tab::History | Tab::Issues => return None,
            Tab::Artifacts => self.report_stats.artifacts.all.bytes,
            Tab::Caches => self.report_stats.caches.all.bytes,
            Tab::Toolchains => self.report_stats.toolchains.all.bytes,
            Tab::Docker => self.report_stats.docker.all.bytes,
            Tab::Repos => report
                .repos
                .iter()
                .filter(|repository| repository.collectable(self.config.git.min_gain.0))
                .map(|repository| repository.objects.gain())
                .sum(),
            Tab::Duplicates => report
                .duplicates
                .as_ref()
                .map_or(0, kuizer::dupes::Duplicates::reclaimable),
        };
        Some(bytes)
    }

    /// What the sidebar shows to the right of a section: the space it can free, or for history
    /// and issues how many entries there are.
    fn tab_figure(&self, tab: Tab) -> Option<String> {
        match self.tab_bytes(tab) {
            Some(bytes) if bytes > 0 => Some(theme::bytes(bytes)),
            _ => self
                .tab_count(tab)
                .filter(|&count| count > 0)
                .map(|count| count.to_string()),
        }
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let collapsed = self.collapsed;
        let mut items: Vec<AnyElement> = Vec::new();
        for tab in Tab::ALL {
            let heading = tab.heading().map(|heading| {
                if collapsed {
                    ui::divider().mx(unit(space::M)).into_any_element()
                } else {
                    div()
                        .flex()
                        .items_end()
                        .h(unit(size::ROW))
                        .px(unit(space::M))
                        .pb(unit(space::S))
                        .text_size(unit(font::CAPTION))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::faint())
                        .child(heading)
                        .into_any_element()
                }
            });
            items.extend(heading);
            items.push(self.render_tab(tab, cx).into_any_element());
        }
        let toggle = div()
            .id("sidebar-toggle")
            .debug_selector(|| "sidebar-toggle".to_owned())
            .flex()
            .flex_none()
            .items_center()
            .gap(unit(space::M))
            .h(unit(size::ROW))
            .px(unit(space::M))
            .rounded(theme::radius_control())
            .cursor_pointer()
            .text_color(theme::faint())
            .hover(|style| style.bg(theme::hover()).text_color(theme::muted()))
            .tip_hover(
                Tooltip::new(if collapsed {
                    "Show section names"
                } else {
                    "Show only icons"
                })
                .keys(&[SIDEBAR_KEY])
                .build(),
                cx.listener(|app, hovered: &bool, _, cx| {
                    app.hover_icon(Icon::Sidebar, *hovered, cx);
                }),
            )
            .child(icon::view(
                Icon::Sidebar,
                theme::faint(),
                self.icon_epoch(Icon::Sidebar),
                self.zoom.rem_size(),
            ))
            .when(!collapsed, |this| this.child("Collapse"))
            .on_click(cx.listener(|app, _: &ClickEvent, _, cx| {
                app.collapsed = !app.collapsed;
                cx.notify();
            }));
        div()
            .id("sidebar")
            .debug_selector(|| "sidebar".to_owned())
            .flex()
            .flex_col()
            .flex_none()
            .w(unit(if collapsed {
                size::SIDEBAR_COLLAPSED
            } else {
                size::SIDEBAR
            }))
            .py(unit(space::M))
            .px(unit(space::INSET))
            .bg(theme::panel())
            .border_r_1()
            .border_color(theme::border())
            .child(
                div()
                    .id("sidebar-items")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(items),
            )
            .child(toggle)
    }

    fn hover_icon(&mut self, icon: Icon, hovered: bool, cx: &mut Context<Self>) {
        if hovered && !motion::reduced() {
            *self.icon_hovers.entry(icon.name()).or_default() += 1;
            cx.notify();
        }
    }

    fn icon_epoch(&self, icon: Icon) -> Option<u64> {
        self.icon_hovers.get(icon.name()).copied()
    }

    fn render_tab(&self, tab: Tab, cx: &mut Context<Self>) -> Stateful<Div> {
        let active = self.tab == tab;
        let collapsed = self.collapsed;
        let figure = self.tab_figure(tab);
        let tip = Tooltip::new(tab.term().text())
            .title(match (collapsed, &figure) {
                (true, Some(figure)) => format!("{}  {figure}", tab.label()),
                _ => tab.label().to_owned(),
            })
            .keys(tab.keys())
            .build();
        div()
            .id(tab.label())
            .debug_selector(|| tab.label().to_owned())
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .gap(unit(space::M))
            .h(unit(size::ROW))
            .px(unit(space::M))
            .rounded(theme::radius_control())
            .cursor_pointer()
            .text_color(if active {
                theme::text()
            } else {
                theme::muted()
            })
            .when(active, |this| {
                this.bg(theme::selected()).font_weight(FontWeight::MEDIUM)
            })
            .when(!active, |this| {
                this.hover(|style| style.bg(theme::hover()).text_color(theme::text()))
            })
            .tip_hover(
                tip,
                cx.listener(move |app, hovered: &bool, _, cx| {
                    app.hover_icon(tab.icon(), *hovered, cx);
                }),
            )
            .child(icon::view(
                tab.icon(),
                if active {
                    theme::text()
                } else {
                    theme::muted()
                },
                self.icon_epoch(tab.icon()),
                self.zoom.rem_size(),
            ))
            .when(!collapsed, |this| {
                this.child(div().flex_1().min_w_0().truncate().child(tab.label()))
                    .children(figure.map(|figure| {
                        ui::figure(figure)
                            .text_size(unit(font::SMALL))
                            .font_weight(FontWeight::NORMAL)
                            .text_color(if active {
                                theme::muted()
                            } else {
                                theme::faint()
                            })
                    }))
            })
            .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                app.go_to(tab, cx);
            }))
    }

    fn render_statusbar(&self, cx: &mut Context<Self>) -> Div {
        let stats = self.selection.stats();
        let total = stats.total();
        let empty = total.all.count == 0;
        let review = total.review.count;
        let summary = if empty {
            "Nothing selected".to_owned()
        } else {
            format!(
                "{} selected, {}",
                total.all.count,
                theme::bytes(total.all.bytes)
            )
        };
        let ready = self.report().is_some() && !self.busy();
        let refreshing = self.refreshing();
        let can_select = ready && self.tab.selects();
        let permanent = stats.permanent() > 0;
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(unit(space::M))
            .h(unit(size::BAR))
            .px(unit(space::GUTTER))
            .bg(theme::panel())
            .border_t_1()
            .border_color(theme::border())
            .child(
                div()
                    .id("selection")
                    .when(!empty, |this| this.tip(selection_details(stats)))
                    .child(summary),
            )
            .when(review > 0, |this| {
                this.child(
                    div()
                        .id("marked-review")
                        .text_color(theme::review())
                        .tip(tooltip::term(Term::Review))
                        .child(format!("{review} marked review")),
                )
            })
            .child(div().flex_1())
            .child(
                ui::button("select-safe", "Select safe", ButtonKind::Ghost, can_select)
                    .tip(tooltip::term(Term::SelectSafe))
                    .on_click(cx.listener(|app, _: &ClickEvent, _, cx| {
                        if app.tab.selects() && !app.busy() {
                            app.select_safe(cx);
                        }
                    })),
            )
            .child(
                ui::button("clear", "Clear", ButtonKind::Ghost, ready && !empty)
                .tip(tooltip::plain("Clear the selection in every tab"))
                .on_click(cx.listener(|app, _: &ClickEvent, _, cx| {
                    if !app.busy() {
                        app.selection.clear();
                        cx.notify();
                    }
                })),
            )
            .child(
                ui::button(
                    "trash",
                    if permanent { "Remove" } else { "Move to trash" },
                    ButtonKind::Primary,
                    ready && !refreshing && !empty,
                )
                .when(refreshing, |this| {
                    this.tip(tooltip::plain(
                        "Available once the fresh scan finishes. These results come from an earlier scan, so every item is checked again first",
                    ))
                })
                .when(!refreshing && !permanent, |this| this.tip(tooltip::term(Term::Trash)))
                .when(!refreshing && permanent, |this| {
                    this.tip(tooltip::plain(
                        "Toolchains and Docker objects in the selection are deleted for good, the rest moves to the trash",
                    ))
                })
                .on_click(cx.listener(|app, _: &ClickEvent, window, cx| {
                    app.open_confirm(window, cx);
                })),
            )
    }

    fn render_content(&mut self, cx: &mut Context<Self>) -> AnyElement {
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
                    .child(self.render_filter(cx))
                    .children(pending)
                    .child(div().flex().flex_col().flex_1().min_h_0().child(list))
                    .into_any_element()
            }
        }
    }

    fn render_start(&self, cx: &mut Context<Self>) -> AnyElement {
        let home = platform::home_dir();
        let field_text = if self.whole_home {
            home.as_deref().map(display_path).unwrap_or_default()
        } else {
            self.folder.clone()
        };
        let field = div()
            .id("folder-field")
            .flex()
            .items_center()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .h(unit(size::INPUT))
            .px(unit(space::L))
            .rounded(theme::radius_control())
            .bg(theme::background())
            .border_1()
            .border_color(if self.folder_error.is_some() {
                theme::danger()
            } else if self.whole_home {
                theme::border_strong()
            } else {
                theme::focus()
            })
            .cursor(CursorStyle::IBeam)
            .tip(
                Tooltip::details("Keyboard")
                    .row("Enter", "start the scan")
                    .row("Ctrl+V", "paste a path")
                    .row("Ctrl+Backspace", "remove the last folder")
                    .row("Ctrl+U", "clear the field")
                    .build(),
            )
            .when(field_text.is_empty(), |this| {
                this.child(
                    div()
                        .w(unit(size::HAIR))
                        .h(unit(size::CARET))
                        .bg(theme::text()),
                )
                .child(
                    div()
                        .pl(unit(space::XXS))
                        .text_color(theme::faint())
                        .child("Type a path, for example ~/apps"),
                )
            })
            .when(!field_text.is_empty(), |this| {
                this.child(
                    div()
                        .flex_none()
                        .text_color(if self.whole_home {
                            theme::muted()
                        } else {
                            theme::text()
                        })
                        .child(field_text),
                )
                .when(!self.whole_home, |this| {
                    this.child(
                        div()
                            .flex_none()
                            .w(unit(size::HAIR))
                            .h(unit(size::CARET))
                            .bg(theme::text()),
                    )
                })
            })
            .on_click(cx.listener(|app, _: &ClickEvent, window, cx| {
                window.focus(&app.focus);
                app.whole_home = false;
                cx.notify();
            }));
        let scope = ui::segments()
            .child(
                ui::segment("scope-folder", "One folder", !self.whole_home)
                    .tip(tooltip::plain("Scan one folder and everything below it"))
                    .on_click(cx.listener(
                    |app, _: &ClickEvent, _, cx| {
                        app.whole_home = false;
                        cx.notify();
                    },
                )),
            )
            .child(
                ui::segment("scope-home", "Whole home folder", self.whole_home)
                    .tip(tooltip::plain(
                        "Scan everything under your home folder. This takes longer than one project folder",
                    ))
                    .on_click(
                    cx.listener(|app, _: &ClickEvent, _, cx| {
                        app.whole_home = true;
                        app.folder_error = None;
                        cx.notify();
                    }),
                ),
            );
        let folder_card = ui::card("What to scan")
            .child(div().flex().child(scope))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(unit(space::M))
                    .child(field)
                    .child(
                        ui::button("browse", "Choose folder", ButtonKind::Secondary, true).tip(tooltip::plain("Pick a folder with the system dialog"))
                            .on_click(cx.listener(|app, _: &ClickEvent, _, cx| {
                                app.choose_folder(cx);
                            })),
                    )
                    .child(
                        ui::button("start-scan", "Start scan", ButtonKind::Primary, true).tip(Tooltip::new("Scan the chosen folder").keys(&["Enter"]).build())
                            .on_click(cx.listener(|app, _: &ClickEvent, _, cx| {
                                app.start_scan(cx);
                            })),
                    ),
            )
            .children(
                self.folder_error
                    .clone()
                    .map(|error| div().text_color(theme::danger()).child(error)),
            )
            .child(
                div()
                    .text_size(unit(font::SMALL))
                    .text_color(theme::faint())
                    .child(if self.whole_home {
                        "Walks everything under your home folder. Expect this to take a while."
                    } else {
                        "Enter starts the scan. Ctrl+V pastes a path, Ctrl+Backspace removes the last folder."
                    }),
            );

        let recent_card = (!self.recent.paths.is_empty()).then(|| {
            ui::card("Recent folders").child(div().flex().flex_col().children(
                self.recent.paths.iter().enumerate().map(|(index, path)| {
                    let path = path.clone();
                    let label = display_path(&path);
                    let missing = self.recent.is_gone(&path);
                    let full = tooltip::path(&path);
                    div()
                        .id(("recent", index))
                        .flex()
                        .items_center()
                        .gap(unit(space::M))
                        .h(unit(size::CONTROL))
                        .px(unit(space::M))
                        .rounded(theme::radius_control())
                        .cursor_pointer()
                        .hover(|style| style.bg(theme::hover()))
                        .child(
                            div()
                                .id(("recent-path", index))
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .tip(full)
                                .text_color(if missing {
                                    theme::faint()
                                } else {
                                    theme::text()
                                })
                                .child(label),
                        )
                        .child(
                            div()
                                .text_size(unit(font::SMALL))
                                .text_color(theme::faint())
                                .child(if missing {
                                    "missing"
                                } else {
                                    "double-click to scan"
                                }),
                        )
                        .on_click(cx.listener(move |app, event: &ClickEvent, _, cx| {
                            app.pick_recent(&path, cx);
                            if event.click_count() >= 2 {
                                app.start_scan(cx);
                            }
                        }))
                }),
            ))
        });

        let options = &self.options;
        let size_segments = start::SIZE_STEPS.into_iter().enumerate().fold(
            ui::segments(),
            |row, (index, bytes)| {
                row.child(
                    ui::segment(
                        ("min-size", index),
                        start::size_label(bytes),
                        options.min_size == bytes,
                    )
                    .on_click(cx.listener(
                        move |app, _: &ClickEvent, _, cx| {
                            app.options.min_size = bytes;
                            cx.notify();
                        },
                    )),
                )
            },
        );
        let depth_segments = start::DEPTH_STEPS.into_iter().enumerate().fold(
            ui::segments(),
            |row, (index, depth)| {
                row.child(
                    ui::segment(
                        ("depth", index),
                        start::depth_label(depth),
                        options.max_depth == depth,
                    )
                    .on_click(cx.listener(
                        move |app, _: &ClickEvent, _, cx| {
                            app.options.max_depth = depth;
                            cx.notify();
                        },
                    )),
                )
            },
        );
        let categories =
            Category::ALL
                .into_iter()
                .fold(div().flex().gap(unit(space::XXS)), |row, category| {
                    let on = options.categories.contains(&category);
                    row.child(
                        div()
                            .id(("category", category as usize))
                            .flex()
                            .items_center()
                            .gap(unit(space::S))
                            .px(unit(space::M))
                            .h(unit(size::SEGMENT))
                            .rounded(theme::radius_control())
                            .cursor_pointer()
                            .text_color(if on { theme::text() } else { theme::faint() })
                            .hover(|style| style.bg(theme::hover()))
                            .tip(tooltip::term(Term::category(category)))
                            .child(ui::checkbox(on))
                            .child(category.as_str())
                            .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                                app.options.toggle_category(category);
                                cx.notify();
                            })),
                    )
                });
        let toggle = |id: &'static str, on: bool, flip: fn(&mut ScanOptions)| {
            div()
                .id(id)
                .flex()
                .items_center()
                .gap(unit(space::S))
                .cursor_pointer()
                .child(ui::checkbox(on))
                .child(
                    div()
                        .text_color(theme::muted())
                        .child(if on { "On" } else { "Off" }),
                )
                .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                    flip(&mut app.options);
                    cx.notify();
                }))
        };
        let options_card = ui::card("Scan options")
            .child(
                ui::option_row("Kinds of artifacts", "Which detected folders to list")
                    .child(categories),
            )
            .child(
                ui::option_row("Smallest item", "Skip artifacts below this size")
                    .child(size_segments),
            )
            .child(
                ui::option_row("Folder depth", "How deep to walk below the chosen folder")
                    .child(depth_segments),
            )
            .child(
                ui::option_row(
                    "Global caches",
                    "Measure npm, cargo, pip and other caches in your home folder",
                )
                .child(toggle("opt-caches", options.caches, |options| {
                    options.caches = !options.caches;
                })),
            )
            .child(
                ui::option_row(
                    "Toolchains",
                    "List rustup, Node, Python, Java and Go versions and the projects that pin them",
                )
                .child(toggle("opt-toolchains", options.toolchains, |options| {
                    options.toolchains = !options.toolchains;
                })),
            )
            .child(
                ui::option_row(
                    "Docker and Podman",
                    "List dangling images, stopped containers, build cache and unused volumes",
                )
                .child(toggle("opt-docker", options.docker, |options| {
                    options.docker = !options.docker;
                })),
            )
            .child(
                ui::option_row(
                    "Git repositories",
                    "Find loose git objects to pack and branches that are already merged",
                )
                .child(toggle("opt-repos", options.repos, |options| {
                    options.repos = !options.repos;
                })),
            )
            .child(
                ui::option_row(
                    "Duplicate files",
                    "After the scan, hash files to find identical copies. Reads every file, so it is slower",
                )
                .child(toggle("opt-duplicates", options.duplicates, |options| {
                    options.duplicates = !options.duplicates;
                })),
            )
            .child(
                ui::option_row(
                    "Stay on one file system",
                    "Do not follow into other disks or mounts",
                )
                .child(toggle("opt-one-fs", options.one_file_system, |options| {
                    options.one_file_system = !options.one_file_system;
                })),
            );

        div()
            .id("start")
            .flex()
            .flex_col()
            .items_center()
            .size_full()
            .overflow_y_scroll()
            .p(unit(space::XXL))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(unit(space::XL))
                    .w_full()
                    .max_w(unit(size::PAGE))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(unit(space::XS))
                            .child(
                                div()
                                    .text_size(unit(font::HEADING))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Start a scan"),
                            )
                            .child(div().text_color(theme::muted()).child(
                                "kuizer looks for build output, dependencies, caches and duplicate files. Nothing is removed until you select items and confirm.",
                            )),
                    )
                    .child(folder_card)
                    .children(recent_card)
                    .child(options_card),
            )
            .into_any_element()
    }

    fn render_loading(&self, activity: &Activity) -> AnyElement {
        let status = self.tab.status(activity, &self.options);
        let header = div()
            .flex()
            .items_end()
            .gap(unit(space::XL))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap(unit(space::XXS))
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .child(self.tab.label()),
                    )
                    .child(
                        div()
                            .debug_selector(|| status.to_owned())
                            .text_color(theme::muted())
                            .child(status),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(unit(space::M))
                    .flex_none()
                    .text_size(unit(font::SMALL))
                    .text_color(theme::faint())
                    .child(
                        ui::tabular(div().text_color(theme::muted()))
                            .child(activity::took(self.started.elapsed())),
                    )
                    .child(ui::kbd("Esc"))
                    .child("cancels"),
            );
        let steps = ui::card("Progress").child(
            div().flex().flex_col().children(
                Step::ALL
                    .into_iter()
                    .map(|step| step_row(step, activity, &self.options, &self.roots)),
            ),
        );
        scroll("loading")
            .debug_selector(|| "activity".to_owned())
            .child(header)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_start()
                    .gap(unit(space::L))
                    .child(steps.flex_1().min_w(unit(size::LIST)))
                    .child(
                        found_card(activity, &self.roots)
                            .flex_1()
                            .min_w(unit(size::LIST)),
                    ),
            )
            .into_any_element()
    }

    fn render_overview(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(report) = self.report() else {
            return div().into_any_element();
        };
        if self.map.shown {
            return self.render_map(cx);
        }
        let figures = &self.report_stats;
        let total = figures.artifacts.all.bytes;
        let safe = figures.artifacts.safe.bytes;
        let review = figures.artifacts.review.bytes;
        let caches = figures.caches.all.bytes;
        let toolchains = figures.toolchains.all.bytes;
        let containers = figures.docker.all.bytes;
        let space = figures.space();
        let duplicates = report
            .duplicates
            .as_ref()
            .map_or(0, kuizer::dupes::Duplicates::reclaimable);
        let disk = report
            .disks
            .first()
            .and_then(|disk| Some((disk.available?, disk.total?)));

        let charts = figures.charts(report);
        let categories = &charts.categories;
        let projects = &charts.projects;
        let largest_project = projects.first().map_or(0, |bucket| bucket.bytes);
        let largest_cache = report
            .caches
            .first()
            .map_or(0, |(_, candidate)| candidate.usage.allocated);

        scroll("overview")
            .child(div().flex().child(self.overview_switch(cx)))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(unit(space::L))
                    .child(
                        ui::stat(
                            "Project artifacts",
                            theme::bytes(total),
                            format!("{} items", report.candidates.len()),
                        )
                        .id("stat-artifacts")
                        .tip(tooltip::term(Term::ProjectArtifacts)),
                    )
                    .child(
                        ui::stat(
                            "Safe to remove",
                            theme::bytes(safe),
                            "rebuilt on the next build",
                        )
                        .id("stat-safe")
                        .tip(tooltip::term(Term::Safe)),
                    )
                    .child(
                        ui::stat(
                            "Needs review",
                            theme::bytes(review),
                            "check before removing",
                        )
                        .id("stat-review")
                        .tip(tooltip::term(Term::Review)),
                    )
                    .child(
                        ui::stat(
                            "Global caches",
                            theme::bytes(caches),
                            format!("{} caches", report.caches.len()),
                        )
                        .id("stat-caches")
                        .tip(tooltip::term(Term::GlobalCaches)),
                    )
                    .child(
                        ui::stat(
                            "Toolchains",
                            theme::bytes(toolchains),
                            plural(report.toolchains.len(), "version"),
                        )
                        .id("stat-toolchains")
                        .tip(tooltip::term(Term::Toolchains)),
                    )
                    .child(
                        ui::stat(
                            "Docker",
                            theme::bytes(containers),
                            plural(report.docker.len(), "object"),
                        )
                        .id("stat-docker")
                        .tip(tooltip::term(Term::Docker)),
                    )
                    .child(
                        ui::stat(
                            "Git repositories",
                            theme::bytes(
                                report
                                    .repos
                                    .iter()
                                    .filter(|repository| {
                                        repository.collectable(self.config.git.min_gain.0)
                                    })
                                    .map(|repository| repository.objects.gain())
                                    .sum(),
                            ),
                            format!(
                                "git gc, {} merged branches",
                                report
                                    .repos
                                    .iter()
                                    .map(|repository| repository.merged.len())
                                    .sum::<usize>()
                            ),
                        )
                        .id("stat-repos")
                        .tip(tooltip::term(Term::GitRepositories)),
                    )
                    .child(
                        ui::stat(
                            "Duplicate files",
                            theme::bytes(duplicates),
                            "keep one copy of each",
                        )
                        .id("stat-duplicates")
                        .tip(tooltip::term(Term::DuplicateFiles)),
                    )
                    .child(
                        ui::stat(
                            "Disk free",
                            disk.map_or_else(String::new, |(free, _)| theme::bytes(free)),
                            disk.map_or_else(String::new, |(_, size)| {
                                format!("of {}", theme::bytes(size))
                            }),
                        )
                        .id("stat-disk")
                        .tip(tooltip::term(Term::DiskFree)),
                    ),
            )
            .child(overview_disk(report, figures))
            .child(
                div()
                    .flex()
                    .gap(unit(space::L))
                    .items_start()
                    .child(
                        ui::card("Where the space is")
                            .flex_1()
                            .children(categories.iter().enumerate().map(|(index, bucket)| {
                                ui::bar_row(
                                    format!(
                                        "{}  {}",
                                        bucket.key.as_str(),
                                        plural(bucket.count, "item")
                                    ),
                                    theme::bytes(bucket.bytes),
                                    theme::share(bucket.bytes, space),
                                    theme::bar(),
                                )
                                .id(("space", index))
                                .tip(tooltip::term(Term::category(bucket.key)))
                            }))
                            .child(ui::bar_row(
                                format!("global caches  {}", plural(report.caches.len(), "cache")),
                                theme::bytes(caches),
                                theme::share(caches, space),
                                theme::faint(),
                            ))
                            .when(report.options.toolchains, |this| {
                                this.child(ui::bar_row(
                                    format!(
                                        "toolchains  {}",
                                        plural(report.toolchains.len(), "version")
                                    ),
                                    theme::bytes(toolchains),
                                    theme::share(toolchains, space),
                                    theme::faint(),
                                ))
                            })
                            .when(report.options.docker, |this| {
                                this.child(ui::bar_row(
                                    format!("docker  {}", plural(report.docker.len(), "object")),
                                    theme::bytes(containers),
                                    theme::share(containers, space),
                                    theme::faint(),
                                ))
                            })
                            .when(categories.is_empty(), |this| {
                                this.child(ui::empty("No project artifacts found"))
                            }),
                    )
                    .child(
                        ui::card("Largest projects")
                            .flex_1()
                            .children(projects.iter().take(8).enumerate().map(|(index, bucket)| {
                                ui::bar_row(
                                    model::short_path(&bucket.key, &report.roots),
                                    theme::bytes(bucket.bytes),
                                    theme::share(bucket.bytes, largest_project),
                                    theme::bar(),
                                )
                                .id(("project", index))
                                .tip(tooltip::path(&bucket.key))
                            }))
                            .when(projects.is_empty(), |this| {
                                this.child(ui::empty("No projects with artifacts"))
                            }),
                    )
                    .child(
                        ui::card("Largest caches")
                            .flex_1()
                            .children(report.caches.iter().take(8).map(|(cache, candidate)| {
                                ui::bar_row(
                                    cache_label(cache),
                                    theme::bytes(candidate.usage.allocated),
                                    theme::share(candidate.usage.allocated, largest_cache),
                                    if candidate.rule.safety == Safety::Review {
                                        theme::review()
                                    } else {
                                        theme::bar()
                                    },
                                )
                            }))
                            .when(report.caches.is_empty(), |this| {
                                this.child(ui::empty("No global caches found"))
                            }),
                    ),
            )
            .child(overview_trends(figures, charts))
            .child(
                div()
                    .text_size(unit(font::SMALL))
                    .text_color(theme::faint())
                    .child(format!(
                        "Scanned {} folders and {} files in {:.1}s{}",
                        report.summary.progress.dirs,
                        report.summary.progress.files,
                        report.summary.duration.as_secs_f32(),
                        if report.summary.cancelled {
                            ", cancelled"
                        } else {
                            ""
                        }
                    )),
            )
            .into_any_element()
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
        let segment = |id: &'static str, label: &'static str, value: Option<Safety>| {
            ui::segment(id, label, self.safety == value)
                .when_some(value, |this, safety| {
                    this.tip(tooltip::term(Term::safety(safety)))
                })
                .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                    app.safety = value;
                    app.refresh_rows();
                    cx.notify();
                }))
        };
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
        let toolbar = ui::toolbar()
            .child(
                ui::segments()
                    .child(segment("safety-all", "All", None))
                    .child(segment("safety-safe", "Safe", Some(Safety::Safe)))
                    .child(segment("safety-review", "Review", Some(Safety::Review))),
            )
            .child(
                div()
                    .flex()
                    .gap(unit(space::XXS))
                    .children(Category::ALL.into_iter().map(chip)),
            )
            .child(div().flex_1())
            .child(div().text_color(theme::muted()).child(format!(
                "{} items, {}",
                self.rows.len(),
                theme::bytes(shown_bytes)
            )));
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

    fn render_caches(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(report) = self.report() else {
            return div().into_any_element();
        };
        let header = table_header()
            .child(div().w(unit(size::CHECK)))
            .child(head(col::M, "Size", Term::Size).text_right())
            .child(head(col::S, "Safety", Term::Safety))
            .child(head(col::M, "Tool", Term::Tool))
            .child(column(col::XXL).child("Cache"))
            .child(head(col::XS, "Changed", Term::Changed).text_right())
            .child(div().flex_1().child("Path"));
        let cursor = self.cursor_target();
        let rows = report
            .caches
            .iter()
            .filter(|(cache, candidate)| self.shows_cache(cache, candidate))
            .enumerate()
            .map(|(index, (cache, candidate))| {
                let id = candidate.id;
                let checked = self.selection.contains(&(Source::Cache, id));
                let review = candidate.rule.safety == Safety::Review;
                let row = table_row(("cache", id), checked)
                    .debug_selector(move || format!("cache-row-{index}"));
                let target = Target::Item(Source::Cache, id, candidate.path.clone());
                self.menu_row(row, target, cursor.as_ref(), cx)
                    .child(ui::checkbox(checked))
                    .child(
                        column(col::M)
                            .id(("cache-size", id))
                            .text_right()
                            .tip(size_details(candidate, &[]))
                            .font_weight(FontWeight::MEDIUM)
                            .child(theme::bytes(candidate.usage.allocated)),
                    )
                    .child(
                        column(col::S)
                            .flex()
                            .child(ui::safety_label(("cache-safety", id), review)),
                    )
                    .child(column(col::M).text_color(theme::muted()).child(cache.tool))
                    .child(column(col::XXL).truncate().child(cache.name))
                    .child(
                        column(col::XS)
                            .text_right()
                            .text_color(theme::muted())
                            .child(theme::age(candidate.newest)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .justify_center()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .id(("cache-path", id))
                                    .truncate()
                                    .text_color(theme::muted())
                                    .tip(tooltip::path(&candidate.path))
                                    .child(display_path(&candidate.path)),
                            )
                            .when(!cache.note.is_empty(), |this| {
                                this.child(
                                    div()
                                        .truncate()
                                        .text_size(unit(font::SMALL))
                                        .text_color(theme::faint())
                                        .child(cache.note),
                                )
                            }),
                    )
                    .h(unit(size::ROW_TALL))
                    .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                        if !app.busy() {
                            app.toggle(Source::Cache, id, cx);
                        }
                    }))
            });
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(
                ui::toolbar()
                    .child(format!(
                        "{} in {} caches",
                        theme::bytes(report.cache_total(None)),
                        report.caches.len()
                    ))
                    .child(
                        div()
                            .id("caches-safe")
                            .tip(tooltip::term(Term::Safe))
                            .child(format!(
                                "safe {}",
                                theme::bytes(report.cache_total(Some(Safety::Safe)))
                            )),
                    )
                    .child(
                        div()
                            .id("caches-review")
                            .text_color(theme::review())
                            .tip(tooltip::term(Term::Review))
                            .child(format!(
                                "review {}",
                                theme::bytes(report.cache_total(Some(Safety::Review)))
                            )),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_color(theme::faint())
                            .child("Safe caches are rebuilt on the next install or build"),
                    ),
            )
            .child(header)
            .child(
                div()
                    .id("cache-rows")
                    .debug_selector(|| "cache-list".to_owned())
                    .flex()
                    .flex_col()
                    .flex_1()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll_for(Tab::Caches))
                    .children(rows)
                    .when(report.caches.is_empty(), |this| {
                        this.child(ui::empty(if report.options.caches {
                            "No global caches found"
                        } else {
                            "Global caches were skipped for this scan"
                        }))
                    }),
            )
            .into_any_element()
    }

    fn render_toolchains(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(report) = self.report() else {
            return div().into_any_element();
        };
        let header = table_header()
            .child(div().w(unit(size::CHECK)))
            .child(head(col::M, "Size", Term::Size).text_right())
            .child(head(col::S, "Safety", Term::Safety))
            .child(head(col::S, "Tool", Term::Tool))
            .child(column(col::WIDE).child("Version"))
            .child(div().flex_1().child("Path"));
        let cursor = self.cursor_target();
        let rows = report
            .toolchains
            .iter()
            .filter(|(toolchain, candidate)| self.shows_toolchain(toolchain, candidate))
            .enumerate()
            .map(|(index, (toolchain, candidate))| {
                let id = candidate.id;
                let checked = self.selection.contains(&(Source::Toolchain, id));
                let review = candidate.rule.safety == Safety::Review;
                let note = toolchain.note();
                let row = table_row(("toolchain", id), checked)
                    .debug_selector(move || format!("toolchain-row-{index}"));
                let target = Target::Item(Source::Toolchain, id, candidate.path.clone());
                self.menu_row(row, target, cursor.as_ref(), cx)
                    .child(ui::checkbox(checked))
                    .child(
                        column(col::M)
                            .id(("toolchain-size", id))
                            .text_right()
                            .tip(size_details(candidate, &[]))
                            .font_weight(FontWeight::MEDIUM)
                            .child(theme::bytes(candidate.usage.allocated)),
                    )
                    .child(
                        column(col::S)
                            .flex()
                            .child(ui::safety_label(("toolchain-safety", id), review)),
                    )
                    .child(
                        column(col::S)
                            .text_color(theme::muted())
                            .child(toolchain.tool.name()),
                    )
                    .child(
                        column(col::WIDE)
                            .id(("toolchain-version", id))
                            .truncate()
                            .tip(tooltip::plain(toolchain.version.clone()))
                            .child(toolchain_label(toolchain)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .justify_center()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .id(("toolchain-path", id))
                                    .truncate()
                                    .text_color(theme::muted())
                                    .tip(tooltip::path(&candidate.path))
                                    .child(display_path(&candidate.path)),
                            )
                            .when(!note.is_empty(), |this| {
                                this.child(
                                    div()
                                        .truncate()
                                        .text_size(unit(font::SMALL))
                                        .text_color(theme::faint())
                                        .child(note),
                                )
                            }),
                    )
                    .h(unit(size::ROW_TALL))
                    .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                        if !app.busy() {
                            app.toggle(Source::Toolchain, id, cx);
                        }
                    }))
            });
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(
                ui::toolbar()
                    .child(format!(
                        "{} in {}",
                        theme::bytes(report.toolchain_total(None)),
                        plural(report.toolchains.len(), "version")
                    ))
                    .child(
                        div()
                            .id("toolchains-safe")
                            .tip(tooltip::term(Term::Safe))
                            .child(format!(
                                "unused {}",
                                theme::bytes(report.toolchain_total(Some(Safety::Safe)))
                            )),
                    )
                    .child(
                        div()
                            .id("toolchains-review")
                            .text_color(theme::review())
                            .tip(tooltip::term(Term::Review))
                            .child(format!(
                                "default or pinned {}",
                                theme::bytes(report.toolchain_total(Some(Safety::Review)))
                            )),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_color(theme::faint())
                            .child("Removed for good, not moved to the trash"),
                    ),
            )
            .child(header)
            .child(
                div()
                    .id("toolchain-rows")
                    .debug_selector(|| "toolchain-list".to_owned())
                    .flex()
                    .flex_col()
                    .flex_1()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll_for(Tab::Toolchains))
                    .children(rows)
                    .when(report.toolchains.is_empty(), |this| {
                        this.child(ui::empty(if report.options.toolchains {
                            "No installed toolchain versions found"
                        } else {
                            "Toolchains were skipped for this scan"
                        }))
                    }),
            )
            .into_any_element()
    }

    fn render_docker(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(report) = self.report() else {
            return div().into_any_element();
        };
        let header = table_header()
            .child(div().w(unit(size::CHECK)))
            .child(head(col::M, "Size", Term::Size).text_right())
            .child(head(col::S, "Safety", Term::Safety))
            .child(column(col::S).child("Engine"))
            .child(column(col::L).child("Object"))
            .child(div().flex_1().child("Name"));
        let cursor = self.cursor_target();
        let rows = report
            .docker
            .iter()
            .filter(|(item, candidate)| self.shows_docker(item, candidate))
            .enumerate()
            .map(|(index, (item, candidate))| {
                let id = candidate.id;
                let checked = self.selection.contains(&(Source::Docker, id));
                let review = candidate.rule.safety == Safety::Review;
                let row = table_row(("docker", id), checked)
                    .debug_selector(move || format!("docker-row-{index}"));
                let target = Target::Item(Source::Docker, id, candidate.path.clone());
                self.menu_row(row, target, cursor.as_ref(), cx)
                    .child(ui::checkbox(checked))
                    .child(
                        column(col::M)
                            .text_right()
                            .font_weight(FontWeight::MEDIUM)
                            .child(theme::bytes(candidate.usage.allocated)),
                    )
                    .child(
                        column(col::S)
                            .flex()
                            .child(ui::safety_label(("docker-safety", id), review)),
                    )
                    .child(
                        column(col::S)
                            .text_color(theme::muted())
                            .child(item.engine.clone()),
                    )
                    .child(
                        column(col::L)
                            .text_color(theme::muted())
                            .child(item.object.name()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .justify_center()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .id(("docker-name", id))
                                    .truncate()
                                    .tip(tooltip::plain(format!("{}  {}", item.label, item.id)))
                                    .child(item.label.clone()),
                            )
                            .when(!item.note.is_empty(), |this| {
                                this.child(
                                    div()
                                        .truncate()
                                        .text_size(unit(font::SMALL))
                                        .text_color(theme::faint())
                                        .child(item.note.clone()),
                                )
                            }),
                    )
                    .h(unit(size::ROW_TALL))
                    .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                        if !app.busy() {
                            app.toggle(Source::Docker, id, cx);
                        }
                    }))
            });
        let empty = if !report.options.docker {
            "Docker and Podman were skipped for this scan".to_owned()
        } else if report.docker_issues.is_empty() {
            "Nothing to free in Docker or Podman".to_owned()
        } else {
            report.docker_issues.join(". ")
        };
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(
                ui::toolbar()
                    .child(format!(
                        "{} in {}",
                        theme::bytes(report.docker_total(None)),
                        plural(report.docker.len(), "object")
                    ))
                    .child(
                        div()
                            .id("docker-safe")
                            .tip(tooltip::term(Term::Safe))
                            .child(format!(
                                "safe {}",
                                theme::bytes(report.docker_total(Some(Safety::Safe)))
                            )),
                    )
                    .child(
                        div()
                            .id("docker-review")
                            .text_color(theme::review())
                            .tip(tooltip::term(Term::Review))
                            .child(format!(
                                "review {}",
                                theme::bytes(report.docker_total(Some(Safety::Review)))
                            )),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_color(theme::faint())
                            .child("Removed through the engine, not moved to the trash"),
                    ),
            )
            .when(
                !report.docker.is_empty() && !report.docker_issues.is_empty(),
                |this| {
                    this.child(
                        div()
                            .px(unit(space::GUTTER))
                            .py(unit(space::S))
                            .text_color(theme::faint())
                            .child(report.docker_issues.join(". ")),
                    )
                },
            )
            .child(header)
            .child(
                div()
                    .id("docker-rows")
                    .debug_selector(|| "docker-list".to_owned())
                    .flex()
                    .flex_col()
                    .flex_1()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll_for(Tab::Docker))
                    .children(rows)
                    .when(report.docker.is_empty(), |this| {
                        this.child(ui::empty(empty))
                    }),
            )
            .into_any_element()
    }

    fn render_duplicate_search(&self, cx: &mut Context<Self>) -> AnyElement {
        let (text, label) = match (&self.search, &self.search_error) {
            (Some(search), _) => (
                match search.stage {
                    None => "Starting the duplicate search".to_owned(),
                    Some(dupes::Stage::Walking { files }) => {
                        format!("Collecting files, {} so far", activity::grouped(files))
                    }
                    Some(dupes::Stage::Hashing { files }) => {
                        format!(
                            "Hashing {} files of equal size",
                            activity::grouped(files as u64)
                        )
                    }
                    Some(dupes::Stage::Verifying { groups }) => format!(
                        "Comparing {} groups byte by byte",
                        activity::grouped(groups as u64)
                    ),
                },
                "Stop the search",
            ),
            (None, Some(error)) => (
                format!("The duplicate search failed: {error}"),
                "Find duplicates",
            ),
            (None, None) => (
                "Duplicate files are found on request. It reads every file, so it takes a while"
                    .to_owned(),
                "Find duplicates",
            ),
        };
        let tip = Tooltip::new("Hash files under the scanned folders to find identical copies")
            .keys(&["D"])
            .build();
        centered(
            div()
                .debug_selector(|| "duplicate-search".to_owned())
                .flex()
                .flex_col()
                .items_center()
                .gap(unit(space::L))
                .child(ui::empty(text))
                .child(
                    ui::button("find-duplicates", label, ButtonKind::Secondary, true)
                        .tip(tip)
                        .on_click(cx.listener(|app, _: &ClickEvent, _, cx| {
                            app.find_duplicates(cx);
                        })),
                ),
        )
    }

    fn render_duplicates(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(report) = self.report() else {
            return div().into_any_element();
        };
        let Some(found) = &report.duplicates else {
            return self.render_duplicate_search(cx);
        };
        let wasted = found.reclaimable();
        let copies: usize = found.groups.iter().map(|group| group.members.len()).sum();
        let largest = found
            .groups
            .first()
            .map_or(0, kuizer::dupes::Group::reclaimable);
        let cursor = self.cursor_target();
        let needle = self.needle(Tab::Duplicates);
        scroll("duplicates")
            .debug_selector(|| "duplicate-list".to_owned())
            .track_scroll(&self.scroll_for(Tab::Duplicates))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(unit(space::L))
                    .child(
                        ui::stat(
                            "Reclaimable",
                            theme::bytes(wasted),
                            "by keeping one copy of each",
                        )
                        .id("stat-reclaimable")
                        .tip(tooltip::term(Term::Reclaimable)),
                    )
                    .child(
                        ui::stat(
                            "Groups",
                            found.groups.len().to_string(),
                            format!("{copies} files in total"),
                        )
                        .id("stat-groups")
                        .tip(tooltip::term(Term::Groups)),
                    )
                    .child(
                        ui::stat(
                            "Files compared",
                            found.files.to_string(),
                            format!("in {:.1}s", found.duration.as_secs_f32()),
                        )
                        .id("stat-compared")
                        .tip(tooltip::term(Term::FilesCompared)),
                    ),
            )
            .child(
                ui::card(if found.groups.len() > DUPLICATE_GROUPS {
                    format!(
                        "Largest {DUPLICATE_GROUPS} of {} groups",
                        found.groups.len()
                    )
                } else {
                    "Groups".to_owned()
                })
                .children(
                    found
                        .groups
                        .iter()
                        .take(DUPLICATE_GROUPS)
                        .enumerate()
                        .filter(|(_, group)| {
                            group
                                .members
                                .iter()
                                .any(|member| found_path(needle.as_deref(), &member.path))
                        })
                        .map(|(index, group)| {
                            let members = SharedString::from(format!("dupe-{index}"));
                            div()
                                .flex()
                                .flex_col()
                                .gap(unit(space::XS))
                                .pt(unit(space::M))
                                .border_t_1()
                                .border_color(theme::border())
                                .child(
                                    ui::bar_row(
                                        format!(
                                            "{} copies of {}{}",
                                            group.members.len(),
                                            theme::bytes(group.size),
                                            if group.verified {
                                                ""
                                            } else {
                                                ", not byte-verified"
                                            }
                                        ),
                                        format!(
                                            "{} reclaimable",
                                            theme::bytes(group.reclaimable())
                                        ),
                                        theme::share(group.reclaimable(), largest),
                                        theme::bar(),
                                    )
                                    .id(("group", index))
                                    .tip(group_details(group)),
                                )
                                .children(
                                    group
                                        .members
                                        .iter()
                                        .enumerate()
                                        .filter(|(_, member)| {
                                            found_path(needle.as_deref(), &member.path)
                                        })
                                        .map(|(position, member)| {
                                            let row = div()
                                                .flex()
                                                .gap(unit(space::M))
                                                .text_color(theme::muted())
                                                .debug_selector(move || {
                                                    format!("duplicate-{index}-{position}")
                                                });
                                            let target = Target::Duplicate(member.path.clone());
                                            self.menu_row(row, target, cursor.as_ref(), cx)
                                                .child(
                                                    div()
                                                        .id((members.clone(), position))
                                                        .flex_1()
                                                        .min_w_0()
                                                        .truncate()
                                                        .tip(tooltip::path(&member.path))
                                                        .child(model::short_path(
                                                            &member.path,
                                                            &report.roots,
                                                        )),
                                                )
                                                .child(
                                                    div()
                                                        .flex_none()
                                                        .text_color(theme::faint())
                                                        .child(theme::age(member.modified)),
                                                )
                                        }),
                                )
                        }),
                )
                .when(found.groups.is_empty(), |this| {
                    this.child(ui::empty("No duplicate files found"))
                }),
            )
            .into_any_element()
    }

    fn render_history(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(report) = self.report() else {
            return div().into_any_element();
        };
        let view = match &report.history {
            Ok(view) => view,
            Err(error) => {
                return centered(
                    div()
                        .text_color(theme::danger())
                        .child(format!("Cannot read the history: {error}")),
                );
            }
        };
        let cumulative = &view.history.cumulative;
        let runs: Vec<(usize, &kuizer::history::Run)> = view
            .history
            .runs
            .iter()
            .enumerate()
            .filter(|(_, run)| run.action == Action::Delete)
            .rev()
            .collect();
        let needle = self.needle(Tab::History);
        let listed: Vec<(usize, &kuizer::history::Run)> = runs
            .iter()
            .copied()
            .filter(|(_, run)| shows_run(needle.as_deref(), run))
            .collect();
        let recent: Vec<&kuizer::history::Run> = runs
            .iter()
            .take(HISTORY_BARS)
            .rev()
            .map(|(_, run)| *run)
            .collect();
        let can_restore = self.can_restore();
        let refreshing = self.refreshing();
        let cursor = self.cursor_target();
        let tallest = recent.iter().map(|run| run.total.bytes).max().unwrap_or(0);
        let chart = div()
            .flex()
            .items_end()
            .gap(unit(space::XS))
            .h(unit(size::CHART))
            .children(recent.iter().map(|run| {
                div()
                    .id(("cleanup-bar", run.id))
                    .flex_1()
                    .max_w(unit(size::BAR))
                    .h(gpui::relative(
                        theme::share(run.total.bytes, tallest).max(0.02),
                    ))
                    .rounded_t(theme::radius_bar())
                    .bg(theme::bar())
                    .hover(|style| style.bg(theme::text()))
                    .tip(
                        Tooltip::details(format!("Cleanup {}", run.session()))
                            .row("Started", short_time(&run.started_at))
                            .row("Items", run.total.items.to_string())
                            .row("Size", theme::bytes(run.total.bytes))
                            .build(),
                    )
            }));
        let header = table_header()
            .child(head(col::XS, "Run", Term::Run))
            .child(column(col::XL).child("Started"))
            .child(column(col::S).text_right().child("Items"))
            .child(head(col::M, "Size", Term::Size).text_right())
            .child(head(col::M, "Mode", Term::Mode))
            .child(head(col::L, "Trash", Term::Restore))
            .child(
                div()
                    .id("Command")
                    .flex_1()
                    .tip(tooltip::term(Term::Command))
                    .child("Command"),
            );
        scroll("history")
            .debug_selector(|| "history-list".to_owned())
            .track_scroll(&self.scroll_for(Tab::History))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(unit(space::L))
                    .child(
                        ui::stat(
                            "Cleanups",
                            cumulative.runs.to_string(),
                            cumulative
                                .first_run
                                .as_deref()
                                .map(|first| format!("since {}", short_time(first)))
                                .unwrap_or_default(),
                        )
                        .id("stat-cleanups")
                        .tip(tooltip::term(Term::Cleanups)),
                    )
                    .child(
                        ui::stat(
                            "Items removed",
                            cumulative.items.to_string(),
                            format!("{} restored", cumulative.restored_items),
                        )
                        .id("stat-removed")
                        .tip(tooltip::term(Term::ItemsRemoved)),
                    )
                    .child(
                        ui::stat(
                            "Freed",
                            theme::bytes(
                                self.report_stats
                                    .history(report)
                                    .map_or(0, |history| history.freed),
                            ),
                            "removed or emptied from the trash",
                        )
                        .id("stat-freed")
                        .tip(tooltip::term(Term::Freed)),
                    )
                    .child(
                        ui::stat(
                            "Still in the trash",
                            if view.trash_known {
                                theme::bytes(
                                    self.report_stats
                                        .history(report)
                                        .map_or(0, |history| history.in_trash),
                                )
                            } else {
                                "unknown".to_owned()
                            },
                            "restore from the table below",
                        )
                        .id("stat-trash")
                        .tip(tooltip::term(Term::InTrash)),
                    ),
            )
            .child(
                ui::card(format!("Last {} cleanups", recent.len()))
                    .child(chart)
                    .when(recent.is_empty(), |this| {
                        this.child(ui::empty("No cleanups recorded yet"))
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .rounded(theme::radius_panel())
                    .border_1()
                    .border_color(theme::border())
                    .child(header)
                    .children(listed.iter().map(|&(index, run)| {
                        let mode = run.deleted.first().map_or("", |entry| match entry.mode {
                            kuizer::delete::Mode::Trash => "trash",
                            kuizer::delete::Mode::Permanent => "permanent",
                            kuizer::delete::Mode::DryRun => "dry run",
                        });
                        let session = run.id;
                        let row = div()
                            .flex()
                            .items_center()
                            .gap(unit(space::L))
                            .h(unit(size::ROW))
                            .px(unit(space::GUTTER))
                            .border_t_1()
                            .border_color(theme::border())
                            .debug_selector(move || format!("run-{session}"));
                        let target = Target::Run(run.id, run.command.clone());
                        self.menu_row(row, target, cursor.as_ref(), cx)
                            .child(column(col::XS).child(run.session()))
                            .child(
                                column(col::XL)
                                    .id(("started", index))
                                    .text_color(theme::muted())
                                    .tip(tooltip::plain(run.started_at.clone()))
                                    .child(short_time(&run.started_at)),
                            )
                            .child(
                                column(col::S)
                                    .text_right()
                                    .child(run.total.items.to_string()),
                            )
                            .child(
                                column(col::M)
                                    .text_right()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(theme::bytes(run.total.bytes)),
                            )
                            .child(column(col::M).text_color(theme::muted()).child(mode))
                            .child(column(col::L).flex().child(self.restore_cell(
                                self.report_stats.trash_state(report, index),
                                run,
                                can_restore,
                                refreshing,
                                cx,
                            )))
                            .child(
                                div()
                                    .id(("command", index))
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(theme::faint())
                                    .tip(tooltip::plain(run.command.clone()))
                                    .child(run.command.clone()),
                            )
                    })),
            )
            .children(
                view.history
                    .runs
                    .iter()
                    .any(|run| run.action == Action::Restore)
                    .then(|| {
                        div()
                            .text_size(unit(font::SMALL))
                            .text_color(theme::faint())
                            .child("Restores are counted in Items removed as restored")
                    }),
            )
            .into_any_element()
    }

    fn restore_cell(
        &self,
        state: TrashState,
        run: &kuizer::history::Run,
        enabled: bool,
        refreshing: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let session = run.id;
        let tag = |label: &'static str, tip: &'static str| {
            ui::tag(("trash-state", session), label, ui::Tone::Neutral)
                .tip(tooltip::plain(tip))
                .into_any_element()
        };
        match state {
            TrashState::Restorable { items, bytes } => {
                let selector = format!("restore-{}", run.session());
                ui::button(("restore", session), "Restore", ButtonKind::Secondary, enabled)
                    .debug_selector(move || selector.clone())
                    .when(refreshing, |this| {
                        this.tip(tooltip::plain("Available once the fresh scan finishes"))
                    })
                    .when(!refreshing, |this| {
                        this.tip(
                            Tooltip::details(format!("Restore {}", run.session()))
                                .row("Items", items.to_string())
                                .row("Size", theme::bytes(bytes))
                                .note("Puts what is still in the trash back where it was. A path that is taken again is skipped, never overwritten.")
                                .build(),
                        )
                    })
                    .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                        app.restore(session, Scope::StillInTrash, cx);
                    }))
                    .into_any_element()
            }
            TrashState::Restored => tag("restored", "Everything from this cleanup was put back"),
            TrashState::Emptied => tag(
                "not in trash",
                "Nothing from this cleanup is left in the trash to restore",
            ),
            TrashState::Unknown => tag("unknown", "The trash cannot be read on this system"),
            TrashState::NotTrashed => div()
                .text_color(theme::faint())
                .child("-")
                .into_any_element(),
        }
    }

    fn render_issues(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(report) = self.report() else {
            return div().into_any_element();
        };
        if report.issues.is_empty() {
            return centered(ui::empty("The scan reported no issues"));
        }
        let cursor = self.cursor_target();
        let needle = self.needle(Tab::Issues);
        scroll("issues")
            .debug_selector(|| "issue-list".to_owned())
            .track_scroll(&self.scroll_for(Tab::Issues))
            .child(
                div()
                    .id("issues-count")
                    .text_color(theme::muted())
                    .tip(tooltip::term(Term::Issues))
                    .child(format!(
                        "{} folders could not be read or were skipped",
                        report.issues.len()
                    )),
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
            .into_any_element()
    }

    fn render_toast(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let toast = self.toast.as_ref()?;
        let dismiss = || {
            ui::button("dismiss-toast", "Dismiss", ButtonKind::Ghost, true)
                .debug_selector(|| "dismiss-toast".to_owned())
                .tip(Tooltip::new("Hide this message").keys(&["Esc"]).build())
                .on_click(cx.listener(|app, _: &ClickEvent, _, cx| app.dismiss_toast(cx)))
        };
        let line = div().flex().items_center().gap(unit(space::L));
        let panel = div()
            .id("toast")
            .debug_selector(|| "toast".to_owned())
            .flex()
            .flex_col()
            .gap(unit(space::S))
            .max_w(unit(size::PALETTE))
            .py(unit(space::M))
            .px(unit(space::L))
            .rounded(theme::radius_panel())
            .bg(theme::elevated())
            .border_1()
            .border_color(theme::border_strong())
            .occlude();
        let panel = match &toast.notice {
            Notice::Trashed {
                session,
                items,
                bytes,
            } => {
                let session = *session;
                let enabled = self.can_restore();
                let refreshing = self.refreshing();
                panel.child(
                    line.child(div().flex_1().child(format!(
                        "{}, {} moved to trash",
                        plural(*items, "item"),
                        theme::bytes(*bytes)
                    )))
                    .child(
                        ui::button("undo", "Undo", ButtonKind::Secondary, enabled)
                            .debug_selector(|| "undo".to_owned())
                            .when(refreshing, |this| {
                                this.tip(tooltip::plain("Available once the fresh scan finishes"))
                            })
                            .when(!refreshing, |this| {
                                this.tip(
                                    Tooltip::new(format!(
                                        "Put cleanup s{session} back from the trash"
                                    ))
                                    .keys(&[UNDO_KEY])
                                    .build(),
                                )
                            })
                            .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                                app.restore(session, Scope::Undo, cx);
                            })),
                    )
                    .child(ui::kbd(UNDO_KEY))
                    .child(dismiss()),
                )
            }
            Notice::Restoring { session } => panel.child(
                line.child(
                    div()
                        .text_color(theme::muted())
                        .child(format!("Restoring cleanup s{session} from the trash")),
                ),
            ),
            Notice::Restored(restored) => {
                let failed = restored.problems.len();
                panel
                    .child(
                        line.child(div().flex_1().child(if restored.items > 0 {
                            format!(
                                "{}, {} restored",
                                plural(restored.items, "item"),
                                theme::bytes(restored.bytes)
                            )
                        } else {
                            "Nothing was restored".to_owned()
                        }))
                        .child(dismiss()),
                    )
                    .when(failed > 0, |this| {
                        this.child(
                            div()
                                .debug_selector(|| "toast-error".to_owned())
                                .text_color(theme::danger())
                                .child(format!("{} could not be restored", plural(failed, "item"))),
                        )
                        .children(
                            restored
                                .problems
                                .iter()
                                .take(TOAST_PROBLEMS)
                                .enumerate()
                                .map(|(index, (path, reason))| {
                                    div()
                                        .id(("toast-problem", index))
                                        .truncate()
                                        .text_size(unit(font::SMALL))
                                        .text_color(theme::faint())
                                        .tip(tooltip::path(path))
                                        .child(format!("{} {reason}", self.shown_path(path)))
                                }),
                        )
                        .when(failed > TOAST_PROBLEMS, |this| {
                            this.child(
                                div()
                                    .text_size(unit(font::SMALL))
                                    .text_color(theme::faint())
                                    .child(format!(
                                        "and {} more, kuizer history show lists them",
                                        failed - TOAST_PROBLEMS
                                    )),
                            )
                        })
                    })
                    .children(restored.history_error.as_ref().map(|error| {
                        div()
                            .text_color(theme::danger())
                            .child(format!("The history could not be saved: {error}"))
                    }))
            }
            Notice::Failed(error) => panel.child(
                line.child(
                    div()
                        .flex_1()
                        .debug_selector(|| "toast-error".to_owned())
                        .text_color(theme::danger())
                        .child(error.clone()),
                )
                .child(dismiss()),
            ),
            Notice::Info(text) => panel.child(
                line.child(div().flex_1().child(text.clone()))
                    .child(dismiss()),
            ),
        };
        Some(
            div()
                .absolute()
                .bottom(unit(space::L))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .px(unit(space::XL))
                .child(panel)
                .into_any_element(),
        )
    }

    fn shown_path(&self, path: &Path) -> String {
        match self.report() {
            Some(report) => model::short_path(path, &report.roots),
            None => display_path(path),
        }
    }

    fn render_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dialog = self.dialog.as_ref()?;
        let panel = div()
            .flex()
            .flex_col()
            .gap(unit(space::L))
            .w(unit(size::DIALOG))
            .p(unit(space::XL))
            .rounded(theme::radius_dialog())
            .bg(theme::elevated())
            .border_1()
            .border_color(theme::border_strong());
        let body = match dialog {
            Dialog::Confirm {
                rule,
                typed,
                stats,
                preview,
                ..
            } => {
                let total = stats.total();
                let count = total.all.count;
                let bytes = total.all.bytes;
                let review = total.review.count;
                let accepted = rule.accepts(typed);
                let permanent = stats.permanent();
                let trashed = count - permanent;
                let selected: Vec<(Source, &Candidate)> =
                    preview.iter().filter_map(|&key| self.item(key)).collect();
                let (title, verb, button) = match (permanent, trashed) {
                    (0, count) => (
                        format!("Move {count} items to the trash?"),
                        "move",
                        "Move to trash",
                    ),
                    (count, 0) => (
                        format!("Delete {count} items for good?"),
                        "delete",
                        "Delete for good",
                    ),
                    _ => (
                        format!("Remove {count} items?"),
                        "remove",
                        "Remove",
                    ),
                };
                panel
                    .child(
                        div()
                            .text_size(unit(font::TITLE))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .when(permanent == 0, |this| {
                        this.child(div().text_color(theme::muted()).child(format!(
                            "{} will be moved to the trash. Everything stays restorable with kuizer restore until the trash is emptied.",
                            theme::bytes(bytes)
                        )))
                    })
                    .when(permanent > 0, |this| {
                        this.child(
                            div()
                                .debug_selector(|| "permanent-warning".to_owned())
                                .text_color(theme::danger())
                                .child(format!(
                                    "{permanent} {} deleted permanently. They skip the trash, cannot be restored and the undo bar cannot bring them back.{}",
                                    permanent_kinds(stats),
                                    if stats.source(Source::Toolchain).all.count > 0 {
                                        " The tool downloads a toolchain version again when it is needed."
                                    } else {
                                        ""
                                    }
                                )),
                        )
                        .child(div().text_color(theme::muted()).child(if trashed > 0 {
                            format!(
                                "{} in total. The other {trashed} items move to the trash and stay restorable.",
                                theme::bytes(bytes)
                            )
                        } else {
                            format!("{} in total.", theme::bytes(bytes))
                        }))
                    })
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(unit(space::XS))
                            .p(unit(space::L))
                            .rounded(theme::radius_panel())
                            .bg(theme::panel())
                            .children(selected.iter().take(LIST_LIMIT).map(|(_, candidate)| {
                                div()
                                    .flex()
                                    .gap(unit(space::M))
                                    .child(
                                        div()
                                            .id(("dialog-path", candidate.id))
                                            .flex_1()
                                            .min_w_0()
                                            .truncate()
                                            .tip(tooltip::path(&candidate.path))
                                            .child(self.dialog_path(candidate)),
                                    )
                                    .child(ui::safety_label(
                                        ("dialog-safety", candidate.id),
                                        candidate.rule.safety == Safety::Review,
                                    ))
                                    .child(
                                        div()
                                            .w(unit(col::M))
                                            .text_right()
                                            .text_color(theme::muted())
                                            .font_weight(FontWeight::MEDIUM)
.child(theme::bytes(candidate.usage.allocated)),
                                    )
                            }))
                            .when(count > LIST_LIMIT, |this| {
                                this.child(div().text_color(theme::faint()).child(format!(
                                    "and {} more",
                                    count - LIST_LIMIT
                                )))
                            }),
                    )
                    .when_some(
                        match rule {
                            Confirmation::TypeCount(count) => Some(*count),
                            Confirmation::Simple => None,
                        },
                        |this, count| {
                            this.child(div().text_color(theme::review()).child(format!(
                                "{review} of these are marked review. Type {count} to {verb} all {count} items."
                            )))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .h(unit(size::ROW))
                                    .px(unit(space::L))
                                    .rounded(theme::radius_control())
                                    .bg(theme::background())
                                    .border_1()
                                    .border_color(if accepted {
                                        theme::primary()
                                    } else {
                                        theme::border_strong()
                                    })
                                    .child(typed.clone())
                                    .child(div().w(unit(size::HAIR)).h(unit(size::CARET)).bg(theme::text())),
                            )
                        },
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap(unit(space::M))
                            .child(
                                ui::button("cancel", "Cancel", ButtonKind::Secondary, true).tip(Tooltip::new("Keep everything where it is").keys(&["Esc"]).build())
                                    .on_click(cx.listener(|app, _: &ClickEvent, _, cx| {
                                        app.dialog = None;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                ui::button("confirm", button, ButtonKind::Primary, accepted).tip(Tooltip::new(if permanent > 0 { "Remove these items, toolchains for good" } else { "Move these items to the trash" }).keys(&["Enter"]).build())
                                    .on_click(cx.listener(|app, _: &ClickEvent, _, cx| {
                                        app.accept(cx);
                                    })),
                            ),
                    )
            }
            Dialog::Branches(scope) => self.render_branch_dialog(scope, panel, cx),
            Dialog::Running { permanent } => panel
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(if *permanent {
                            "Removing items"
                        } else {
                            "Moving items to the trash"
                        }),
                )
                .child(
                    div()
                        .text_color(theme::muted())
                        .child("Each path is checked again right before it is removed."),
                ),
            Dialog::Finished(finished) => panel
                .child(
                    div()
                        .text_size(unit(font::TITLE))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format!(
                            "{} {} items, {}",
                            if finished.deleted == 0 { "Moved" } else { "Removed" },
                            finished.removed,
                            theme::bytes(finished.removed_bytes)
                        )),
                )
                .child(div().text_color(theme::muted()).child(
                    match (finished.deleted, finished.removed - finished.deleted) {
                        (0, _) => "kuizer history lists this run and kuizer restore brings it back."
                            .to_owned(),
                        (deleted, 0) => format!(
                            "{deleted} items were deleted for good. kuizer history lists them."
                        ),
                        (deleted, trashed) => format!(
                            "{deleted} items were deleted for good. The other {trashed} items are in the trash and kuizer restore brings them back."
                        ),
                    },
                ))
                .when(!finished.failures.is_empty(), |this| {
                    this.child(div().text_color(theme::danger()).child(format!(
                        "{} items could not be removed",
                        finished.failures.len()
                    )))
                    .children(
                        finished.failures.iter().take(LIST_LIMIT).enumerate().map(
                            |(index, (path, error))| {
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .id(("failure", index))
                                            .truncate()
                                            .tip(tooltip::plain(path.clone()))
                                            .child(path.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(unit(font::SMALL))
                                            .text_color(theme::faint())
                                            .child(error.clone()),
                                    )
                            },
                        ),
                    )
                })
                .children(finished.history_error.as_ref().map(|error| {
                    div()
                        .text_color(theme::danger())
                        .child(format!("The history could not be saved: {error}"))
                }))
                .child(
                    div().flex().justify_end().child(
                        ui::button("done", "Done", ButtonKind::Primary, true)
                            .tip(Tooltip::new("Close this summary").keys(&["Enter"]).build())
                            .on_click(cx.listener(|app, _: &ClickEvent, _, cx| {
                                app.dialog = None;
                                cx.notify();
                            })),
                    ),
                ),
        };
        Some(
            div()
                .id("scrim")
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(theme::scrim())
                .occlude()
                .child(body)
                .into_any_element(),
        )
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
        let dialog = self.render_dialog(cx);
        let toast = self.render_toast(cx);
        let menu = self.render_menu(cx);
        let palette = self.render_palette(cx);
        let shortcuts = self.render_shortcuts(cx);
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
                    .children(toast),
            )
            .when(!start, |this| this.child(self.render_statusbar(cx)))
            .children(dialog)
            .children(menu)
            .children(palette)
            .children(shortcuts)
            .child(tooltip::layer(cx));
        frame::frame(root, window)
    }
}

fn overview_disk(report: &Report, figures: &ReportStats) -> Div {
    let safe = figures.artifacts.safe.bytes;
    let review = figures.artifacts.review.bytes;
    let caches = figures.caches.all.bytes;
    let toolchains = figures.toolchains.safe.bytes;
    let containers = figures.docker.safe.bytes;
    let duplicates = report
        .duplicates
        .as_ref()
        .map_or(0, kuizer::dupes::Duplicates::reclaimable);
    let reclaimable = safe + review + caches + toolchains + containers + duplicates;
    let mut segments = vec![
        Segment {
            label: "Safe artifacts".into(),
            bytes: safe,
            fill: theme::bar_strong(),
        },
        Segment {
            label: "Needs review".into(),
            bytes: review,
            fill: theme::review(),
        },
        Segment {
            label: "Global caches".into(),
            bytes: caches,
            fill: theme::bar(),
        },
        Segment {
            label: "Unused toolchains".into(),
            bytes: toolchains,
            fill: theme::bar_mid(),
        },
        Segment {
            label: "Docker".into(),
            bytes: containers,
            fill: theme::bar_soft(),
        },
        Segment {
            label: "Duplicates".into(),
            bytes: duplicates,
            fill: theme::bar_soft(),
        },
    ];
    let disk = report
        .disks
        .first()
        .and_then(|disk| Some((disk.available?, disk.total?)));
    let whole = match disk {
        Some((free, size)) => {
            let used = size.saturating_sub(free);
            segments.push(Segment {
                label: "Other files".into(),
                bytes: used.saturating_sub(reclaimable),
                fill: theme::bar_rest(),
            });
            segments.push(Segment {
                label: "Free".into(),
                bytes: free,
                fill: theme::track(),
            });
            size.max(reclaimable)
        }
        None => reclaimable,
    };
    let title = if disk.is_some() {
        format!(
            "Disk  {} of {} could be reclaimed",
            theme::bytes(reclaimable),
            theme::bytes(whole)
        )
    } else {
        format!("Reclaimable  {}", theme::bytes(reclaimable))
    };
    ui::card(title).child(chart::composition("disk", &segments, whole))
}

fn overview_trends(figures: &ReportStats, charts: &Charts) -> Div {
    let ages = &charts.ages;
    let sizes = &charts.sizes;
    let ecosystems = &charts.ecosystems;
    let largest = ecosystems.first().map_or(0, |bucket| bucket.bytes);
    let total = figures.artifacts.all.bytes;
    let empty = figures.artifacts.all.count == 0;
    let removed = charts.removed.as_ref();
    div()
        .flex()
        .flex_col()
        .gap(unit(space::L))
        .child(
            div()
                .flex()
                .gap(unit(space::L))
                .items_start()
                .child(
                    ui::card("Idle since")
                        .flex_1()
                        .debug_selector(|| "chart-ages".to_owned())
                        .when(!empty, |this| {
                            this.child(chart::columns("age", ages, Measure::Bytes, "Items"))
                        })
                        .when(empty, |this| {
                            this.child(ui::empty("No project artifacts found"))
                        }),
                )
                .child(
                    ui::card("Artifact sizes")
                        .flex_1()
                        .when(!empty, |this| {
                            this.child(chart::columns("size", sizes, Measure::Count, "Items"))
                        })
                        .when(empty, |this| {
                            this.child(ui::empty("No project artifacts found"))
                        }),
                )
                .child(
                    ui::card("By ecosystem")
                        .flex_1()
                        .children(
                            ecosystems
                                .iter()
                                .take(6)
                                .enumerate()
                                .map(|(index, bucket)| {
                                    ui::bar_row(
                                        format!(
                                            "{}  {}",
                                            bucket.label,
                                            plural(bucket.count, "item")
                                        ),
                                        theme::bytes(bucket.bytes),
                                        theme::share(bucket.bytes, largest),
                                        theme::bar(),
                                    )
                                    .id(("ecosystem", index))
                                    .tip(
                                        Tooltip::details(bucket.label.clone())
                                            .row("Size", theme::bytes(bucket.bytes))
                                            .row("Items", bucket.count.to_string())
                                            .row(
                                                "Share",
                                                chart::percent(theme::share(bucket.bytes, total)),
                                            )
                                            .build(),
                                    )
                                }),
                        )
                        .when(empty, |this| {
                            this.child(ui::empty("No project artifacts found"))
                        }),
                ),
        )
        .children(removed.map(|months| {
            let total: u64 = months.iter().map(|month| month.bytes).sum();
            ui::card(format!(
                "Removed in the last 12 months  {}",
                theme::bytes(total)
            ))
            .child(chart::columns("removed", months, Measure::Bytes, "Items"))
        }))
}

fn scan_parts(
    config: &Config,
    roots: &[PathBuf],
    options: &ScanOptions,
    cancel: &Cancel,
    snapshots: &Snapshots,
    sender: &mpsc::UnboundedSender<Update>,
    shown: bool,
) {
    let saved = (!shown)
        .then(|| {
            let (snapshots, roots, options, sender) = (
                snapshots.clone(),
                roots.to_vec(),
                options.clone(),
                sender.clone(),
            );
            std::thread::Builder::new()
                .name("kuizer-gui-snapshot".to_owned())
                .spawn(move || {
                    if let Some(report) = snapshots.load(&roots, &options) {
                        let _ = sender.unbounded_send(Update::Cached(Box::new(report)));
                    }
                })
                .ok()
        })
        .flatten();
    let saved = Mutex::new(saved);
    let shown_first = || {
        if let Some(loading) = saved.lock().ok().and_then(|mut saved| saved.take()) {
            let _ = loading.join();
        }
    };
    let tracker = {
        let sender = sender.clone();
        Tracker::new(move |activity| {
            let _ = sender.unbounded_send(Update::Activity(Box::new(activity)));
        })
    };
    let recorder = Recorder::new(roots, options);
    let result = model::load(config, roots, options, cancel, &tracker, &|loaded| {
        recorder.record(&loaded);
        shown_first();
        let _ = sender.unbounded_send(Update::Loaded(loaded));
    });
    shown_first();
    let scanned = SystemTime::now();
    let finished = result.is_ok();
    let _ = sender.unbounded_send(Update::Done(result.map(|()| scanned)));
    if finished {
        recorder.save(snapshots, scanned);
    }
}

fn step_row(step: Step, activity: &Activity, options: &ScanOptions, roots: &[PathBuf]) -> Div {
    let state = activity.state(step, options);
    let running = match state {
        State::Active(running) => Some(running),
        _ => None,
    };
    let active = running.is_some();
    let (detail, took) = match &state {
        State::Off => ("Off".to_owned(), String::new()),
        State::Pending => (String::new(), String::new()),
        State::Active(running) => (activity.detail(running), String::new()),
        State::Done(done) => (done.result.clone(), activity::took(done.took)),
    };
    let quiet = matches!(state, State::Off | State::Pending);
    let line = div()
        .flex()
        .items_center()
        .gap(unit(space::L))
        .h(unit(size::ROW))
        .child(step_dot(step, &state))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(if quiet { theme::faint() } else { theme::text() })
                .when(active, |label| label.font_weight(FontWeight::MEDIUM))
                .child(step.label()),
        )
        .child(
            ui::tabular(div())
                .flex_none()
                .text_size(unit(font::SMALL))
                .text_color(if active {
                    theme::text()
                } else {
                    theme::muted()
                })
                .when(quiet, |detail| detail.text_color(theme::faint()))
                .child(detail),
        )
        .child(
            ui::tabular(column(col::XS))
                .flex()
                .justify_end()
                .text_size(unit(font::SMALL))
                .text_color(theme::faint())
                .child(took),
        );
    let mut row = div()
        .debug_selector(move || format!("step-{step:?}"))
        .flex()
        .flex_col()
        .child(line);
    if let Some(running) = running {
        let indent = unit(size::DOT + space::L);
        if let Some((done, total)) = running.count
            && total > 0
        {
            row = row.child(
                div().pl(indent).pb(unit(space::S)).child(
                    div()
                        .w_full()
                        .h(unit(size::TRACK))
                        .bg(theme::track())
                        .child(
                            div()
                                .h_full()
                                .w(gpui::relative(done as f32 / total as f32))
                                .bg(theme::bar()),
                        ),
                ),
            );
        }
        if let Some(current) = &running.current {
            row = row.child(
                div()
                    .debug_selector(|| "current".to_owned())
                    .pl(indent)
                    .pb(unit(space::S))
                    .truncate()
                    .text_size(unit(font::SMALL))
                    .text_color(theme::faint())
                    .child(model::short_path(current, roots)),
            );
        }
    }
    row
}

fn step_dot(step: Step, state: &State) -> AnyElement {
    let dot = div()
        .flex_none()
        .size(unit(size::DOT))
        .rounded(theme::radius_dot());
    match state {
        State::Done(_) => dot.bg(theme::faint()).into_any_element(),
        State::Active(_) => chart::breathe(
            ElementId::Name(format!("step-dot-{step:?}").into()),
            dot.bg(theme::primary()),
        ),
        State::Pending => dot
            .border_1()
            .border_color(theme::border_strong())
            .into_any_element(),
        State::Off => dot.bg(theme::border()).into_any_element(),
    }
}

fn found_card(activity: &Activity, roots: &[PathBuf]) -> Div {
    let total = div()
        .flex()
        .items_center()
        .justify_between()
        .child(ui::heading("Largest found so far"))
        .child(
            ui::tabular(div())
                .debug_selector(|| "found-total".to_owned())
                .text_size(unit(font::SMALL))
                .text_color(theme::muted())
                .child(format!(
                    "{}, {}",
                    plural(activity.found as usize, "item"),
                    theme::bytes(activity.bytes)
                )),
        );
    let rows: Vec<Div> = activity
        .largest
        .iter()
        .map(|found| {
            div()
                .flex()
                .items_center()
                .gap(unit(space::L))
                .h(unit(size::ROW))
                .border_t_1()
                .border_color(theme::border())
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(model::short_path(&found.path, roots)),
                )
                .child(
                    div()
                        .w(unit(col::XL))
                        .flex_none()
                        .truncate()
                        .text_size(unit(font::SMALL))
                        .text_color(theme::muted())
                        .child(found.name.clone()),
                )
                .child(
                    ui::tabular(column(col::S))
                        .flex()
                        .justify_end()
                        .child(theme::bytes(found.bytes)),
                )
        })
        .collect();
    let card = div()
        .flex()
        .flex_col()
        .min_w_0()
        .gap(unit(space::L))
        .p(unit(space::GUTTER))
        .rounded(theme::radius_panel())
        .bg(theme::panel())
        .border_1()
        .border_color(theme::border())
        .child(total);
    if rows.is_empty() {
        card.child(
            div()
                .text_size(unit(font::SMALL))
                .text_color(theme::faint())
                .child("Nothing found yet"),
        )
    } else {
        card.child(div().flex().flex_col().children(rows))
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

fn selection_details(
    stats: SelectionStats,
) -> impl Fn(&mut Window, &mut App) -> gpui::AnyView + use<> {
    move |window, cx| {
        let line = |tally: Tally| format!("{}, {}", tally.count, theme::bytes(tally.bytes));
        let total = stats.total();
        Tooltip::details("Selection")
            .row("safe", line(total.safe))
            .row("review", line(total.review))
            .row(
                "Project artifacts",
                line(stats.source(Source::Artifact).all),
            )
            .row("Global caches", line(stats.source(Source::Cache).all))
            .row("Toolchains", line(stats.source(Source::Toolchain).all))
            .row("Docker", line(stats.source(Source::Docker).all))
            .build()(window, cx)
    }
}

fn group_details(
    group: &kuizer::dupes::Group,
) -> impl Fn(&mut Window, &mut App) -> gpui::AnyView + use<> {
    let details = Tooltip::details("Duplicate group")
        .row("Copies", group.members.len().to_string())
        .row("Size of each", theme::bytes(group.size))
        .row("Reclaimable", theme::bytes(group.reclaimable()));
    if group.verified {
        details.build()
    } else {
        details.note(Term::Unverified.text()).build()
    }
}

fn permanent_kinds(stats: &SelectionStats) -> &'static str {
    let toolchains = stats.source(Source::Toolchain).all.count > 0;
    let docker = stats.source(Source::Docker).all.count > 0;
    match (toolchains, docker) {
        (true, true) => "toolchain versions and Docker objects are",
        (true, false) => "toolchain versions are",
        _ => "Docker objects are",
    }
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

#[cfg(test)]
mod tests {
    use gpui::{
        Entity, Keystroke, Modifiers, ScrollDelta, TestAppContext, TouchPhase, VisualTestContext,
        point,
    };

    use super::*;

    pub(super) fn open(
        cx: &mut TestAppContext,
        recent: PathBuf,
    ) -> (Entity<Kuizer>, &mut VisualTestContext) {
        cx.add_window_view(|window, cx| {
            let mut app = Kuizer::new(
                Config::default(),
                Vec::new(),
                Recent::load(Some(recent.clone())),
                Snapshots::new(recent.parent().map(|dir| dir.join("scans"))),
                Zoom::load(None),
                "Noto Sans".into(),
                window,
                cx,
            );
            app.options.caches = false;
            app.options.toolchains = false;
            app.options.docker = false;
            app.config_path = recent.parent().map(|dir| dir.join("config.toml"));
            app.reveal = |_| Err("no file manager in tests".to_owned());
            app
        })
    }

    pub(super) fn type_text(cx: &mut VisualTestContext, text: &str) {
        for ch in text.chars() {
            let keystroke = Keystroke {
                modifiers: Modifiers::default(),
                key: ch.to_string(),
                key_char: Some(ch.to_string()),
            };
            cx.update(|window, cx| {
                window.dispatch_keystroke(keystroke, cx);
            });
        }
        cx.run_until_parked();
    }

    fn wait_for_scan(view: &Entity<Kuizer>, cx: &mut VisualTestContext) {
        for _ in 0..1000 {
            cx.executor().advance_clock(POLL);
            cx.run_until_parked();
            if view.read_with(cx, |app, _| {
                !matches!(app.phase, Phase::Loading(_)) && app.cancel.is_none()
            }) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the scan did not finish");
    }

    fn wait_until_shown(view: &Entity<Kuizer>, cx: &mut VisualTestContext) -> bool {
        for _ in 0..1000 {
            cx.executor().advance_clock(POLL);
            cx.run_until_parked();
            if let Some(refreshing) = view.read_with(cx, |app, _| {
                (!matches!(app.phase, Phase::Loading(_))).then(|| app.stale.is_some())
            }) {
                return refreshing;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("nothing was shown");
    }

    pub(super) fn wait_for_refresh(view: &Entity<Kuizer>, cx: &mut VisualTestContext) {
        for _ in 0..1000 {
            cx.executor().advance_clock(POLL);
            cx.run_until_parked();
            if view.read_with(cx, |app, _| {
                !matches!(app.phase, Phase::Loading(_)) && !app.refreshing()
            }) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the refresh did not finish");
    }

    pub(super) fn wait_for_duplicates(view: &Entity<Kuizer>, cx: &mut VisualTestContext) {
        for _ in 0..1000 {
            cx.run_until_parked();
            if view.read_with(cx, |app, _| app.search.is_none()) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the duplicate search did not finish");
    }

    /// Waits until the finished scan is written, which happens once its results are shown.
    pub(super) fn wait_for_saved(dir: &Path) {
        for _ in 0..1000 {
            let saved = std::fs::read_dir(dir.join("scans")).is_ok_and(|mut entries| {
                entries.any(|entry| {
                    entry.is_ok_and(|entry| {
                        entry.path().extension().is_some_and(|ext| ext == "json")
                    })
                })
            });
            if saved {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the scan was not saved");
    }

    fn scan_again_from_the_start(view: &Entity<Kuizer>, cx: &mut VisualTestContext) {
        view.update(cx, |app, cx| app.new_scan(cx));
        cx.simulate_keystrokes("enter");
    }

    pub(super) fn project(dir: &Path) -> PathBuf {
        let root = dir.join("apps");
        let modules = root.join("site").join("node_modules").join("left-pad");
        std::fs::create_dir_all(&modules).unwrap();
        std::fs::write(root.join("site").join("package.json"), "{}").unwrap();
        std::fs::write(modules.join("index.js"), vec![b'x'; 64 * 1024]).unwrap();
        root
    }

    #[gpui::test]
    fn opens_on_the_start_screen_without_a_path(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (view, cx) = open(cx, dir.path().join("recent"));
        view.read_with(cx, |app, _| {
            assert!(matches!(app.phase, Phase::Start));
            assert!(app.folder.is_empty());
        });
    }

    #[gpui::test]
    fn a_missing_folder_stays_on_the_start_screen(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (view, cx) = open(cx, dir.path().join("recent"));
        type_text(cx, &dir.path().join("missing").display().to_string());
        cx.simulate_keystrokes("enter");
        view.read_with(cx, |app, _| {
            assert!(matches!(app.phase, Phase::Start));
            assert_eq!(
                app.folder_error.as_deref(),
                Some("This folder does not exist")
            );
        });
    }

    #[gpui::test]
    fn ctrl_backspace_removes_the_last_folder(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (view, cx) = open(cx, dir.path().join("recent"));
        type_text(cx, "/home/me/apps");
        cx.simulate_keystrokes("ctrl-backspace");
        view.read_with(cx, |app, _| assert_eq!(app.folder, "/home/me/"));
        cx.simulate_keystrokes("backspace");
        view.read_with(cx, |app, _| assert_eq!(app.folder, "/home/me"));
    }

    #[gpui::test]
    fn enter_scans_the_typed_folder_and_remembers_it(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let root = project(dir.path());
        let recent = dir.path().join("recent");
        let (view, cx) = open(cx, recent.clone());
        type_text(cx, &root.display().to_string());
        cx.simulate_keystrokes("enter");
        view.read_with(cx, |app, _| assert!(matches!(app.phase, Phase::Loading(_))));
        wait_for_scan(&view, cx);
        let canonical = std::fs::canonicalize(&root).unwrap();
        view.read_with(cx, |app, _| {
            let Phase::Ready(report) = &app.phase else {
                panic!("expected a report");
            };
            assert_eq!(report.roots, vec![canonical.clone()]);
            assert_eq!(report.candidates.len(), 1);
            assert!(report.caches.is_empty());
        });
        crate::persist::flush();
        let saved = std::fs::read_to_string(&recent).unwrap();
        assert_eq!(saved.trim(), canonical.display().to_string());

        view.update(cx, |app, cx| app.new_scan(cx));
        view.read_with(cx, |app, _| {
            assert!(matches!(app.phase, Phase::Start));
            assert_eq!(app.recent.paths, vec![canonical.clone()]);
        });
    }

    #[gpui::test]
    fn escape_cancels_a_running_scan(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let root = project(dir.path());
        let (view, cx) = open(cx, dir.path().join("recent"));
        type_text(cx, &root.display().to_string());
        cx.simulate_keystrokes("enter");
        cx.simulate_keystrokes("escape");
        view.read_with(cx, |app, _| {
            assert!(matches!(app.phase, Phase::Start));
            assert!(app.cancel.is_none());
            assert!(app.task.is_none());
        });
    }

    #[gpui::test]
    fn the_whole_home_option_ignores_the_typed_folder(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (view, cx) = open(cx, dir.path().join("recent"));
        type_text(cx, "/does/not/matter");
        view.update(cx, |app, cx| {
            app.whole_home = true;
            app.options.duplicates = false;
            app.start_scan(cx);
        });
        cx.run_until_parked();
        view.update(cx, |app, cx| {
            assert!(matches!(app.phase, Phase::Loading(_)));
            assert_eq!(
                app.roots,
                scan::normalize_roots(&[platform::home_dir().unwrap()])
            );
            app.cancel_scan(cx);
        });
    }

    #[gpui::test]
    fn the_scan_view_shows_each_step_and_what_it_touches(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (view, cx) = open(cx, dir.path().join("recent"));
        view.update(cx, |app, cx| {
            app.options.caches = true;
            let mut activity = Activity::default();
            activity.record(Path::new("/p/site/node_modules"), "node_modules", 3 << 30);
            activity.finish(
                Step::Folders,
                Duration::from_millis(1_500),
                "1 item found".to_owned(),
            );
            activity.begin(Step::Caches, Some(4));
            activity.advance(Step::Caches, Path::new("/home/me/.cargo/registry"));
            app.phase = Phase::Loading(Box::new(activity));
            cx.notify();
        });
        cx.update(|window, cx| window.draw(cx).clear());
        for selector in [
            "activity",
            "step-Folders",
            "step-Caches",
            "step-Docker",
            "current",
            "found-total",
            "Shown once every part of the scan is done",
        ] {
            assert!(
                cx.debug_bounds(selector).is_some(),
                "{selector} is not shown"
            );
        }
        let folders = cx.debug_bounds("step-Folders").unwrap();
        let caches = cx.debug_bounds("step-Caches").unwrap();
        assert!(
            caches.size.height > folders.size.height,
            "the running step shows its progress and current folder below it"
        );
    }

    #[gpui::test]
    fn the_folder_field_skips_recent_folders_that_are_gone(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let kept = dir.path().join("kept");
        std::fs::create_dir_all(&kept).unwrap();
        let recent = dir.path().join("recent");
        std::fs::write(
            &recent,
            format!("/tmp/.mount_kuizerGone/usr\n{}\n", kept.display()),
        )
        .unwrap();
        let (view, cx) = open(cx, recent);
        cx.run_until_parked();
        view.read_with(cx, |app, _| {
            assert_eq!(app.folder, display_path(&kept));
        });
    }

    #[gpui::test]
    fn a_scan_shows_its_progress_then_charts_with_tooltips(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let root = project(dir.path());
        let (view, cx) = open(cx, dir.path().join("recent"));
        type_text(cx, &root.display().to_string());
        cx.simulate_keystrokes("enter");
        cx.update(|window, cx| window.draw(cx).clear());
        assert!(cx.debug_bounds("activity").is_some());
        assert!(cx.debug_bounds("loader").is_some());
        wait_for_scan(&view, cx);
        view.read_with(cx, |app, _| assert!(!app.busy()));
        cx.update(|window, cx| window.draw(cx).clear());
        assert!(cx.debug_bounds("chart-ages").is_some());
        let segment = cx
            .debug_bounds("disk-0")
            .expect("the safe artifacts segment");
        cx.simulate_event(MouseMoveEvent {
            position: segment.center(),
            pressed_button: None,
            modifiers: Modifiers::default(),
        });
        cx.executor().advance_clock(Duration::from_millis(700));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear());
        assert!(cx.debug_bounds("tooltip").is_some());
        let legend = cx
            .debug_bounds("disk-legend-2")
            .expect("the global caches legend item");
        cx.simulate_mouse_move(legend.center(), None, Modifiers::default());
        cx.executor().advance_clock(Duration::from_millis(700));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear());
        let tip = cx.debug_bounds("tooltip").expect("the legend tooltip");
        assert!(tip.origin.y >= legend.bottom());
    }

    #[gpui::test]
    fn the_titlebar_search_opens_the_palette(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (view, cx) = open(cx, dir.path().join("recent"));
        cx.update(|window, cx| window.draw(cx).clear());
        let search = cx
            .debug_bounds("search")
            .expect("the titlebar search field");
        cx.simulate_click(search.center(), Modifiers::default());
        view.read_with(cx, |app, _| assert!(app.palette.is_some()));
    }

    #[gpui::test]
    fn the_sidebar_switches_sections_during_a_scan(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (view, cx) = open(cx, dir.path().join("recent"));
        view.update(cx, |app, cx| {
            app.roots = vec![dir.path().to_path_buf()];
            let mut activity = Activity::default();
            activity.begin(Step::Folders, None);
            activity.begin(Step::History, None);
            app.phase = Phase::Loading(Box::new(activity));
            cx.notify();
        });
        for (label, shown) in [
            ("Global caches", "Global caches are off for this scan"),
            ("History", "Reading the cleanup history"),
            ("Duplicates", "Searched on request once the scan is done"),
        ] {
            cx.update(|window, cx| window.draw(cx).clear());
            let bounds = cx.debug_bounds(label).expect("a sidebar item");
            cx.simulate_click(bounds.center(), Modifiers::default());
            view.read_with(cx, |app, _| {
                assert!(matches!(app.phase, Phase::Loading(_)));
                assert_eq!(app.tab.label(), label);
            });
            cx.update(|window, cx| window.draw(cx).clear());
            assert!(cx.debug_bounds(shown).is_some(), "{shown} is not shown");
        }
    }

    fn artifact(id: u64, path: &Path) -> Candidate {
        Candidate {
            id,
            path: path.to_path_buf(),
            kind: kuizer::detect::Kind::Dir,
            rule: std::sync::Arc::new(kuizer::detect::RuleInfo {
                name: "node_modules".to_owned(),
                category: Category::Dependencies,
                safety: Safety::Safe,
                ecosystem: "node".to_owned(),
            }),
            project: path.parent().map(Path::to_path_buf),
            usage: kuizer::scan::Usage {
                allocated: id * 4096,
                apparent: id * 4096,
                entries: 1,
                partial: false,
            },
            newest: None,
            identity: None,
            project_last_active: None,
        }
    }

    #[gpui::test]
    fn parts_fill_the_shown_scan_while_deletion_waits(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (view, cx) = open(cx, dir.path().join("recent"));
        let (a, b) = (
            Path::new("/p/a/node_modules"),
            Path::new("/p/b/node_modules"),
        );
        view.update(cx, |app, cx| {
            app.roots = vec![PathBuf::from("/p")];
            app.cancel = Some(Cancel::new());
            app.pending = Part::expected(&app.options);
            app.phase = Phase::Loading(Box::default());
            let history = Loaded::History(Err("unreadable".to_owned()));
            app.take(
                vec![
                    Update::Loaded(history),
                    Update::Loaded(Loaded::Found(vec![artifact(2, b)])),
                ],
                cx,
            );
            let report = app.report().expect("the first artifacts are shown");
            assert_eq!(report.candidates.len(), 1);
            assert!(report.history.is_err(), "an earlier part is kept");
            assert!(app.refreshing());
            assert!(!app.pending.contains(&Part::History));

            app.select((Source::Artifact, 2));
            app.take(
                vec![Update::Loaded(Loaded::Found(vec![artifact(5, a)]))],
                cx,
            );
            let paths: Vec<&Path> = app
                .report()
                .unwrap()
                .candidates
                .iter()
                .map(|candidate| candidate.path.as_path())
                .collect();
            assert_eq!(paths, [a, b], "largest first");

            let mut saved = Report::empty(&app.roots, &app.options);
            saved.candidates = vec![artifact(9, Path::new("/p/old/node_modules"))];
            saved.history = Ok(HistoryView {
                history: kuizer::history::History::default(),
                kept: HashSet::new(),
                trash_known: true,
            });
            app.take(vec![Update::Cached(Box::new(saved))], cx);
            let report = app.report().unwrap();
            assert_eq!(report.candidates.len(), 2, "streamed artifacts win");
            assert!(report.history.is_err(), "a fresh part is never replaced");
        });
        cx.update(|window, cx| view.update(cx, |app, cx| app.open_confirm(window, cx)));
        view.read_with(cx, |app, _| {
            assert!(app.dialog.is_none(), "nothing is removed mid scan");
        });

        view.update(cx, |app, cx| {
            let summary = app.report().unwrap().summary;
            let artifacts = Loaded::Artifacts {
                candidates: vec![artifact(5, a), artifact(2, b)],
                issues: Vec::new(),
                summary,
                disks: Vec::new(),
            };
            app.take(
                vec![
                    Update::Loaded(artifacts),
                    Update::Loaded(Loaded::Repos(Vec::new())),
                    Update::Done(Ok(SystemTime::UNIX_EPOCH)),
                ],
                cx,
            );
            assert!(!app.refreshing());
            assert_eq!(app.selection, HashSet::from([(Source::Artifact, 2)]));
            assert_eq!(app.report().unwrap().scanned, SystemTime::UNIX_EPOCH);
        });
        cx.update(|window, cx| view.update(cx, |app, cx| app.open_confirm(window, cx)));
        view.read_with(cx, |app, _| assert!(app.dialog.is_some()));
    }

    #[gpui::test]
    fn the_sidebar_collapses_to_icons_and_back(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let root = project(dir.path());
        let (view, cx) = open(cx, dir.path().join("recent"));
        type_text(cx, &root.display().to_string());
        cx.simulate_keystrokes("enter");
        wait_for_scan(&view, cx);
        cx.update(|window, cx| window.draw(cx).clear());
        let wide = cx.debug_bounds("sidebar").expect("the sidebar").size.width;
        cx.simulate_keystrokes("ctrl-b");
        view.read_with(cx, |app, _| assert!(app.collapsed));
        cx.update(|window, cx| window.draw(cx).clear());
        let narrow = cx.debug_bounds("sidebar").expect("the sidebar").size.width;
        assert!(narrow < wide / 2.);
        let history = cx.debug_bounds("History").expect("a sidebar icon");
        cx.simulate_click(history.center(), Modifiers::default());
        view.read_with(cx, |app, _| assert_eq!(app.tab, Tab::History));
        let toggle = cx
            .debug_bounds("sidebar-toggle")
            .expect("the collapse button");
        cx.simulate_click(toggle.center(), Modifiers::default());
        view.read_with(cx, |app, _| assert!(!app.collapsed));
    }

    #[gpui::test]
    fn the_sidebar_opens_collapsed_when_configured(cx: &mut TestAppContext) {
        let mut config = Config::default();
        config.appearance.sidebar = Sidebar::Collapsed;
        let (view, cx) = cx.add_window_view(|window, cx| {
            Kuizer::new(
                config,
                Vec::new(),
                Recent::load(None),
                Snapshots::new(None),
                Zoom::load(None),
                "Noto Sans".into(),
                window,
                cx,
            )
        });
        view.read_with(cx, |app, _| assert!(app.collapsed));
    }

    #[gpui::test]
    fn the_sidebar_shows_what_each_section_frees(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let root = project(dir.path());
        let (view, cx) = open(cx, dir.path().join("recent"));
        type_text(cx, &root.display().to_string());
        cx.simulate_keystrokes("enter");
        wait_for_scan(&view, cx);
        view.read_with(cx, |app, _| {
            let report = app.report().expect("a report");
            assert_eq!(
                app.tab_figure(Tab::Artifacts),
                Some(theme::bytes(report.total(None)))
            );
            assert_eq!(app.tab_figure(Tab::Overview), None);
            assert_eq!(app.tab_figure(Tab::Issues), None);
        });
    }

    #[gpui::test]
    fn hovering_a_section_explains_it(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let root = project(dir.path());
        let (view, cx) = open(cx, dir.path().join("recent"));
        type_text(cx, &root.display().to_string());
        cx.simulate_keystrokes("enter");
        wait_for_scan(&view, cx);
        cx.update(|window, cx| window.draw(cx).clear());
        let history = cx.debug_bounds("History").expect("a sidebar item");
        let issues = cx.debug_bounds("Issues").expect("a sidebar item");
        cx.simulate_mouse_move(history.center(), None, Modifiers::default());
        cx.executor().advance_clock(Duration::from_millis(600));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear());
        let tip = cx.debug_bounds("tooltip").expect("the History tooltip");
        assert!(tip.origin.y >= history.bottom());
        view.read_with(cx, |app, _| {
            assert_eq!(app.icon_epoch(Icon::History), Some(1));
        });
        cx.simulate_mouse_move(issues.center(), None, Modifiers::default());
        cx.update(|window, cx| window.draw(cx).clear());
        let next = cx
            .debug_bounds("tooltip")
            .expect("the Issues tooltip opens at once");
        assert!(next.origin.y >= issues.bottom());
    }

    fn scroll(cx: &mut VisualTestContext, lines: f32, modifiers: Modifiers) {
        cx.simulate_event(ScrollWheelEvent {
            position: point(px(400.), px(300.)),
            delta: ScrollDelta::Lines(point(0., lines)),
            modifiers,
            touch_phase: TouchPhase::Moved,
        });
    }

    #[gpui::test]
    fn ctrl_plus_minus_and_zero_change_the_zoom(cx: &mut TestAppContext) {
        cx.update(bind_keys);
        let dir = tempfile::tempdir().unwrap();
        let (view, cx) = open(cx, dir.path().join("recent"));
        cx.simulate_keystrokes("ctrl-= ctrl-shift-=");
        view.read_with(cx, |app, _| assert_eq!(app.zoom.percent, 120));
        cx.update(|window, _| assert_eq!(window.rem_size(), px(19.2)));
        cx.simulate_keystrokes("ctrl-- ctrl-- ctrl-- ctrl--");
        view.read_with(cx, |app, _| {
            assert_eq!(app.zoom.percent, 80);
            assert!(app.folder.is_empty());
        });
        cx.simulate_keystrokes("ctrl-0");
        view.read_with(cx, |app, _| assert_eq!(app.zoom.percent, 100));
    }

    #[gpui::test]
    fn ctrl_scroll_zooms_and_plain_scroll_does_not(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (view, cx) = open(cx, dir.path().join("recent"));
        scroll(cx, 1., Modifiers::control());
        scroll(cx, 1., Modifiers::control());
        view.read_with(cx, |app, _| assert_eq!(app.zoom.percent, 120));
        scroll(cx, -1., Modifiers::control());
        view.read_with(cx, |app, _| assert_eq!(app.zoom.percent, 110));
        scroll(cx, 1., Modifiers::default());
        view.read_with(cx, |app, _| assert_eq!(app.zoom.percent, 110));
    }

    #[gpui::test]
    fn a_known_folder_opens_with_the_last_scan_while_it_refreshes(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let root = project(dir.path());
        let (view, cx) = open(cx, dir.path().join("recent"));
        type_text(cx, &root.display().to_string());
        cx.simulate_keystrokes("enter");
        assert!(!wait_until_shown(&view, cx));
        wait_for_refresh(&view, cx);
        wait_for_saved(dir.path());

        let id = view.update(cx, |app, cx| {
            let report = app.report().unwrap();
            let saved = app
                .snapshots
                .load(&report.roots, &report.options)
                .expect("the saved scan");
            app.set_phase(Phase::Loading(Box::default()), cx);
            app.cancel = Some(Cancel::new());
            app.pending = Part::expected(&app.options);
            app.take(vec![Update::Cached(Box::new(saved))], cx);
            let report = app.report().expect("the saved scan comes first");
            assert_eq!(report.candidates.len(), 1);
            report.candidates[0].id
        });
        cx.update(|window, cx| window.draw(cx).clear());
        assert!(cx.debug_bounds("stale").is_some());
        assert!(cx.debug_bounds("loader").is_some());

        view.update(cx, |app, cx| app.toggle(Source::Artifact, id, cx));
        cx.update(|window, cx| view.update(cx, |app, cx| app.open_confirm(window, cx)));
        view.read_with(cx, |app, _| {
            assert!(app.dialog.is_none(), "stale items cannot be trashed");
            assert_eq!(app.selected().len(), 1);
        });

        view.update(cx, |app, cx| app.rescan(cx));
        wait_for_refresh(&view, cx);
        view.read_with(cx, |app, _| {
            assert!(matches!(app.phase, Phase::Ready(_)));
            assert_eq!(app.selected().len(), 1);
            assert!(app.stale.is_none());
        });
        cx.update(|window, cx| view.update(cx, |app, cx| app.open_confirm(window, cx)));
        view.read_with(cx, |app, _| assert!(app.dialog.is_some()));
    }

    #[gpui::test]
    fn a_narrow_titlebar_never_overlaps_while_refreshing(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let root = project(dir.path());
        let (view, cx) = open(cx, dir.path().join("recent"));
        type_text(cx, &root.display().to_string());
        cx.simulate_keystrokes("enter");
        wait_for_refresh(&view, cx);
        view.update(cx, |app, cx| {
            app.stale = Some(SystemTime::now());
            cx.notify();
        });

        cx.simulate_resize(gpui::size(px(860.), px(520.)));
        cx.update(|window, cx| window.draw(cx).clear());
        let mut bounds = |selector: &'static str| {
            cx.debug_bounds(selector)
                .unwrap_or_else(|| panic!("{selector} is painted"))
        };
        let brand = bounds("brand");
        let search = bounds("search");
        let stale = bounds("stale");
        let actions = bounds("titlebar-actions");
        let titlebar = bounds("titlebar");
        assert!(brand.right() <= search.left());
        assert!(search.right() <= stale.left());
        assert!(actions.right() <= titlebar.right());
    }

    #[gpui::test]
    fn the_fresh_scan_keeps_the_selection_by_path(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let root = project(dir.path());
        let gone = root.join("old").join("node_modules");
        std::fs::create_dir_all(gone.join("x")).unwrap();
        std::fs::write(root.join("old").join("package.json"), "{}").unwrap();
        std::fs::write(gone.join("x").join("index.js"), vec![b'x'; 8 * 1024]).unwrap();
        let (view, cx) = open(cx, dir.path().join("recent"));
        type_text(cx, &root.display().to_string());
        cx.simulate_keystrokes("enter");
        wait_for_refresh(&view, cx);
        view.update(cx, |app, cx| {
            let Phase::Ready(report) = &mut app.phase else {
                panic!("expected a report");
            };
            assert_eq!(report.candidates.len(), 2);
            for candidate in &mut report.candidates {
                candidate.id += 100;
            }
            let keys: Vec<(Source, u64)> = report
                .candidates
                .iter()
                .map(|candidate| (Source::Artifact, candidate.id))
                .collect();
            app.refresh_stats();
            for key in keys {
                app.select(key);
            }
            app.refresh_rows();
            cx.notify();
        });
        std::fs::remove_dir_all(&gone).unwrap();

        view.update(cx, |app, cx| app.rescan(cx));
        view.read_with(cx, |app, _| {
            assert!(app.refreshing());
            assert!(matches!(app.phase, Phase::Ready(_)));
            assert_eq!(app.selection.len(), 2);
        });
        wait_for_refresh(&view, cx);
        view.read_with(cx, |app, _| {
            let report = app.report().expect("the fresh scan");
            assert_eq!(report.candidates.len(), 1);
            let kept = &report.candidates[0];
            assert!(kept.id < 100);
            assert!(kept.path.ends_with("site/node_modules"));
            assert_eq!(app.selection, HashSet::from([(Source::Artifact, kept.id)]));
        });
    }

    const ISOLATED: &str = "KUIZER_GUI_TEST_BASE";

    /// Runs one ignored test in a child process whose HOME and XDG folders point into a temporary
    /// folder, like the CLI tests in tests/restore.rs, so the trash and the history it touches
    /// belong to that test alone.
    fn isolated(name: &str) {
        let dir = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(dir.path()).unwrap();
        let home = base.join("home");
        std::fs::create_dir_all(home.join(".local/share")).unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                &format!("app::tests::{name}"),
                "--ignored",
                "--test-threads=1",
            ])
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_CACHE_HOME", home.join(".cache"))
            .env_remove("RUSTUP_HOME")
            .env_remove("NVM_DIR")
            .env_remove("FNM_DIR")
            .env_remove("VOLTA_HOME")
            .env_remove("PYENV_ROOT")
            .env_remove("UV_PYTHON_INSTALL_DIR")
            .env_remove("SDKMAN_DIR")
            .env(ISOLATED, &base)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success() && stdout.contains("1 passed"),
            "{stdout}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn isolated_base() -> Option<PathBuf> {
        std::env::var_os(ISOLATED).map(PathBuf::from)
    }

    fn wait_until(
        view: &Entity<Kuizer>,
        cx: &mut VisualTestContext,
        what: &str,
        done: impl Fn(&Kuizer) -> bool,
    ) {
        for _ in 0..1000 {
            cx.executor().advance_clock(POLL);
            cx.run_until_parked();
            if view.read_with(cx, |app, _| done(app)) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("{what} did not happen");
    }

    fn toast_is(view: &Entity<Kuizer>, cx: &mut VisualTestContext, check: fn(&Notice) -> bool) {
        let shown = view.read_with(cx, |app, _| {
            app.toast.as_ref().is_some_and(|toast| check(&toast.notice))
        });
        assert!(shown, "unexpected toast");
    }

    fn trash_the_project<'a>(
        base: &Path,
        cx: &'a mut TestAppContext,
    ) -> (Entity<Kuizer>, &'a mut VisualTestContext, PathBuf) {
        cx.update(bind_keys);
        let root = project(&base.join("work"));
        let modules = std::fs::canonicalize(root.join("site").join("node_modules")).unwrap();
        let (view, cx) = open(cx, base.join("recent"));
        type_text(cx, &root.display().to_string());
        cx.simulate_keystrokes("enter");
        wait_for_refresh(&view, cx);
        let id = view.read_with(cx, |app, _| app.report().unwrap().candidates[0].id);
        view.update(cx, |app, cx| app.toggle(Source::Artifact, id, cx));
        cx.update(|window, cx| view.update(cx, |app, cx| app.open_confirm(window, cx)));
        cx.simulate_keystrokes("enter");
        wait_until(&view, cx, "the trash", |app| {
            matches!(
                &app.toast,
                Some(Toast {
                    notice: Notice::Trashed { .. },
                    ..
                })
            )
        });
        assert!(!modules.exists());
        view.read_with(cx, |app, _| {
            assert!(app.dialog.is_none(), "a clean run needs no summary dialog");
            assert!(app.report().unwrap().candidates.is_empty());
        });
        (view, cx, modules)
    }

    fn restore_runs() -> Vec<kuizer::history::Run> {
        let path = kuizer::history::default_path().unwrap();
        kuizer::history::load(&path)
            .unwrap()
            .runs
            .into_iter()
            .filter(|run| run.action == Action::Restore)
            .collect()
    }

    #[test]
    fn undo_puts_the_trashed_items_back() {
        isolated("undo_puts_the_trashed_items_back_inside");
    }

    #[gpui::test]
    #[ignore = "runs in its own process through undo_puts_the_trashed_items_back"]
    fn undo_puts_the_trashed_items_back_inside(cx: &mut TestAppContext) {
        let Some(base) = isolated_base() else {
            return;
        };
        let (view, cx, modules) = trash_the_project(&base, cx);
        cx.update(|window, cx| window.draw(cx).clear());
        assert!(cx.debug_bounds("toast").is_some());
        let undo = cx.debug_bounds("undo").expect("an Undo button");
        cx.simulate_click(undo.center(), Modifiers::default());
        toast_is(&view, cx, |notice| {
            matches!(
                notice,
                Notice::Restoring { session: 1 } | Notice::Restored(_)
            )
        });

        wait_until(&view, cx, "the restore", |app| !app.restoring());
        toast_is(
            &view,
            cx,
            |notice| matches!(notice, Notice::Restored(restored) if restored.items == 1 && restored.problems.is_empty()),
        );
        assert!(modules.join("left-pad").join("index.js").is_file());
        let runs = restore_runs();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].deleted[0].restores.as_deref(), Some("s1.1"));

        wait_for_refresh(&view, cx);
        view.read_with(cx, |app, _| {
            let report = app.report().unwrap();
            assert_eq!(report.candidates.len(), 1, "the item is back in the list");
            let history = report.history.as_ref().unwrap();
            assert_eq!(history.history.cumulative.restored_items, 1);
            assert_eq!(history.trash_state(0), TrashState::Restored);
        });
    }

    #[test]
    fn docker_objects_are_removed_through_the_engine() {
        isolated("docker_objects_are_removed_through_the_engine_inside");
    }

    #[cfg(unix)]
    #[gpui::test]
    #[ignore = "runs in its own process through docker_objects_are_removed_through_the_engine"]
    fn docker_objects_are_removed_through_the_engine_inside(cx: &mut TestAppContext) {
        use std::os::unix::fs::PermissionsExt;

        let Some(base) = isolated_base() else {
            return;
        };
        cx.update(bind_keys);
        let bin = base.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let engine = bin.join("docker");
        std::fs::write(
            &engine,
            r#"#!/bin/sh
echo "$*" >> "$(dirname "$0")/calls"
case "$1 $2" in
  "image ls") printf 'sha256:aaaaaaaaaaaaaaaa\t1.5GB\t3 weeks ago\n' ;;
  "volume ls") echo pgdata ;;
  *) exit 0 ;;
esac
"#,
        )
        .unwrap();
        std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o755)).unwrap();
        let root = project(&base.join("work"));
        let (view, cx) = open(cx, base.join("recent"));
        view.update(cx, |app, _| {
            app.options.docker = true;
            app.config.docker.engines = vec![engine.display().to_string()];
        });
        type_text(cx, &root.display().to_string());
        cx.simulate_keystrokes("enter");
        wait_for_refresh(&view, cx);
        let image = view.read_with(cx, |app, _| {
            let docker = &app.report().unwrap().docker;
            assert_eq!(docker.len(), 2);
            assert_eq!(docker[0].1.rule.safety, Safety::Safe);
            assert_eq!(docker[1].0.object, kuizer::docker::Object::Volume);
            assert_eq!(docker[1].1.rule.safety, Safety::Review);
            docker[0].1.id
        });

        cx.simulate_keystrokes("5");
        view.read_with(cx, |app, _| assert_eq!(app.tab, Tab::Docker));
        cx.update(|window, cx| window.draw(cx).clear());
        assert!(cx.debug_bounds("docker-row-1").is_some());
        let target = view.read_with(cx, |app, _| app.targets(Tab::Docker)[0].clone());
        let menu = view.read_with(cx, |app, _| app.menu_items(&target));
        let reveal = menu
            .iter()
            .find(|item| item.choice == Choice::Reveal)
            .unwrap();
        assert!(!reveal.enabled, "a Docker object has no folder to reveal");
        view.update(cx, |app, cx| app.toggle(Source::Docker, image, cx));
        cx.update(|window, cx| view.update(cx, |app, cx| app.open_confirm(window, cx)));
        cx.update(|window, cx| window.draw(cx).clear());
        assert!(cx.debug_bounds("permanent-warning").is_some());
        cx.simulate_keystrokes("enter");
        wait_until(&view, cx, "the removal", |app| {
            matches!(app.dialog, Some(Dialog::Finished(_)))
        });
        let calls = std::fs::read_to_string(bin.join("calls")).unwrap();
        assert_eq!(calls.lines().last(), Some("image rm aaaaaaaaaaaaaaaa"));
        view.read_with(cx, |app, _| {
            assert!(app.toast.is_none(), "nothing to undo");
            assert_eq!(app.report().unwrap().docker.len(), 1);
        });
    }

    #[test]
    fn toolchains_are_deleted_for_good_without_undo() {
        isolated("toolchains_are_deleted_for_good_without_undo_inside");
    }

    #[gpui::test]
    #[ignore = "runs in its own process through toolchains_are_deleted_for_good_without_undo"]
    fn toolchains_are_deleted_for_good_without_undo_inside(cx: &mut TestAppContext) {
        let Some(base) = isolated_base() else {
            return;
        };
        cx.update(bind_keys);
        let versions = base.join("home/.nvm/versions/node");
        let old = versions.join("v18.17.0");
        std::fs::create_dir_all(old.join("bin")).unwrap();
        std::fs::write(old.join("bin/node"), vec![0u8; 4096]).unwrap();
        std::fs::create_dir_all(versions.join("v20.11.0/bin")).unwrap();
        let root = project(&base.join("work"));
        std::fs::write(root.join("site/.nvmrc"), "20\n").unwrap();
        let (view, cx) = open(cx, base.join("recent"));
        view.update(cx, |app, _| app.options.toolchains = true);
        type_text(cx, &root.display().to_string());
        cx.simulate_keystrokes("enter");
        wait_for_refresh(&view, cx);
        let (id, pinned) = view.read_with(cx, |app, _| {
            let toolchains = &app.report().unwrap().toolchains;
            assert_eq!(toolchains.len(), 2);
            let find = |version: &str| {
                toolchains
                    .iter()
                    .find(|(toolchain, _)| toolchain.version == version)
                    .unwrap()
            };
            assert_eq!(find("v18.17.0").1.rule.safety, Safety::Safe);
            (find("v18.17.0").1.id, find("v20.11.0").0.clone())
        });
        assert_eq!(pinned.safety(), Safety::Review);
        assert!(pinned.note().contains(".nvmrc"), "{}", pinned.note());

        cx.simulate_keystrokes("4");
        view.read_with(cx, |app, _| assert_eq!(app.tab, Tab::Toolchains));
        cx.update(|window, cx| window.draw(cx).clear());
        assert!(cx.debug_bounds("toolchain-row-1").is_some());
        view.update(cx, |app, cx| app.toggle(Source::Toolchain, id, cx));
        cx.update(|window, cx| view.update(cx, |app, cx| app.open_confirm(window, cx)));
        cx.update(|window, cx| window.draw(cx).clear());
        assert!(cx.debug_bounds("permanent-warning").is_some());
        cx.simulate_keystrokes("enter");
        wait_until(&view, cx, "the removal", |app| {
            matches!(app.dialog, Some(Dialog::Finished(_)))
        });
        assert!(!old.exists());
        assert!(versions.join("v20.11.0").exists());
        view.read_with(cx, |app, _| {
            assert!(app.toast.is_none(), "nothing to undo");
            let Some(Dialog::Finished(finished)) = &app.dialog else {
                unreachable!()
            };
            assert_eq!((finished.removed, finished.deleted), (1, 1));
            assert_eq!(app.report().unwrap().toolchains.len(), 1);
        });
        let path = kuizer::history::default_path().unwrap();
        let runs = kuizer::history::load(&path).unwrap().runs;
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].deleted[0].mode, kuizer::delete::Mode::Permanent);
    }

    #[test]
    fn restore_in_history_puts_a_run_back() {
        isolated("restore_in_history_puts_a_run_back_inside");
    }

    #[gpui::test]
    #[ignore = "runs in its own process through restore_in_history_puts_a_run_back"]
    fn restore_in_history_puts_a_run_back_inside(cx: &mut TestAppContext) {
        let Some(base) = isolated_base() else {
            return;
        };
        let (view, cx, modules) = trash_the_project(&base, cx);
        cx.simulate_keystrokes("escape");
        view.read_with(cx, |app, _| assert!(app.toast.is_none()));
        cx.update(|window, cx| window.draw(cx).clear());
        let history = cx.debug_bounds("History").expect("a sidebar item");
        cx.simulate_click(history.center(), Modifiers::default());
        cx.update(|window, cx| window.draw(cx).clear());
        let restore = cx.debug_bounds("restore-s1").expect("a Restore button");
        cx.simulate_click(restore.center(), Modifiers::default());
        wait_until(&view, cx, "the restore", |app| !app.restoring());
        toast_is(
            &view,
            cx,
            |notice| matches!(notice, Notice::Restored(restored) if restored.items == 1 && restored.problems.is_empty()),
        );
        assert!(modules.is_dir());
        assert_eq!(restore_runs().len(), 1);
        wait_for_refresh(&view, cx);
        view.read_with(cx, |app, _| {
            let report = app.report().unwrap();
            assert_eq!(report.candidates.len(), 1);
            let history = report.history.as_ref().unwrap();
            assert_eq!(
                history.trash_state(0),
                TrashState::Restored,
                "nothing is left to restore"
            );
        });
    }

    #[test]
    fn a_failed_undo_explains_why() {
        isolated("a_failed_undo_explains_why_inside");
    }

    #[gpui::test]
    #[ignore = "runs in its own process through a_failed_undo_explains_why"]
    fn a_failed_undo_explains_why_inside(cx: &mut TestAppContext) {
        let Some(base) = isolated_base() else {
            return;
        };
        let (view, cx, modules) = trash_the_project(&base, cx);
        std::fs::create_dir_all(&modules).unwrap();
        cx.simulate_keystrokes("ctrl-z");
        wait_until(&view, cx, "the restore", |app| !app.restoring());
        toast_is(&view, cx, |notice| {
            matches!(notice, Notice::Restored(restored) if restored.items == 0
                && restored.problems.len() == 1
                && restored.problems[0].1.ends_with("exists, not overwriting"))
        });
        cx.update(|window, cx| window.draw(cx).clear());
        assert!(cx.debug_bounds("toast-error").is_some());
        assert!(
            !modules.join("left-pad").exists(),
            "nothing was overwritten"
        );
        assert!(restore_runs().is_empty());
        let trashed = kuizer::history::trashed_paths().unwrap();
        assert!(trashed.contains(&modules), "the item stays in the trash");

        cx.executor().advance_clock(TOAST_TIME * 2);
        cx.run_until_parked();
        view.read_with(cx, |app, _| {
            assert!(app.toast.is_some(), "an error stays until it is dismissed");
            assert!(!app.refreshing());
        });
    }

    #[gpui::test]
    fn the_undo_toast_hides_itself_and_waits_for_a_refresh(cx: &mut TestAppContext) {
        cx.update(bind_keys);
        let dir = tempfile::tempdir().unwrap();
        let root = project(dir.path());
        let (view, cx) = open(cx, dir.path().join("recent"));
        type_text(cx, &root.display().to_string());
        cx.simulate_keystrokes("enter");
        wait_for_refresh(&view, cx);
        let trashed = || Notice::Trashed {
            session: 1,
            items: 2,
            bytes: 2048,
        };
        view.update(cx, |app, cx| app.show_toast(trashed(), cx));
        cx.update(|window, cx| window.draw(cx).clear());
        assert!(cx.debug_bounds("toast").is_some());
        cx.executor().advance_clock(TOAST_TIME - POLL * 2);
        cx.run_until_parked();
        view.read_with(cx, |app, _| assert!(app.toast.is_some()));
        cx.executor().advance_clock(POLL * 3);
        cx.run_until_parked();
        view.read_with(cx, |app, _| assert!(app.toast.is_none()));

        view.update(cx, |app, cx| {
            app.show_toast(trashed(), cx);
            app.rescan(cx);
            assert!(app.refreshing());
        });
        cx.simulate_keystrokes("ctrl-z");
        toast_is(&view, cx, |notice| matches!(notice, Notice::Trashed { .. }));
        view.update(cx, |app, cx| app.cancel_scan(cx));
    }

    pub(super) fn scanned<'a>(
        cx: &'a mut TestAppContext,
        dir: &Path,
        tab: Tab,
    ) -> (Entity<Kuizer>, &'a mut VisualTestContext) {
        let root = project(dir);
        let (view, cx) = open(cx, dir.join("recent"));
        type_text(cx, &root.display().to_string());
        cx.simulate_keystrokes("enter");
        wait_for_refresh(&view, cx);
        view.update(cx, |app, cx| {
            app.tab = tab;
            cx.notify();
        });
        cx.update(|window, cx| window.draw(cx).clear());
        (view, cx)
    }

    fn right_click(cx: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
        let bounds = cx.debug_bounds(selector).expect("the row is painted");
        cx.simulate_mouse_down(bounds.center(), MouseButton::Right, Modifiers::default());
        cx.simulate_mouse_up(bounds.center(), MouseButton::Right, Modifiers::default());
        cx.update(|window, cx| window.draw(cx).clear());
        bounds
    }

    fn click_item(cx: &mut VisualTestContext, selector: &'static str) {
        let item = cx.debug_bounds(selector).expect("the menu item is painted");
        cx.simulate_click(item.center(), Modifiers::default());
        cx.run_until_parked();
    }

    pub(super) fn menu_target(view: &Entity<Kuizer>, cx: &mut VisualTestContext) -> Option<Target> {
        view.read_with(cx, |app, _| {
            app.menu.as_ref().map(|menu| menu.target.clone())
        })
    }

    fn only_artifact(view: &Entity<Kuizer>, cx: &mut VisualTestContext) -> (u64, PathBuf) {
        view.read_with(cx, |app, _| {
            let candidates = &app.report().unwrap().candidates;
            assert_eq!(candidates.len(), 1);
            (candidates[0].id, candidates[0].path.clone())
        })
    }

    #[gpui::test]
    fn right_clicking_a_row_opens_its_menu_there(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (view, cx) = scanned(cx, dir.path(), Tab::Artifacts);
        let (id, path) = only_artifact(&view, cx);
        let row = right_click(cx, "artifact-row-0");
        assert_eq!(
            menu_target(&view, cx),
            Some(Target::Item(Source::Artifact, id, path))
        );
        let menu = cx.debug_bounds("menu").expect("the menu is painted");
        assert!(menu.top() >= row.top() && menu.top() <= row.bottom());
        assert!((menu.left() - row.center().x).abs() < px(1.));
        view.read_with(cx, |app, _| {
            assert!(app.selection.is_empty(), "a right click does not select");
            let labels: Vec<&str> = app
                .menu_items(&app.menu.as_ref().unwrap().target)
                .iter()
                .map(|item| item.label)
                .collect();
            assert_eq!(
                labels,
                [
                    "Reveal in file manager",
                    "Copy path",
                    "Select",
                    "Move to trash",
                    "Always exclude"
                ]
            );
        });
    }

    #[gpui::test]
    fn copy_path_puts_the_full_path_on_the_clipboard(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (view, cx) = scanned(cx, dir.path(), Tab::Artifacts);
        let (_, path) = only_artifact(&view, cx);
        right_click(cx, "artifact-row-0");
        click_item(cx, "menu-Copy path");
        assert!(menu_target(&view, cx).is_none(), "choosing closes the menu");
        let copied = cx.read_from_clipboard().and_then(|item| item.text());
        assert_eq!(copied.as_deref(), Some(path.to_str().unwrap()));
    }

    #[gpui::test]
    fn always_exclude_writes_the_config_and_hides_the_item(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (view, cx) = scanned(cx, dir.path(), Tab::Artifacts);
        let (id, path) = only_artifact(&view, cx);
        view.update(cx, |app, cx| app.toggle(Source::Artifact, id, cx));
        cx.update(|window, cx| window.draw(cx).clear());
        right_click(cx, "artifact-row-0");
        click_item(cx, "menu-Always exclude");

        let file = dir.path().join("config.toml");
        let config = Config::load(Some(&file)).unwrap().config;
        let pattern = kuizer::config::exclude_pattern(&path);
        assert_eq!(config.scan.exclude, std::slice::from_ref(&pattern));
        view.read_with(cx, |app, _| {
            assert!(app.menu.is_none());
            assert!(app.report().unwrap().candidates.is_empty());
            assert!(app.rows.is_empty());
            assert!(app.selection.is_empty(), "the selection forgets it too");
            assert_eq!(app.config.scan.exclude, std::slice::from_ref(&pattern));
        });
        toast_is(&view, cx, |notice| matches!(notice, Notice::Info(_)));

        view.update(cx, |app, cx| app.rescan(cx));
        wait_for_refresh(&view, cx);
        view.read_with(cx, |app, _| {
            assert!(
                app.report().unwrap().candidates.is_empty(),
                "a fresh scan skips it"
            );
        });
    }

    #[gpui::test]
    fn escape_or_a_click_outside_closes_the_menu(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (view, cx) = scanned(cx, dir.path(), Tab::Artifacts);
        right_click(cx, "artifact-row-0");
        assert!(menu_target(&view, cx).is_some());
        cx.simulate_keystrokes("escape");
        assert!(menu_target(&view, cx).is_none());

        let row = right_click(cx, "artifact-row-0");
        assert!(menu_target(&view, cx).is_some());
        cx.simulate_click(
            point(row.center().x, row.bottom() + px(200.)),
            Modifiers::default(),
        );
        view.read_with(cx, |app, _| {
            assert!(app.menu.is_none());
            assert!(app.selection.is_empty(), "the click only closed the menu");
        });
    }

    #[gpui::test]
    fn the_keyboard_opens_the_menu_and_chooses_an_item(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (view, cx) = scanned(cx, dir.path(), Tab::Artifacts);
        let (id, path) = only_artifact(&view, cx);
        cx.simulate_keystrokes("shift-f10");
        view.read_with(cx, |app, _| {
            let menu = app.menu.as_ref().expect("Shift+F10 opens the menu");
            assert_eq!(
                menu.target,
                Target::Item(Source::Artifact, id, path.clone())
            );
            assert_eq!(menu.active, Some(0));
        });
        cx.update(|window, cx| window.draw(cx).clear());
        let row = cx.debug_bounds("artifact-row-0").unwrap();
        let menu = cx.debug_bounds("menu").unwrap();
        assert!(
            menu.top() >= row.bottom() - px(1.),
            "it opens below the row"
        );

        let active = |cx: &mut VisualTestContext| {
            view.read_with(cx, |app, _| app.menu.as_ref().and_then(|menu| menu.active))
        };
        cx.simulate_keystrokes("down");
        assert_eq!(active(cx), Some(1));
        cx.simulate_keystrokes("j j");
        assert_eq!(active(cx), Some(3));
        cx.simulate_keystrokes("k");
        assert_eq!(active(cx), Some(2));
        cx.simulate_keystrokes("up up up");
        assert_eq!(active(cx), Some(4), "it wraps around");
        cx.simulate_keystrokes("down down down enter");
        view.read_with(cx, |app, _| {
            assert!(app.menu.is_none());
            assert!(app.selection.contains(&(Source::Artifact, id)));
        });

        cx.simulate_keystrokes("menu");
        assert!(
            menu_target(&view, cx).is_some(),
            "the Menu key opens it too"
        );
        cx.simulate_keystrokes("s");
        view.read_with(cx, |app, _| {
            assert!(app.menu.is_none());
            assert!(app.selection.is_empty(), "S deselects");
        });
    }

    #[gpui::test]
    fn a_right_click_on_the_titlebar_opens_no_item_menu(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (view, cx) = scanned(cx, dir.path(), Tab::Artifacts);
        right_click(cx, "titlebar");
        assert!(menu_target(&view, cx).is_none());
    }

    #[gpui::test]
    fn trash_from_the_menu_asks_for_that_item_only(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let root = project(dir.path());
        let other = root.join("web").join("node_modules").join("x");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(root.join("web").join("package.json"), "{}").unwrap();
        std::fs::write(other.join("index.js"), vec![b'y'; 8 * 1024]).unwrap();
        let (view, cx) = open(cx, dir.path().join("recent"));
        type_text(cx, &root.display().to_string());
        cx.simulate_keystrokes("enter");
        wait_for_refresh(&view, cx);
        view.update(cx, |app, cx| {
            app.tab = Tab::Artifacts;
            let first = app.report().unwrap().candidates[0].id;
            app.toggle(Source::Artifact, first, cx);
        });
        cx.update(|window, cx| window.draw(cx).clear());

        view.update(cx, |app, _| app.stale = Some(SystemTime::now()));
        right_click(cx, "artifact-row-1");
        view.read_with(cx, |app, _| {
            let items = app.menu_items(&app.menu.as_ref().unwrap().target);
            let trash = items
                .iter()
                .find(|item| item.choice == Choice::Trash)
                .unwrap();
            assert!(!trash.enabled, "not while the results are refreshing");
            assert_eq!(trash.tip, app.trash_lock());
        });
        cx.simulate_keystrokes("delete");
        view.read_with(cx, |app, _| assert!(app.dialog.is_none()));
        cx.simulate_keystrokes("escape");
        view.update(cx, |app, _| app.stale = None);

        let second = right_click(cx, "artifact-row-1");
        let Some(Target::Item(_, id, _)) = menu_target(&view, cx) else {
            panic!("the menu is open on the second row");
        };
        assert!(second.size.height > px(0.));
        cx.simulate_keystrokes("delete");
        view.read_with(cx, |app, _| {
            assert!(matches!(
                app.dialog,
                Some(Dialog::Confirm {
                    only: Some((Source::Artifact, only)),
                    ..
                }) if only == id
            ));
            let items = app.confirming();
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].1.id, id);
            assert_eq!(app.selected().len(), 1, "the selection is untouched");
        });
        cx.simulate_keystrokes("escape");
        view.read_with(cx, |app, _| assert!(app.dialog.is_none()));
    }

    #[gpui::test]
    fn a_failed_reveal_explains_why(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (view, cx) = scanned(cx, dir.path(), Tab::Artifacts);
        right_click(cx, "artifact-row-0");
        click_item(cx, "menu-Reveal in file manager");
        toast_is(&view, cx, |notice| {
            matches!(notice, Notice::Failed(text)
                if text == "Could not open the file manager: no file manager in tests")
        });
    }

    #[gpui::test]
    fn duplicates_can_be_excluded_from_their_menu(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let root = project(dir.path());
        for name in ["a", "b"] {
            let folder = root.join("photos").join(name);
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join("same.bin"), vec![b'z'; 16 * 1024]).unwrap();
        }
        let (view, cx) = open(cx, dir.path().join("recent"));
        view.update(cx, |app, _| app.options.duplicates = true);
        type_text(cx, &root.display().to_string());
        cx.simulate_keystrokes("enter");
        wait_for_refresh(&view, cx);
        wait_for_duplicates(&view, cx);
        view.update(cx, |app, cx| {
            app.tab = Tab::Duplicates;
            cx.notify();
        });
        cx.update(|window, cx| window.draw(cx).clear());
        let groups = |cx: &mut VisualTestContext| {
            view.read_with(cx, |app, _| {
                app.report()
                    .unwrap()
                    .duplicates
                    .as_ref()
                    .unwrap()
                    .groups
                    .len()
            })
        };
        assert_eq!(groups(cx), 1);
        right_click(cx, "duplicate-0-0");
        let Some(Target::Duplicate(path)) = menu_target(&view, cx) else {
            panic!("a duplicate menu");
        };
        view.read_with(cx, |app, _| {
            let items = app.menu_items(&app.menu.as_ref().unwrap().target);
            let choices: Vec<Choice> = items.iter().map(|item| item.choice).collect();
            assert_eq!(choices, [Choice::Reveal, Choice::CopyPath, Choice::Exclude]);
        });
        cx.simulate_keystrokes("x");
        assert_eq!(groups(cx), 0, "one copy left is no longer a duplicate");
        let written = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(written.contains(&kuizer::config::exclude_pattern(&path)));
    }

    #[gpui::test]
    fn a_broken_saved_scan_falls_back_to_a_normal_scan(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let root = project(dir.path());
        let (view, cx) = open(cx, dir.path().join("recent"));
        type_text(cx, &root.display().to_string());
        cx.simulate_keystrokes("enter");
        wait_for_refresh(&view, cx);
        wait_for_saved(dir.path());
        let scans = dir.path().join("scans");
        let files: Vec<PathBuf> = std::fs::read_dir(&scans)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(files.len(), 1);
        std::fs::write(&files[0], b"\0garbage").unwrap();

        scan_again_from_the_start(&view, cx);
        view.read_with(cx, |app, _| assert!(matches!(app.phase, Phase::Loading(_))));
        cx.update(|window, cx| window.draw(cx).clear());
        assert!(cx.debug_bounds("activity").is_some());
        assert!(!wait_until_shown(&view, cx), "no saved scan is shown");
        wait_for_refresh(&view, cx);
        let (roots, options) = view.read_with(cx, |app, _| {
            let report = app.report().expect("the fresh scan");
            assert_eq!(report.candidates.len(), 1);
            (report.roots.clone(), report.options.clone())
        });
        let saved = (0..1000).any(|_| {
            let saved = Snapshots::new(Some(scans.clone())).load(&roots, &options);
            saved.is_some() || {
                std::thread::sleep(Duration::from_millis(5));
                false
            }
        });
        assert!(saved, "the fresh scan replaces the broken file");
    }
}
