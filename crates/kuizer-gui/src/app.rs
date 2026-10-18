use std::collections::HashSet;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use gpui::{
    AnyElement, App, ClickEvent, Context, CursorStyle, DispatchPhase, Div, FocusHandle, FontWeight,
    KeyBinding, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    PathPromptOptions, Pixels, ScrollWheelEvent, SharedString, Task, Window, actions, canvas, div,
    prelude::*, px, uniform_list,
};
use kuizer::config::Config;
use kuizer::delete::Status;
use kuizer::detect::{Category, Safety};
use kuizer::dupes;
use kuizer::history::Action;
use kuizer::output::display_path;
use kuizer::platform;
use kuizer::scan::{self, Cancel, Candidate};
use kuizer::stats;

use crate::chart::{self, Measure, Segment};
use crate::frame;
use crate::glossary::Term;
use crate::model::{
    self, Batch, Confirmation, HistoryView, Outcome, Report, Restored, Scope, Source, Stage,
    TrashState,
};
use crate::motion;
use crate::snapshot::{self, Snapshots};
use crate::start::{self, Recent, ScanOptions};
use crate::theme::{self, unit};
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

actions!(kuizer, [ZoomIn, ZoomOut, ResetZoom, Undo]);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("secondary-=", ZoomIn, None),
        KeyBinding::new("secondary-+", ZoomIn, None),
        KeyBinding::new("secondary-shift-=", ZoomIn, None),
        KeyBinding::new("secondary--", ZoomOut, None),
        KeyBinding::new("secondary-0", ResetZoom, None),
        KeyBinding::new("secondary-z", Undo, None),
    ]);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Overview,
    Artifacts,
    Caches,
    Duplicates,
    History,
    Issues,
}

impl Tab {
    const ALL: [Tab; 6] = [
        Self::Overview,
        Self::Artifacts,
        Self::Caches,
        Self::Duplicates,
        Self::History,
        Self::Issues,
    ];

    fn term(self) -> Term {
        match self {
            Self::Overview => Term::Overview,
            Self::Artifacts => Term::ProjectArtifacts,
            Self::Caches => Term::GlobalCaches,
            Self::Duplicates => Term::DuplicateFiles,
            Self::History => Term::History,
            Self::Issues => Term::Issues,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Artifacts => "Project artifacts",
            Self::Caches => "Global caches",
            Self::Duplicates => "Duplicates",
            Self::History => "History",
            Self::Issues => "Issues",
        }
    }

    fn status(self, stage: Stage, options: &ScanOptions) -> &'static str {
        let walking = matches!(stage, Stage::Starting | Stage::Scanning(_));
        match self {
            Self::Overview => "Shown once every part of the scan is done",
            Self::Artifacts if walking => "Looking for project artifacts",
            Self::Artifacts => "Artifacts found, shown once the scan is done",
            Self::Issues if walking => "Collecting issues while scanning folders",
            Self::Issues => "Issues collected, shown once the scan is done",
            Self::Duplicates if !options.duplicates => "Duplicate search is off for this scan",
            Self::Duplicates if walking => "Collecting files while scanning folders",
            Self::Duplicates => match stage {
                Stage::Duplicates(dupes::Stage::Walking { .. }) => "Collecting files",
                Stage::Duplicates(dupes::Stage::Hashing { .. }) => "Hashing files",
                Stage::Duplicates(dupes::Stage::Verifying { .. }) => "Comparing files",
                _ => "Duplicates found, shown once the scan is done",
            },
            Self::Caches if !options.caches => "Global caches are off for this scan",
            Self::Caches => match stage {
                Stage::Caches => "Measuring global caches",
                Stage::History => "Caches measured, shown once the scan is done",
                _ => "Measured after the folder scan",
            },
            Self::History if matches!(stage, Stage::History) => "Reading the cleanup history",
            Self::History => "Read at the end of the scan",
        }
    }
}

enum Phase {
    Start,
    Loading(Stage),
    Ready(Box<Report>),
    Failed(String),
}

struct Finished {
    removed: usize,
    removed_bytes: u64,
    failures: Vec<(String, String)>,
    history_error: Option<String>,
}

enum Update {
    Cached(Report),
    Done(Result<Report, String>),
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
}

impl Notice {
    fn lingers(&self) -> bool {
        match self {
            Self::Trashed { .. } => true,
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
    Confirm { rule: Confirmation, typed: String },
    Running,
    Finished(Finished),
}

pub struct Kuizer {
    config: Config,
    roots: Vec<PathBuf>,
    options: ScanOptions,
    folder: String,
    whole_home: bool,
    folder_error: Option<String>,
    recent: Recent,
    snapshots: Snapshots,
    stale: Option<SystemTime>,
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
    selection: HashSet<(Source, u64)>,
    dialog: Option<Dialog>,
    task: Option<Task<()>>,
    toast: Option<Toast>,
    toast_task: Option<Task<()>>,
    restore_task: Option<Task<()>>,
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
        let folder = recent
            .paths
            .first()
            .or(config.scan.roots.first())
            .or(roots.first())
            .map(|path| display_path(path))
            .unwrap_or_default();
        cx.set_global(tooltip::Typeface(font.clone()));
        motion::configure(config.appearance.motion);
        let mut app = Self {
            options: ScanOptions::from_config(&config),
            config,
            roots,
            folder,
            whole_home: false,
            folder_error: None,
            recent,
            snapshots,
            stale: None,
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
            selection: HashSet::new(),
            dialog: None,
            task: None,
            toast: None,
            toast_task: None,
            restore_task: None,
        };
        if !app.roots.is_empty() {
            app.recent.remember(&app.roots);
            app.rescan(cx);
        }
        app
    }

    fn report(&self) -> Option<&Report> {
        match &self.phase {
            Phase::Ready(report) => Some(report),
            _ => None,
        }
    }

    fn busy(&self) -> bool {
        matches!(self.phase, Phase::Loading(_))
            || matches!(self.dialog, Some(Dialog::Running))
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
            loop {
                cx.background_executor().timer(POLL).await;
                let open = this.update(cx, |app, cx| {
                    let paused = app.dialog.is_some();
                    let Some(toast) = &mut app.toast else {
                        return false;
                    };
                    if !toast.notice.lingers() || paused {
                        return true;
                    }
                    toast.shown += POLL;
                    if toast.shown < TOAST_TIME {
                        return true;
                    }
                    app.toast = None;
                    cx.notify();
                    false
                });
                if !matches!(open, Ok(true)) {
                    break;
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
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("kuizer-gui-restore".to_owned())
            .spawn(move || {
                let restored = model::restore(session, scope);
                let history = HistoryView::load();
                let _ = sender.send((restored, history));
            })
            .expect("spawn restore thread");
        self.restore_task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                let message = match receiver.try_recv() {
                    Ok(message) => Some(message),
                    Err(TryRecvError::Empty) => continue,
                    Err(TryRecvError::Disconnected) => None,
                };
                let _ = this.update(cx, |app, cx| app.restored(message, cx));
                break;
            }
        }));
    }

    fn restored(
        &mut self,
        message: Option<(Result<Restored, String>, Result<HistoryView, String>)>,
        cx: &mut Context<Self>,
    ) {
        let Some((restored, history)) = message else {
            self.show_toast(
                Notice::Failed("the restore stopped unexpectedly".to_owned()),
                cx,
            );
            return;
        };
        if let Phase::Ready(report) = &mut self.phase {
            report.history = history;
        }
        let notice = match restored {
            Ok(restored) => {
                if restored.items > 0 && matches!(self.phase, Phase::Ready(_)) {
                    self.rescan(cx);
                }
                Notice::Restored(restored)
            }
            Err(error) => Notice::Failed(error),
        };
        self.show_toast(notice, cx);
    }

    fn refreshing(&self) -> bool {
        self.stale.is_some()
    }

    fn rescan(&mut self, cx: &mut Context<Self>) {
        let shown = if let Phase::Ready(report) = &self.phase {
            self.stale = Some(report.scanned);
            true
        } else {
            self.phase = Phase::Loading(Stage::default());
            self.stale = None;
            self.selection.clear();
            self.rows.clear();
            false
        };
        self.started = Instant::now();
        let cancel = Cancel::new();
        self.cancel = Some(cancel.clone());
        let stage = Arc::new(Mutex::new(Stage::default()));
        let (sender, receiver) = mpsc::channel();
        {
            let config = self.config.clone();
            let roots = self.roots.clone();
            let options = self.options.clone();
            let stage = Arc::clone(&stage);
            let snapshots = self.snapshots.clone();
            std::thread::Builder::new()
                .name("kuizer-gui-scan".to_owned())
                .spawn(move || {
                    if !shown && let Some(report) = snapshots.load(&roots, &options) {
                        let _ = sender.send(Update::Cached(report));
                    }
                    let result = model::load(&config, &roots, &options, &cancel, &stage);
                    if let Ok(report) = &result {
                        snapshots.save(report);
                    }
                    let _ = sender.send(Update::Done(result));
                })
                .expect("spawn scan thread");
        }
        self.task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                let update = match receiver.try_recv() {
                    Ok(update) => Some(update),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => Some(Update::Done(Err(
                        "the scan stopped unexpectedly".to_owned(),
                    ))),
                };
                let current = stage.lock().map(|stage| *stage).unwrap_or_default();
                let done = matches!(update, Some(Update::Done(_)));
                let updated = this.update(cx, |app, cx| {
                    match update {
                        Some(Update::Cached(report)) => {
                            if matches!(app.phase, Phase::Loading(_)) {
                                app.stale = Some(report.scanned);
                                app.adopt(report);
                            }
                        }
                        Some(Update::Done(Ok(report))) => app.adopt(report),
                        Some(Update::Done(Err(error))) if error == model::CANCELLED => {
                            app.phase = Phase::Start;
                        }
                        Some(Update::Done(Err(error))) => app.phase = Phase::Failed(error),
                        None => {
                            if matches!(app.phase, Phase::Loading(_)) {
                                app.phase = Phase::Loading(current);
                            }
                        }
                    }
                    if done {
                        app.cancel = None;
                        app.stale = None;
                    }
                    app.refresh_rows();
                    cx.notify();
                });
                if done || updated.is_err() {
                    break;
                }
            }
        }));
    }

    fn adopt(&mut self, report: Report) {
        let selection: HashSet<(Source, u64)> = {
            let kept: HashSet<(Source, &Path)> = self
                .selected()
                .into_iter()
                .map(|(source, candidate)| (source, candidate.path.as_path()))
                .collect();
            let artifacts = report
                .candidates
                .iter()
                .map(|candidate| (Source::Artifact, candidate));
            let caches = report
                .caches
                .iter()
                .map(|(_, candidate)| (Source::Cache, candidate));
            artifacts
                .chain(caches)
                .filter(|(source, candidate)| kept.contains(&(*source, candidate.path.as_path())))
                .map(|(source, candidate)| (source, candidate.id))
                .collect()
        };
        self.selection = selection;
        self.phase = Phase::Ready(Box::new(report));
        self.refresh_rows();
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
        match start::check_folder(&path) {
            Ok(root) => {
                self.folder_error = None;
                self.roots = scan::normalize_roots(&[root]);
                self.recent.remember(&self.roots);
                self.rescan(cx);
            }
            Err(error) => {
                self.folder_error = Some(error);
                cx.notify();
            }
        }
    }

    fn cancel_scan(&mut self, cx: &mut Context<Self>) {
        if let Some(cancel) = self.cancel.take() {
            cancel.cancel();
        }
        self.task = None;
        self.stale = None;
        self.phase = Phase::Start;
        cx.notify();
    }

    fn new_scan(&mut self, cx: &mut Context<Self>) {
        if self.busy() || self.refreshing() {
            return;
        }
        self.selection.clear();
        self.rows.clear();
        self.toast = None;
        self.phase = Phase::Start;
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
        let command = keystroke.modifiers.control || keystroke.modifiers.platform;
        match keystroke.key.as_str() {
            "enter" => {
                self.start_scan(cx);
                return;
            }
            "backspace" if command || keystroke.modifiers.alt => {
                let trimmed = self.folder.trim_end_matches('/');
                let keep = trimmed.rfind('/').map_or(0, |index| index + 1);
                self.folder.truncate(keep);
            }
            "backspace" => {
                self.folder.pop();
            }
            "v" if command => {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    self.folder
                        .push_str(text.lines().next().unwrap_or_default().trim());
                }
            }
            "u" if command => self.folder.clear(),
            _ => {
                let Some(typed) = keystroke.key_char.as_deref() else {
                    return;
                };
                if command || typed.chars().any(char::is_control) {
                    return;
                }
                self.folder.push_str(typed);
            }
        }
        self.whole_home = false;
        self.folder_error = None;
        cx.notify();
    }

    fn refresh_rows(&mut self) {
        let Phase::Ready(report) = &self.phase else {
            self.rows.clear();
            return;
        };
        self.rows = report
            .candidates
            .iter()
            .enumerate()
            .filter(|(_, candidate)| {
                self.safety
                    .is_none_or(|safety| candidate.rule.safety == safety)
                    && !self.hidden.contains(&candidate.rule.category)
            })
            .map(|(index, _)| index)
            .collect();
    }

    fn selected(&self) -> Vec<(Source, &Candidate)> {
        let Some(report) = self.report() else {
            return Vec::new();
        };
        let artifacts = report
            .candidates
            .iter()
            .filter(|candidate| self.selection.contains(&(Source::Artifact, candidate.id)))
            .map(|candidate| (Source::Artifact, candidate));
        let caches = report
            .caches
            .iter()
            .map(|(_, candidate)| candidate)
            .filter(|candidate| self.selection.contains(&(Source::Cache, candidate.id)))
            .map(|candidate| (Source::Cache, candidate));
        artifacts.chain(caches).collect()
    }

    fn toggle(&mut self, source: Source, id: u64, cx: &mut Context<Self>) {
        if !self.selection.remove(&(source, id)) {
            self.selection.insert((source, id));
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
                .filter(|(_, candidate)| candidate.rule.safety == Safety::Safe)
                .map(|(_, candidate)| (Source::Cache, candidate.id))
                .collect(),
            _ => self
                .rows
                .iter()
                .map(|&index| &report.candidates[index])
                .filter(|candidate| candidate.rule.safety == Safety::Safe)
                .map(|candidate| (Source::Artifact, candidate.id))
                .collect(),
        };
        self.selection.extend(keys);
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
        if keys.iter().all(|key| self.selection.contains(key)) {
            for key in &keys {
                self.selection.remove(key);
            }
        } else {
            self.selection.extend(keys);
        }
        cx.notify();
    }

    fn open_confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.selected();
        if selected.is_empty() || self.busy() || self.refreshing() {
            return;
        }
        let rule = Confirmation::for_items(selected.iter().map(|(_, candidate)| *candidate));
        self.dialog = Some(Dialog::Confirm {
            rule,
            typed: String::new(),
        });
        window.focus(&self.focus);
        cx.notify();
    }

    fn accept(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::Confirm { rule, typed }) = &self.dialog else {
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
        for (source, candidate) in self.selected() {
            match source {
                Source::Artifact => artifacts.push(candidate.clone()),
                Source::Cache => caches.push(candidate.clone()),
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
        self.dialog = Some(Dialog::Running);
        cx.notify();

        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("kuizer-gui-trash".to_owned())
            .spawn(move || {
                let outcome = model::trash(batches);
                let history = HistoryView::load();
                let _ = sender.send((outcome, history));
            })
            .expect("spawn trash thread");
        self.task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                let message = match receiver.try_recv() {
                    Ok(message) => Some(message),
                    Err(TryRecvError::Empty) => continue,
                    Err(TryRecvError::Disconnected) => None,
                };
                let _ = this.update(cx, |app, cx| {
                    app.finish(message, cx);
                });
                break;
            }
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
            report.history = history;
        }
        for (source, removal) in &outcome.removals {
            if removal.status == Status::Removed {
                self.selection.remove(&(*source, removal.candidate.id));
            }
        }
        self.refresh_rows();
        let clean = finished.failures.is_empty() && finished.history_error.is_none();
        if let Some(session) = outcome.session.filter(|_| finished.removed > 0) {
            self.show_toast(
                Notice::Trashed {
                    session,
                    items: finished.removed,
                    bytes: finished.removed_bytes,
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

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        if self.dialog.is_none() {
            match self.phase {
                Phase::Start => self.type_folder(event, cx),
                Phase::Loading(_) if key == "escape" => self.cancel_scan(cx),
                Phase::Ready(_) if key == "escape" && self.refreshing() => self.cancel_scan(cx),
                Phase::Ready(_) if key == "escape" => self.dismiss_toast(cx),
                _ => {}
            }
            return;
        }
        match &mut self.dialog {
            Some(Dialog::Confirm { typed, .. }) => match key {
                "escape" => self.dialog = None,
                "enter" => {
                    self.accept(cx);
                    return;
                }
                "backspace" => {
                    typed.pop();
                }
                _ => {
                    if let Some(ch) = event.keystroke.key_char.as_deref()
                        && ch.chars().all(|c| c.is_ascii_digit())
                        && typed.len() < 9
                    {
                        typed.push_str(ch);
                    }
                }
            },
            Some(Dialog::Finished(_)) if matches!(key, "escape" | "enter") => self.dialog = None,
            _ => return,
        }
        cx.notify();
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
                .gap(unit(6.))
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
        let actions = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(unit(12.))
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
            .flex()
            .flex_none()
            .items_center()
            .gap(unit(12.))
            .h(unit(40.))
            .min_h(frame::TITLEBAR)
            .px(unit(14.))
            .when_some(frame::inset(window), |this, inset| this.pl(inset))
            .when(custom, |this| this.pr(unit(6.)))
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
            .child(div().font_weight(FontWeight::SEMIBOLD).child("kuizer"))
            .child(
                div()
                    .id("roots")
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(theme::muted())
                    .when(!matches!(self.phase, Phase::Start), |this| {
                        this.tip(tooltip::plain(roots.clone()))
                    })
                    .child(roots),
            )
            .children(stale)
            .child(actions)
            .children(controls)
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
            Tab::Duplicates => report.duplicates.as_ref().map(|found| found.groups.len()),
            Tab::History => report
                .history
                .as_ref()
                .ok()
                .map(|view| view.delete_runs().count()),
            Tab::Issues => Some(report.issues.len()),
        }
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_col()
            .flex_none()
            .w(unit(200.))
            .gap(unit(2.))
            .p(unit(8.))
            .bg(theme::panel())
            .border_r_1()
            .border_color(theme::border())
            .children(Tab::ALL.into_iter().map(|tab| {
                let active = self.tab == tab;
                let count = self.tab_count(tab);
                div()
                    .id(tab.label())
                    .debug_selector(|| tab.label().to_owned())
                    .flex()
                    .items_center()
                    .h(unit(28.))
                    .px(unit(10.))
                    .rounded(theme::radius_control())
                    .cursor_pointer()
                    .text_color(if active {
                        theme::text()
                    } else {
                        theme::muted()
                    })
                    .when(active, |this| this.bg(theme::selected()))
                    .hover(|style| style.bg(theme::hover()))
                    .tip(tooltip::term(tab.term()))
                    .child(div().flex_1().child(tab.label()))
                    .children(count.map(|count| {
                        div()
                            .text_size(unit(12.))
                            .text_color(theme::faint())
                            .child(count.to_string())
                    }))
                    .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                        app.tab = tab;
                        cx.notify();
                    }))
            }))
    }

    fn render_statusbar(&self, cx: &mut Context<Self>) -> Div {
        let selected = self.selected();
        let bytes: u64 = selected
            .iter()
            .map(|(_, candidate)| candidate.usage.allocated)
            .sum();
        let review = selected
            .iter()
            .filter(|(_, candidate)| candidate.rule.safety == Safety::Review)
            .count();
        let summary = if selected.is_empty() {
            "Nothing selected".to_owned()
        } else {
            format!("{} selected, {}", selected.len(), theme::bytes(bytes))
        };
        let ready = self.report().is_some() && !self.busy();
        let refreshing = self.refreshing();
        let can_select = ready && matches!(self.tab, Tab::Artifacts | Tab::Caches);
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(unit(8.))
            .h(unit(40.))
            .px(unit(14.))
            .bg(theme::panel())
            .border_t_1()
            .border_color(theme::border())
            .child(
                div()
                    .id("selection")
                    .when(!selected.is_empty(), |this| {
                        this.tip(selection_details(&selected))
                    })
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
                        if matches!(app.tab, Tab::Artifacts | Tab::Caches) && !app.busy() {
                            app.select_safe(cx);
                        }
                    })),
            )
            .child(
                ui::button(
                    "clear",
                    "Clear",
                    ButtonKind::Ghost,
                    ready && !selected.is_empty(),
                )
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
                    "Move to trash",
                    ButtonKind::Primary,
                    ready && !refreshing && !selected.is_empty(),
                )
                .when(refreshing, |this| {
                    this.tip(tooltip::plain(
                        "Available once the fresh scan finishes. These results come from an earlier scan, so every item is checked again first",
                    ))
                })
                .when(!refreshing, |this| this.tip(tooltip::term(Term::Trash)))
                .on_click(cx.listener(|app, _: &ClickEvent, window, cx| {
                    app.open_confirm(window, cx);
                })),
            )
    }

    fn render_content(&mut self, cx: &mut Context<Self>) -> AnyElement {
        match &self.phase {
            Phase::Start => self.render_start(cx),
            Phase::Loading(stage) => self.render_loading(*stage),
            Phase::Failed(error) => centered(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(unit(6.))
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .child("The scan failed"),
                    )
                    .child(div().text_color(theme::danger()).child(error.clone())),
            ),
            Phase::Ready(_) => match self.tab {
                Tab::Overview => self.render_overview(),
                Tab::Artifacts => self.render_artifacts(cx),
                Tab::Caches => self.render_caches(cx),
                Tab::Duplicates => self.render_duplicates(),
                Tab::History => self.render_history(cx),
                Tab::Issues => self.render_issues(),
            },
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
            .h(unit(32.))
            .px(unit(10.))
            .rounded(theme::radius_control())
            .bg(theme::background())
            .border_1()
            .border_color(if self.folder_error.is_some() {
                theme::danger()
            } else {
                theme::border_strong()
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
                this.child(div().w(unit(1.)).h(unit(16.)).bg(theme::text()))
                    .child(
                        div()
                            .pl(unit(2.))
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
                    this.child(div().flex_none().w(unit(1.)).h(unit(16.)).bg(theme::text()))
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
                    .gap(unit(8.))
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
                    .text_size(unit(12.))
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
                    let missing = !path.is_dir();
                    let full = tooltip::path(&path);
                    div()
                        .id(("recent", index))
                        .flex()
                        .items_center()
                        .gap(unit(8.))
                        .h(unit(26.))
                        .px(unit(8.))
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
                        .child(div().text_size(unit(12.)).text_color(theme::faint()).child(
                            if missing {
                                "missing"
                            } else {
                                "double-click to scan"
                            },
                        ))
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
                .fold(div().flex().gap(unit(2.)), |row, category| {
                    let on = options.categories.contains(&category);
                    row.child(
                        div()
                            .id(("category", category as usize))
                            .flex()
                            .items_center()
                            .gap(unit(6.))
                            .px(unit(8.))
                            .h(unit(24.))
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
                .gap(unit(6.))
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
                    "Duplicate files",
                    "Hash files to find identical copies. Reads every file, so it is slower",
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
            .p(unit(24.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(unit(14.))
                    .w_full()
                    .max_w(unit(760.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(unit(4.))
                            .child(
                                div()
                                    .text_size(unit(18.))
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

    fn render_loading(&self, stage: Stage) -> AnyElement {
        let status = self.tab.status(stage, &self.options);
        let header = div()
            .flex()
            .items_end()
            .gap(unit(16.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap(unit(2.))
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
                    .flex_col()
                    .items_end()
                    .gap(unit(2.))
                    .flex_none()
                    .text_size(unit(12.))
                    .text_color(theme::faint())
                    .child(stage.describe())
                    .child(format!(
                        "{}s elapsed, press Escape to cancel",
                        self.started.elapsed().as_secs()
                    )),
            );
        let body = match self.tab {
            Tab::Overview => skeleton_overview(stage).into_any_element(),
            _ => skeleton_table().into_any_element(),
        };
        scroll("loading")
            .debug_selector(|| "skeleton".to_owned())
            .child(header)
            .child(body)
            .into_any_element()
    }

    fn render_overview(&self) -> AnyElement {
        let Some(report) = self.report() else {
            return div().into_any_element();
        };
        let total = report.total(None);
        let safe = report.total(Some(Safety::Safe));
        let review = report.total(Some(Safety::Review));
        let caches = report.cache_total(None);
        let duplicates = report
            .duplicates
            .as_ref()
            .map_or(0, kuizer::dupes::Duplicates::reclaimable);
        let disk = report
            .disks
            .first()
            .and_then(|disk| Some((disk.available?, disk.total?)));

        let categories = report.by_category();
        let projects = report.by_project();
        let largest_project = projects.first().map_or(0, |bucket| bucket.bytes);
        let largest_cache = report
            .caches
            .first()
            .map_or(0, |(_, candidate)| candidate.usage.allocated);

        scroll("overview")
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(unit(10.))
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
            .child(overview_disk(report))
            .child(
                div()
                    .flex()
                    .gap(unit(10.))
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
                                    theme::share(bucket.bytes, total + caches),
                                    theme::bar(),
                                )
                                .id(("space", index))
                                .tip(tooltip::term(Term::category(bucket.key)))
                            }))
                            .child(ui::bar_row(
                                format!("global caches  {}", plural(report.caches.len(), "cache")),
                                theme::bytes(caches),
                                theme::share(caches, total + caches),
                                theme::faint(),
                            ))
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
            .child(overview_trends(report))
            .child(
                div()
                    .text_size(unit(12.))
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

    fn render_artifacts(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(report) = self.report() else {
            return div().into_any_element();
        };
        let shown_bytes: u64 = self
            .rows
            .iter()
            .map(|&index| report.candidates[index].usage.allocated)
            .sum();
        let all_checked = !self.rows.is_empty()
            && self.rows.iter().all(|&index| {
                self.selection
                    .contains(&(Source::Artifact, report.candidates[index].id))
            });
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
                .gap(unit(6.))
                .px(unit(8.))
                .h(unit(24.))
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
        let toolbar = div()
            .flex()
            .flex_none()
            .flex_wrap()
            .items_center()
            .gap(unit(12.))
            .px(unit(14.))
            .py(unit(8.))
            .border_b_1()
            .border_color(theme::border())
            .child(
                ui::segments()
                    .child(segment("safety-all", "All", None))
                    .child(segment("safety-safe", "Safe", Some(Safety::Safe)))
                    .child(segment("safety-review", "Review", Some(Safety::Review))),
            )
            .child(
                div()
                    .flex()
                    .gap(unit(2.))
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
            .child(head(90., "Size", Term::Size).text_right())
            .child(head(64., "Safety", Term::Safety))
            .child(head(110., "Category", Term::Category))
            .child(head(150., "Rule", Term::Rule))
            .child(head(56., "Changed", Term::Changed).text_right())
            .child(div().flex_1().child("Path"));
        let list = uniform_list(
            "artifacts",
            self.rows.len(),
            cx.processor(|app, range: Range<usize>, _window, cx| {
                let Some(report) = app.report() else {
                    return Vec::new();
                };
                range
                    .filter_map(|row| {
                        let candidate = &report.candidates[*app.rows.get(row)?];
                        let id = candidate.id;
                        let checked = app.selection.contains(&(Source::Artifact, id));
                        let review = candidate.rule.safety == Safety::Review;
                        Some(
                            table_row(("artifact", id), checked)
                                .child(ui::checkbox(checked))
                                .child(
                                    column(90.)
                                        .id(("artifact-size", id))
                                        .text_right()
                                        .tip(size_details(candidate, &report.roots))
                                        .child(theme::bytes(candidate.usage.allocated)),
                                )
                                .child(
                                    column(64.)
                                        .child(ui::safety_label(("artifact-safety", id), review)),
                                )
                                .child(
                                    column(110.)
                                        .id(("artifact-category", id))
                                        .text_color(theme::muted())
                                        .tip(tooltip::term(Term::category(candidate.rule.category)))
                                        .child(candidate.rule.category.as_str()),
                                )
                                .child(
                                    column(150.)
                                        .id(("artifact-rule", id))
                                        .truncate()
                                        .tip(tooltip::titled(
                                            candidate.rule.name.clone(),
                                            format!(
                                                "{} Ecosystem: {}.",
                                                Term::Rule.text(),
                                                candidate.rule.ecosystem
                                            ),
                                        ))
                                        .child(candidate.rule.name.clone()),
                                )
                                .child(
                                    column(56.)
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
                                        .tip(tooltip::path(&candidate.path))
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
            .child(div().w(unit(14.)))
            .child(head(90., "Size", Term::Size).text_right())
            .child(head(64., "Safety", Term::Safety))
            .child(head(90., "Tool", Term::Tool))
            .child(column(170.).child("Cache"))
            .child(head(56., "Changed", Term::Changed).text_right())
            .child(div().flex_1().child("Path"));
        let rows = report.caches.iter().map(|(cache, candidate)| {
            let id = candidate.id;
            let checked = self.selection.contains(&(Source::Cache, id));
            let review = candidate.rule.safety == Safety::Review;
            table_row(("cache", id), checked)
                .child(ui::checkbox(checked))
                .child(
                    column(90.)
                        .id(("cache-size", id))
                        .text_right()
                        .tip(size_details(candidate, &[]))
                        .child(theme::bytes(candidate.usage.allocated)),
                )
                .child(column(64.).child(ui::safety_label(("cache-safety", id), review)))
                .child(column(90.).text_color(theme::muted()).child(cache.tool))
                .child(column(170.).truncate().child(cache.name))
                .child(
                    column(56.)
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
                                    .text_size(unit(12.))
                                    .text_color(theme::faint())
                                    .child(cache.note),
                            )
                        }),
                )
                .h(unit(44.))
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
                div()
                    .flex()
                    .flex_none()
                    .gap(unit(16.))
                    .px(unit(14.))
                    .py(unit(10.))
                    .border_b_1()
                    .border_color(theme::border())
                    .text_color(theme::muted())
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
                    .flex()
                    .flex_col()
                    .flex_1()
                    .overflow_y_scroll()
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

    fn render_duplicates(&self) -> AnyElement {
        let Some(report) = self.report() else {
            return div().into_any_element();
        };
        let Some(found) = &report.duplicates else {
            return centered(ui::empty(
                "The duplicate search was turned off for this scan",
            ));
        };
        let wasted = found.reclaimable();
        let copies: usize = found.groups.iter().map(|group| group.members.len()).sum();
        let largest = found
            .groups
            .first()
            .map_or(0, kuizer::dupes::Group::reclaimable);
        scroll("duplicates")
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(unit(10.))
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
                .children(found.groups.iter().take(DUPLICATE_GROUPS).enumerate().map(
                    |(index, group)| {
                        let members = SharedString::from(format!("dupe-{index}"));
                        div()
                            .flex()
                            .flex_col()
                            .gap(unit(4.))
                            .pt(unit(8.))
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
                                    format!("{} reclaimable", theme::bytes(group.reclaimable())),
                                    theme::share(group.reclaimable(), largest),
                                    theme::bar(),
                                )
                                .id(("group", index))
                                .tip(group_details(group)),
                            )
                            .children(group.members.iter().enumerate().map(|(position, member)| {
                                div()
                                    .flex()
                                    .gap(unit(8.))
                                    .text_color(theme::muted())
                                    .child(
                                        div()
                                            .id((members.clone(), position))
                                            .flex_1()
                                            .min_w_0()
                                            .truncate()
                                            .tip(tooltip::path(&member.path))
                                            .child(model::short_path(&member.path, &report.roots)),
                                    )
                                    .child(
                                        div()
                                            .flex_none()
                                            .text_color(theme::faint())
                                            .child(theme::age(member.modified)),
                                    )
                            }))
                    },
                ))
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
        let recent: Vec<&kuizer::history::Run> = runs
            .iter()
            .take(HISTORY_BARS)
            .rev()
            .map(|(_, run)| *run)
            .collect();
        let can_restore = self.can_restore();
        let refreshing = self.refreshing();
        let tallest = recent.iter().map(|run| run.total.bytes).max().unwrap_or(0);
        let chart = div()
            .flex()
            .items_end()
            .gap(unit(4.))
            .h(unit(120.))
            .children(recent.iter().map(|run| {
                div()
                    .id(("cleanup-bar", run.id))
                    .flex_1()
                    .max_w(unit(40.))
                    .h(gpui::relative(
                        theme::share(run.total.bytes, tallest).max(0.02),
                    ))
                    .rounded_t(unit(2.))
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
            .child(head(56., "Run", Term::Run))
            .child(column(150.).child("Started"))
            .child(column(70.).text_right().child("Items"))
            .child(head(90., "Size", Term::Size).text_right())
            .child(head(80., "Mode", Term::Mode))
            .child(head(110., "Trash", Term::Restore))
            .child(
                div()
                    .id("Command")
                    .flex_1()
                    .tip(tooltip::term(Term::Command))
                    .child("Command"),
            );
        scroll("history")
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(unit(10.))
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
                            theme::bytes(view.freed()),
                            "removed or emptied from the trash",
                        )
                        .id("stat-freed")
                        .tip(tooltip::term(Term::Freed)),
                    )
                    .child(
                        ui::stat(
                            "Still in the trash",
                            if view.trash_known {
                                theme::bytes(view.in_trash())
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
                    .children(runs.iter().map(|&(index, run)| {
                        let mode = run.deleted.first().map_or("", |entry| match entry.mode {
                            kuizer::delete::Mode::Trash => "trash",
                            kuizer::delete::Mode::Permanent => "permanent",
                            kuizer::delete::Mode::DryRun => "dry run",
                        });
                        div()
                            .flex()
                            .items_center()
                            .gap(unit(12.))
                            .h(unit(30.))
                            .px(unit(14.))
                            .border_t_1()
                            .border_color(theme::border())
                            .child(column(56.).child(run.session()))
                            .child(
                                column(150.)
                                    .id(("started", index))
                                    .text_color(theme::muted())
                                    .tip(tooltip::plain(run.started_at.clone()))
                                    .child(short_time(&run.started_at)),
                            )
                            .child(column(70.).text_right().child(run.total.items.to_string()))
                            .child(
                                column(90.)
                                    .text_right()
                                    .child(theme::bytes(run.total.bytes)),
                            )
                            .child(column(80.).text_color(theme::muted()).child(mode))
                            .child(column(110.).flex().child(self.restore_cell(
                                view.trash_state(index),
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
                            .text_size(unit(12.))
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

    fn render_issues(&self) -> AnyElement {
        let Some(report) = self.report() else {
            return div().into_any_element();
        };
        if report.issues.is_empty() {
            return centered(ui::empty("The scan reported no issues"));
        }
        scroll("issues")
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
                    .map(|(index, issue)| {
                        div()
                            .flex()
                            .flex_col()
                            .gap(unit(2.))
                            .py(unit(6.))
                            .border_b_1()
                            .border_color(theme::border())
                            .child(
                                div()
                                    .id(("issue", index))
                                    .truncate()
                                    .tip(tooltip::path(&issue.path))
                                    .child(display_path(&issue.path)),
                            )
                            .child(
                                div()
                                    .text_size(unit(12.))
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
        let line = div().flex().items_center().gap(unit(10.));
        let panel = div()
            .id("toast")
            .debug_selector(|| "toast".to_owned())
            .flex()
            .flex_col()
            .gap(unit(6.))
            .max_w(unit(560.))
            .py(unit(8.))
            .px(unit(12.))
            .rounded(theme::radius_panel())
            .bg(theme::elevated())
            .border_1()
            .border_color(theme::border_strong())
            .shadow_lg()
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
                                        .text_size(unit(12.))
                                        .text_color(theme::faint())
                                        .tip(tooltip::path(path))
                                        .child(format!("{} {reason}", self.shown_path(path)))
                                }),
                        )
                        .when(failed > TOAST_PROBLEMS, |this| {
                            this.child(div().text_size(unit(12.)).text_color(theme::faint()).child(
                                format!(
                                    "and {} more, kuizer history show lists them",
                                    failed - TOAST_PROBLEMS
                                ),
                            ))
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
                        .child(format!("Could not restore: {error}")),
                )
                .child(dismiss()),
            ),
        };
        Some(
            div()
                .absolute()
                .bottom(unit(12.))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .px(unit(16.))
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
            .gap(unit(12.))
            .w(unit(520.))
            .p(unit(18.))
            .rounded(theme::radius_dialog())
            .bg(theme::elevated())
            .border_1()
            .border_color(theme::border_strong())
            .shadow_lg();
        let body = match dialog {
            Dialog::Confirm { rule, typed } => {
                let selected = self.selected();
                let bytes: u64 = selected
                    .iter()
                    .map(|(_, candidate)| candidate.usage.allocated)
                    .sum();
                let review = selected
                    .iter()
                    .filter(|(_, candidate)| candidate.rule.safety == Safety::Review)
                    .count();
                let accepted = rule.accepts(typed);
                panel
                    .child(
                        div()
                            .text_size(unit(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(format!(
                                "Move {} items to the trash?",
                                selected.len()
                            )),
                    )
                    .child(div().text_color(theme::muted()).child(format!(
                        "{} will be moved to the trash. Everything stays restorable with kuizer restore until the trash is emptied.",
                        theme::bytes(bytes)
                    )))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(unit(4.))
                            .p(unit(10.))
                            .rounded(theme::radius_panel())
                            .bg(theme::panel())
                            .children(selected.iter().take(LIST_LIMIT).map(|(_, candidate)| {
                                div()
                                    .flex()
                                    .gap(unit(8.))
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
                                            .w(unit(80.))
                                            .text_right()
                                            .text_color(theme::muted())
                                            .child(theme::bytes(candidate.usage.allocated)),
                                    )
                            }))
                            .when(selected.len() > LIST_LIMIT, |this| {
                                this.child(div().text_color(theme::faint()).child(format!(
                                    "and {} more",
                                    selected.len() - LIST_LIMIT
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
                                "{review} of these are marked review. Type {count} to move all {count} items."
                            )))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .h(unit(30.))
                                    .px(unit(10.))
                                    .rounded(theme::radius_control())
                                    .bg(theme::background())
                                    .border_1()
                                    .border_color(if accepted {
                                        theme::primary()
                                    } else {
                                        theme::border_strong()
                                    })
                                    .child(typed.clone())
                                    .child(div().w(unit(1.)).h(unit(16.)).bg(theme::text())),
                            )
                        },
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap(unit(8.))
                            .child(
                                ui::button("cancel", "Cancel", ButtonKind::Secondary, true).tip(Tooltip::new("Keep everything where it is").keys(&["Esc"]).build())
                                    .on_click(cx.listener(|app, _: &ClickEvent, _, cx| {
                                        app.dialog = None;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                ui::button("confirm", "Move to trash", ButtonKind::Primary, accepted).tip(Tooltip::new("Move the selected items to the trash").keys(&["Enter"]).build())
                                    .on_click(cx.listener(|app, _: &ClickEvent, _, cx| {
                                        app.accept(cx);
                                    })),
                            ),
                    )
            }
            Dialog::Running => panel
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Moving items to the trash"),
                )
                .child(
                    div()
                        .text_color(theme::muted())
                        .child("Each path is checked again right before it moves."),
                ),
            Dialog::Finished(finished) => panel
                .child(
                    div()
                        .text_size(unit(15.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format!(
                            "Moved {} items, {}",
                            finished.removed,
                            theme::bytes(finished.removed_bytes)
                        )),
                )
                .child(
                    div()
                        .text_color(theme::muted())
                        .child("kuizer history lists this run and kuizer restore brings it back."),
                )
                .when(!finished.failures.is_empty(), |this| {
                    this.child(div().text_color(theme::danger()).child(format!(
                        "{} items could not be moved",
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
                                            .text_size(unit(12.))
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
        let content = self.render_content(cx);
        let dialog = self.render_dialog(cx);
        let toast = self.render_toast(cx);
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
            .text_size(unit(13.))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_action(cx.listener(Self::zoom_in))
            .on_action(cx.listener(Self::zoom_out))
            .on_action(cx.listener(Self::reset_zoom))
            .on_action(cx.listener(Self::undo))
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
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .right_0()
                                .child(chart::loader()),
                        )
                    })
                    .children(toast),
            )
            .when(!start, |this| this.child(self.render_statusbar(cx)))
            .children(dialog)
            .child(tooltip::layer(cx));
        frame::frame(root, window)
    }
}

const WIDTHS: [f32; 6] = [0.62, 0.44, 0.71, 0.38, 0.56, 0.49];
const HEIGHTS: [f32; 6] = [0.35, 0.8, 0.55, 1.0, 0.25, 0.6];

fn overview_disk(report: &Report) -> Div {
    let safe = report.total(Some(Safety::Safe));
    let review = report.total(Some(Safety::Review));
    let caches = report.cache_total(None);
    let duplicates = report
        .duplicates
        .as_ref()
        .map_or(0, kuizer::dupes::Duplicates::reclaimable);
    let reclaimable = safe + review + caches + duplicates;
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

fn overview_trends(report: &Report) -> Div {
    let ages = stats::by_age(&report.candidates, SystemTime::now());
    let sizes = stats::by_size(&report.candidates);
    let ecosystems = stats::by_ecosystem(&report.candidates);
    let largest = ecosystems.first().map_or(0, |bucket| bucket.bytes);
    let total = report.total(None);
    let empty = report.candidates.is_empty();
    let removed = report
        .history
        .as_ref()
        .ok()
        .map(|view| stats::removed_by_month(&view.history, &kuizer::history::now(), 12));
    div()
        .flex()
        .flex_col()
        .gap(unit(10.))
        .child(
            div()
                .flex()
                .gap(unit(10.))
                .items_start()
                .child(
                    ui::card("Idle since")
                        .flex_1()
                        .debug_selector(|| "chart-ages".to_owned())
                        .when(!empty, |this| {
                            this.child(chart::columns("age", &ages, Measure::Bytes, "Items"))
                        })
                        .when(empty, |this| {
                            this.child(ui::empty("No project artifacts found"))
                        }),
                )
                .child(
                    ui::card("Artifact sizes")
                        .flex_1()
                        .when(!empty, |this| {
                            this.child(chart::columns("size", &sizes, Measure::Count, "Items"))
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
            .child(chart::columns("removed", &months, Measure::Bytes, "Items"))
        }))
}

fn skeleton_stat(label: &'static str, index: usize, live: Option<String>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(unit(6.))
        .flex_1()
        .min_w(unit(140.))
        .p(unit(14.))
        .rounded(theme::radius_panel())
        .bg(theme::panel())
        .border_1()
        .border_color(theme::border())
        .child(
            div()
                .text_size(unit(12.))
                .text_color(theme::muted())
                .child(label),
        )
        .child(match live {
            Some(value) => div()
                .h(unit(24.))
                .text_size(unit(20.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(value)
                .into_any_element(),
            None => chart::block("stat-value", index, unit(96.), unit(24.)),
        })
        .child(chart::block("stat-sub", index, unit(72.), unit(12.)))
}

fn skeleton_bars(id: &'static str, title: &'static str) -> Div {
    ui::card(title).flex_1().children((0..5).map(move |index| {
        div()
            .flex()
            .flex_col()
            .gap(unit(6.))
            .child(chart::block(
                id,
                index,
                gpui::relative(WIDTHS[index]),
                unit(12.),
            ))
            .child(chart::block(id, index + 10, gpui::relative(1.), unit(4.)))
    }))
}

fn skeleton_columns(id: &'static str, title: &'static str) -> Div {
    ui::card(title).flex_1().child(
        div()
            .flex()
            .items_end()
            .gap(unit(4.))
            .h(unit(88.))
            .children(HEIGHTS.iter().enumerate().map(move |(index, height)| {
                div()
                    .flex_1()
                    .h(gpui::relative(*height))
                    .child(chart::block(
                        id,
                        index,
                        gpui::relative(1.),
                        gpui::relative(1.),
                    ))
            })),
    )
}

fn skeleton_overview(stage: Stage) -> Div {
    let found = match stage {
        Stage::Scanning(progress) => Some(plural(progress.candidates as usize, "item")),
        _ => None,
    };
    div()
        .flex()
        .flex_col()
        .gap(unit(14.))
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap(unit(10.))
                .child(skeleton_stat("Project artifacts", 0, found))
                .child(skeleton_stat("Safe to remove", 1, None))
                .child(skeleton_stat("Needs review", 2, None))
                .child(skeleton_stat("Global caches", 3, None))
                .child(skeleton_stat("Duplicate files", 4, None))
                .child(skeleton_stat("Disk free", 5, None)),
        )
        .child(
            ui::card("Disk")
                .child(chart::block("disk", 0, gpui::relative(1.), unit(12.)))
                .child(div().flex().gap(unit(16.)).children(
                    (1..6).map(|index| chart::block("disk", index, unit(84.), unit(12.))),
                )),
        )
        .child(
            div()
                .flex()
                .gap(unit(10.))
                .items_start()
                .child(skeleton_bars("space", "Where the space is"))
                .child(skeleton_bars("projects", "Largest projects"))
                .child(skeleton_bars("caches", "Largest caches")),
        )
        .child(
            div()
                .flex()
                .gap(unit(10.))
                .items_start()
                .child(skeleton_columns("ages", "Idle since"))
                .child(skeleton_columns("sizes", "Artifact sizes"))
                .child(skeleton_bars("ecosystems", "By ecosystem")),
        )
}

fn skeleton_table() -> Div {
    div()
        .flex()
        .flex_col()
        .rounded(theme::radius_panel())
        .border_1()
        .border_color(theme::border())
        .child(table_header().child(chart::block("head", 0, unit(120.), unit(10.))))
        .children((0..12).map(|index| {
            div()
                .flex()
                .items_center()
                .gap(unit(12.))
                .h(unit(30.))
                .px(unit(14.))
                .border_b_1()
                .border_color(theme::border())
                .child(chart::block("row-check", index, unit(14.), unit(14.)))
                .child(div().flex_1().min_w_0().child(chart::block(
                    "row-path",
                    index,
                    gpui::relative(WIDTHS[index % WIDTHS.len()]),
                    unit(10.),
                )))
                .child(chart::block("row-size", index, unit(64.), unit(10.)))
        }))
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
        .gap(unit(14.))
        .size_full()
        .p(unit(16.))
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
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(unit(12.))
        .h(unit(30.))
        .px(unit(14.))
        .bg(theme::panel())
        .border_b_1()
        .border_color(theme::border())
        .text_size(unit(12.))
        .text_color(theme::muted())
}

fn table_row(id: (&'static str, u64), checked: bool) -> gpui::Stateful<Div> {
    div()
        .id(gpui::ElementId::NamedInteger(id.0.into(), id.1))
        .flex()
        .items_center()
        .gap(unit(12.))
        .w_full()
        .h(unit(30.))
        .px(unit(14.))
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
    details.build()
}

fn selection_details(
    selected: &[(Source, &Candidate)],
) -> impl Fn(&mut Window, &mut App) -> gpui::AnyView + use<> {
    let total = |keep: &dyn Fn(&(Source, &Candidate)) -> bool| {
        selected
            .iter()
            .filter(|item| keep(item))
            .fold((0, 0), |(count, bytes), (_, candidate)| {
                (count + 1, bytes + candidate.usage.allocated)
            })
    };
    let line = |(count, bytes): (usize, u64)| format!("{count}, {}", theme::bytes(bytes));
    Tooltip::details("Selection")
        .row("safe", line(total(&|(_, c)| c.rule.safety == Safety::Safe)))
        .row(
            "review",
            line(total(&|(_, c)| c.rule.safety == Safety::Review)),
        )
        .row(
            "Project artifacts",
            line(total(&|(s, _)| *s == Source::Artifact)),
        )
        .row("Global caches", line(total(&|(s, _)| *s == Source::Cache)))
        .build()
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

fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
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

    fn open(cx: &mut TestAppContext, recent: PathBuf) -> (Entity<Kuizer>, &mut VisualTestContext) {
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
            app
        })
    }

    fn type_text(cx: &mut VisualTestContext, text: &str) {
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
            if view.read_with(cx, |app, _| !matches!(app.phase, Phase::Loading(_))) {
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
                (!matches!(app.phase, Phase::Loading(_))).then(|| app.refreshing())
            }) {
                return refreshing;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("nothing was shown");
    }

    fn wait_for_refresh(view: &Entity<Kuizer>, cx: &mut VisualTestContext) {
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

    fn scan_again_from_the_start(view: &Entity<Kuizer>, cx: &mut VisualTestContext) {
        view.update(cx, |app, cx| app.new_scan(cx));
        cx.simulate_keystrokes("enter");
    }

    fn project(dir: &Path) -> PathBuf {
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
    fn a_scan_shows_skeletons_then_charts_with_tooltips(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let root = project(dir.path());
        let (view, cx) = open(cx, dir.path().join("recent"));
        type_text(cx, &root.display().to_string());
        cx.simulate_keystrokes("enter");
        cx.update(|window, cx| window.draw(cx).clear());
        assert!(cx.debug_bounds("skeleton").is_some());
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
    }

    #[gpui::test]
    fn the_sidebar_switches_sections_during_a_scan(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let root = project(dir.path());
        let (view, cx) = open(cx, dir.path().join("recent"));
        type_text(cx, &root.display().to_string());
        cx.simulate_keystrokes("enter");
        for (label, shown) in [
            ("Global caches", "Global caches are off for this scan"),
            ("History", "Read at the end of the scan"),
            ("Duplicates", "Collecting files while scanning folders"),
        ] {
            cx.update(|window, cx| window.draw(cx).clear());
            let bounds = cx.debug_bounds(label).expect("a sidebar item");
            cx.simulate_click(bounds.center(), Modifiers::default());
            view.read_with(cx, |app, _| {
                assert!(matches!(app.phase, Phase::Loading(_)));
                assert_eq!(app.tab.label(), label);
            });
            cx.update(|window, cx| window.draw(cx).clear());
            assert!(cx.debug_bounds(shown).is_some());
        }
        wait_for_scan(&view, cx);
        view.read_with(cx, |app, _| {
            assert!(matches!(app.phase, Phase::Ready(_)));
            assert_eq!(app.tab, Tab::Duplicates);
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
        cx.simulate_mouse_move(issues.center(), None, Modifiers::default());
        cx.update(|window, cx| window.draw(cx).clear());
        let next = cx
            .debug_bounds("tooltip")
            .expect("the Issues tooltip opens at once");
        assert!(next.origin.y >= issues.bottom());
    }

    #[test]
    fn each_section_describes_its_own_progress() {
        let mut options = ScanOptions::from_config(&Config::default());
        options.caches = true;
        options.duplicates = true;
        let walking = Stage::Starting;
        assert_eq!(
            Tab::Caches.status(walking, &options),
            "Measured after the folder scan"
        );
        assert_eq!(
            Tab::Caches.status(Stage::Caches, &options),
            "Measuring global caches"
        );
        assert_eq!(
            Tab::Artifacts.status(Stage::Caches, &options),
            "Artifacts found, shown once the scan is done"
        );
        options.duplicates = false;
        assert_eq!(
            Tab::Duplicates.status(walking, &options),
            "Duplicate search is off for this scan"
        );
        options.duplicates = true;
        let hashing = Stage::Duplicates(dupes::Stage::Hashing { files: 3 });
        assert_eq!(Tab::Duplicates.status(hashing, &options), "Hashing files");
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

        scan_again_from_the_start(&view, cx);
        assert!(wait_until_shown(&view, cx), "the saved scan comes first");
        let id = view.read_with(cx, |app, _| {
            let report = app.report().expect("the saved scan");
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
            app.selection = report
                .candidates
                .iter()
                .map(|candidate| (Source::Artifact, candidate.id))
                .collect();
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
            matches!(notice, Notice::Restoring { session: 1 })
        });
        cx.update(|window, cx| window.draw(cx).clear());
        assert!(cx.debug_bounds("loader").is_some());

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

    #[gpui::test]
    fn a_broken_saved_scan_falls_back_to_a_normal_scan(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let root = project(dir.path());
        let (view, cx) = open(cx, dir.path().join("recent"));
        type_text(cx, &root.display().to_string());
        cx.simulate_keystrokes("enter");
        wait_for_refresh(&view, cx);
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
        assert!(cx.debug_bounds("skeleton").is_some());
        assert!(!wait_until_shown(&view, cx), "no saved scan is shown");
        let (roots, options) = view.read_with(cx, |app, _| {
            let report = app.report().expect("the fresh scan");
            assert_eq!(report.candidates.len(), 1);
            (report.roots.clone(), report.options.clone())
        });
        assert!(Snapshots::new(Some(scans)).load(&roots, &options).is_some());
    }
}
