use std::collections::HashSet;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, App, ClickEvent, Context, CursorStyle, DispatchPhase, Div, FocusHandle, FontWeight,
    KeyBinding, KeyDownEvent, PathPromptOptions, Pixels, ScrollWheelEvent, SharedString, Task,
    Window, actions, canvas, div, prelude::*, px, uniform_list,
};
use kuizer::config::Config;
use kuizer::delete::Status;
use kuizer::detect::{Category, Safety};
use kuizer::history::Action;
use kuizer::output::display_path;
use kuizer::platform;
use kuizer::scan::{self, Cancel, Candidate};

use crate::model::{self, Batch, Confirmation, HistoryView, Outcome, Report, Source, Stage};
use crate::start::{self, Recent, ScanOptions};
use crate::theme::{self, unit};
use crate::ui::{self, ButtonKind};
use crate::zoom::Zoom;

const POLL: Duration = Duration::from_millis(100);
const LIST_LIMIT: usize = 6;
const DUPLICATE_GROUPS: usize = 200;
const ISSUE_LIMIT: usize = 500;
const HISTORY_BARS: usize = 24;
const WHEEL_NOTCH: Pixels = px(20.);

actions!(kuizer, [ZoomIn, ZoomOut, ResetZoom]);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("secondary-=", ZoomIn, None),
        KeyBinding::new("secondary-+", ZoomIn, None),
        KeyBinding::new("secondary-shift-=", ZoomIn, None),
        KeyBinding::new("secondary--", ZoomOut, None),
        KeyBinding::new("secondary-0", ResetZoom, None),
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
    zoom: Zoom,
    wheel: Pixels,
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
}

impl Kuizer {
    pub fn new(
        config: Config,
        roots: Vec<PathBuf>,
        recent: Recent,
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
        let mut app = Self {
            options: ScanOptions::from_config(&config),
            config,
            roots,
            folder,
            whole_home: false,
            folder_error: None,
            recent,
            zoom,
            wheel: px(0.),
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
        matches!(self.phase, Phase::Loading(_)) || matches!(self.dialog, Some(Dialog::Running))
    }

    fn rescan(&mut self, cx: &mut Context<Self>) {
        self.phase = Phase::Loading(Stage::default());
        self.selection.clear();
        self.rows.clear();
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
            std::thread::Builder::new()
                .name("kuizer-gui-scan".to_owned())
                .spawn(move || {
                    let _ = sender.send(model::load(&config, &roots, &options, &cancel, &stage));
                })
                .expect("spawn scan thread");
        }
        self.task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                let result = match receiver.try_recv() {
                    Ok(result) => Some(result),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => {
                        Some(Err("the scan stopped unexpectedly".to_owned()))
                    }
                };
                let current = stage.lock().map(|stage| *stage).unwrap_or_default();
                let done = result.is_some();
                let updated = this.update(cx, |app, cx| {
                    app.phase = match result {
                        Some(Ok(report)) => Phase::Ready(Box::new(report)),
                        Some(Err(error)) if error == model::CANCELLED => Phase::Start,
                        Some(Err(error)) => Phase::Failed(error),
                        None => Phase::Loading(current),
                    };
                    if done {
                        app.cancel = None;
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
        self.phase = Phase::Start;
        cx.notify();
    }

    fn new_scan(&mut self, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        self.selection.clear();
        self.rows.clear();
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
        if selected.is_empty() || self.busy() {
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
        if !rule.accepts(typed) {
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
        self.dialog = Some(Dialog::Finished(finished));
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
        match self.report() {
            Some(report) => model::short_path(&candidate.path, &report.roots),
            None => display_path(&candidate.path),
        }
    }

    fn render_titlebar(&self, cx: &mut Context<Self>) -> Div {
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
        let scanning = matches!(self.phase, Phase::Loading(_));
        let finished = matches!(self.phase, Phase::Ready(_) | Phase::Failed(_));
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(unit(12.))
            .h(unit(40.))
            .px(unit(14.))
            .bg(theme::panel())
            .border_b_1()
            .border_color(theme::border())
            .child(div().font_weight(FontWeight::SEMIBOLD).child("kuizer"))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(theme::muted())
                    .child(roots),
            )
            .when(scanning, |this| {
                this.child(
                    ui::button("cancel-scan", "Cancel scan", ButtonKind::Secondary, true).on_click(
                        cx.listener(|app, _: &ClickEvent, _, cx| {
                            app.cancel_scan(cx);
                        }),
                    ),
                )
            })
            .when(finished, |this| {
                this.child(
                    ui::button("new-scan", "New scan", ButtonKind::Ghost, !busy).on_click(
                        cx.listener(|app, _: &ClickEvent, _, cx| {
                            app.new_scan(cx);
                        }),
                    ),
                )
                .child(
                    ui::button("rescan", "Rescan", ButtonKind::Secondary, !busy).on_click(
                        cx.listener(|app, _: &ClickEvent, _, cx| {
                            if !app.busy() {
                                app.rescan(cx);
                            }
                        }),
                    ),
                )
            })
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
            .child(div().child(summary))
            .when(review > 0, |this| {
                this.child(
                    div()
                        .text_color(theme::review())
                        .child(format!("{review} marked review")),
                )
            })
            .child(div().flex_1())
            .child(
                ui::button("select-safe", "Select safe", ButtonKind::Ghost, can_select).on_click(
                    cx.listener(|app, _: &ClickEvent, _, cx| {
                        if matches!(app.tab, Tab::Artifacts | Tab::Caches) && !app.busy() {
                            app.select_safe(cx);
                        }
                    }),
                ),
            )
            .child(
                ui::button(
                    "clear",
                    "Clear",
                    ButtonKind::Ghost,
                    ready && !selected.is_empty(),
                )
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
                    ready && !selected.is_empty(),
                )
                .on_click(cx.listener(|app, _: &ClickEvent, window, cx| {
                    app.open_confirm(window, cx);
                })),
            )
    }

    fn render_content(&mut self, cx: &mut Context<Self>) -> AnyElement {
        match &self.phase {
            Phase::Start => self.render_start(cx),
            Phase::Loading(stage) => {
                centered(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(unit(6.))
                        .child(div().font_weight(FontWeight::MEDIUM).child("Scanning"))
                        .child(div().text_color(theme::muted()).child(stage.describe()))
                        .child(div().text_size(unit(12.)).text_color(theme::faint()).child(
                            format!(
                                "{}s elapsed, press Escape to cancel",
                                self.started.elapsed().as_secs()
                            ),
                        )),
                )
            }
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
                Tab::History => self.render_history(),
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
                ui::segment("scope-folder", "One folder", !self.whole_home).on_click(cx.listener(
                    |app, _: &ClickEvent, _, cx| {
                        app.whole_home = false;
                        cx.notify();
                    },
                )),
            )
            .child(
                ui::segment("scope-home", "Whole home folder", self.whole_home).on_click(
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
                        ui::button("browse", "Choose folder", ButtonKind::Secondary, true)
                            .on_click(cx.listener(|app, _: &ClickEvent, _, cx| {
                                app.choose_folder(cx);
                            })),
                    )
                    .child(
                        ui::button("start-scan", "Start scan", ButtonKind::Primary, true)
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
                                .flex_1()
                                .min_w_0()
                                .truncate()
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
                    .child(ui::stat(
                        "Project artifacts",
                        theme::bytes(total),
                        format!("{} items", report.candidates.len()),
                    ))
                    .child(ui::stat(
                        "Safe to remove",
                        theme::bytes(safe),
                        "rebuilt on the next build",
                    ))
                    .child(ui::stat(
                        "Needs review",
                        theme::bytes(review),
                        "check before removing",
                    ))
                    .child(ui::stat(
                        "Global caches",
                        theme::bytes(caches),
                        format!("{} caches", report.caches.len()),
                    ))
                    .child(ui::stat(
                        "Duplicate files",
                        theme::bytes(duplicates),
                        "keep one copy of each",
                    ))
                    .child(ui::stat(
                        "Disk free",
                        disk.map_or_else(String::new, |(free, _)| theme::bytes(free)),
                        disk.map_or_else(String::new, |(_, size)| {
                            format!("of {}", theme::bytes(size))
                        }),
                    )),
            )
            .child(
                div()
                    .flex()
                    .gap(unit(10.))
                    .items_start()
                    .child(
                        ui::card("Where the space is")
                            .flex_1()
                            .children(categories.iter().map(|bucket| {
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
                            .children(projects.iter().take(8).map(|bucket| {
                                ui::bar_row(
                                    model::short_path(&bucket.key, &report.roots),
                                    theme::bytes(bucket.bytes),
                                    theme::share(bucket.bytes, largest_project),
                                    theme::bar(),
                                )
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
            ui::segment(id, label, self.safety == value).on_click(cx.listener(
                move |app, _: &ClickEvent, _, cx| {
                    app.safety = value;
                    app.refresh_rows();
                    cx.notify();
                },
            ))
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
                    .child(ui::checkbox(all_checked))
                    .on_click(cx.listener(|app, _: &ClickEvent, _, cx| {
                        if !app.busy() {
                            app.toggle_visible(cx);
                        }
                    })),
            )
            .child(column(90.).text_right().child("Size"))
            .child(column(64.).child("Safety"))
            .child(column(110.).child("Category"))
            .child(column(150.).child("Rule"))
            .child(column(56.).text_right().child("Changed"))
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
                                        .text_right()
                                        .child(theme::bytes(candidate.usage.allocated)),
                                )
                                .child(column(64.).child(ui::safety_label(review)))
                                .child(
                                    column(110.)
                                        .text_color(theme::muted())
                                        .child(candidate.rule.category.as_str()),
                                )
                                .child(column(150.).truncate().child(candidate.rule.name.clone()))
                                .child(
                                    column(56.)
                                        .text_right()
                                        .text_color(theme::muted())
                                        .child(theme::age(candidate.newest)),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_color(theme::muted())
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
            .child(column(90.).text_right().child("Size"))
            .child(column(64.).child("Safety"))
            .child(column(90.).child("Tool"))
            .child(column(170.).child("Cache"))
            .child(column(56.).text_right().child("Changed"))
            .child(div().flex_1().child("Path"));
        let rows = report.caches.iter().map(|(cache, candidate)| {
            let id = candidate.id;
            let checked = self.selection.contains(&(Source::Cache, id));
            let review = candidate.rule.safety == Safety::Review;
            table_row(("cache", id), checked)
                .child(ui::checkbox(checked))
                .child(
                    column(90.)
                        .text_right()
                        .child(theme::bytes(candidate.usage.allocated)),
                )
                .child(column(64.).child(ui::safety_label(review)))
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
                                .truncate()
                                .text_color(theme::muted())
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
                    .child(format!(
                        "safe {}",
                        theme::bytes(report.cache_total(Some(Safety::Safe)))
                    ))
                    .child(div().text_color(theme::review()).child(format!(
                        "review {}",
                        theme::bytes(report.cache_total(Some(Safety::Review)))
                    )))
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
                    .child(ui::stat(
                        "Reclaimable",
                        theme::bytes(wasted),
                        "by keeping one copy of each",
                    ))
                    .child(ui::stat(
                        "Groups",
                        found.groups.len().to_string(),
                        format!("{copies} files in total"),
                    ))
                    .child(ui::stat(
                        "Files compared",
                        found.files.to_string(),
                        format!("in {:.1}s", found.duration.as_secs_f32()),
                    )),
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
                .children(found.groups.iter().take(DUPLICATE_GROUPS).map(|group| {
                    div()
                        .flex()
                        .flex_col()
                        .gap(unit(4.))
                        .pt(unit(8.))
                        .border_t_1()
                        .border_color(theme::border())
                        .child(ui::bar_row(
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
                        ))
                        .children(group.members.iter().map(|member| {
                            div()
                                .flex()
                                .gap(unit(8.))
                                .text_color(theme::muted())
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .child(model::short_path(&member.path, &report.roots)),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .text_color(theme::faint())
                                        .child(theme::age(member.modified)),
                                )
                        }))
                }))
                .when(found.groups.is_empty(), |this| {
                    this.child(ui::empty("No duplicate files found"))
                }),
            )
            .into_any_element()
    }

    fn render_history(&self) -> AnyElement {
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
        let runs: Vec<&kuizer::history::Run> = view.delete_runs().rev().collect();
        let recent: Vec<&kuizer::history::Run> =
            runs.iter().take(HISTORY_BARS).rev().copied().collect();
        let tallest = recent.iter().map(|run| run.total.bytes).max().unwrap_or(0);
        let chart = div()
            .flex()
            .items_end()
            .gap(unit(4.))
            .h(unit(120.))
            .children(recent.iter().map(|run| {
                div()
                    .flex_1()
                    .max_w(unit(40.))
                    .h(gpui::relative(
                        theme::share(run.total.bytes, tallest).max(0.02),
                    ))
                    .rounded_t(unit(2.))
                    .bg(theme::bar())
            }));
        let header = table_header()
            .child(column(56.).child("Run"))
            .child(column(150.).child("Started"))
            .child(column(70.).text_right().child("Items"))
            .child(column(90.).text_right().child("Size"))
            .child(column(80.).child("Mode"))
            .child(div().flex_1().child("Command"));
        scroll("history")
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(unit(10.))
                    .child(ui::stat(
                        "Cleanups",
                        cumulative.runs.to_string(),
                        cumulative
                            .first_run
                            .as_deref()
                            .map(|first| format!("since {}", short_time(first)))
                            .unwrap_or_default(),
                    ))
                    .child(ui::stat(
                        "Items removed",
                        cumulative.items.to_string(),
                        format!("{} restored", cumulative.restored_items),
                    ))
                    .child(ui::stat(
                        "Freed",
                        theme::bytes(view.freed()),
                        "removed or emptied from the trash",
                    ))
                    .child(ui::stat(
                        "Still in the trash",
                        if view.trash_known {
                            theme::bytes(view.in_trash())
                        } else {
                            "unknown".to_owned()
                        },
                        "restore with kuizer restore",
                    )),
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
                    .children(runs.iter().map(|run| {
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
                                    .text_color(theme::muted())
                                    .child(short_time(&run.started_at)),
                            )
                            .child(column(70.).text_right().child(run.total.items.to_string()))
                            .child(
                                column(90.)
                                    .text_right()
                                    .child(theme::bytes(run.total.bytes)),
                            )
                            .child(column(80.).text_color(theme::muted()).child(mode))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(theme::faint())
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

    fn render_issues(&self) -> AnyElement {
        let Some(report) = self.report() else {
            return div().into_any_element();
        };
        if report.issues.is_empty() {
            return centered(ui::empty("The scan reported no issues"));
        }
        scroll("issues")
            .child(div().text_color(theme::muted()).child(format!(
                "{} folders could not be read or were skipped",
                report.issues.len()
            )))
            .children(report.issues.iter().take(ISSUE_LIMIT).map(|issue| {
                div()
                    .flex()
                    .flex_col()
                    .gap(unit(2.))
                    .py(unit(6.))
                    .border_b_1()
                    .border_color(theme::border())
                    .child(div().truncate().child(display_path(&issue.path)))
                    .child(
                        div()
                            .text_size(unit(12.))
                            .text_color(theme::faint())
                            .child(issue.message.clone()),
                    )
            }))
            .into_any_element()
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
                                            .flex_1()
                                            .min_w_0()
                                            .truncate()
                                            .child(self.dialog_path(candidate)),
                                    )
                                    .child(ui::safety_label(
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
                                ui::button("cancel", "Cancel", ButtonKind::Secondary, true)
                                    .on_click(cx.listener(|app, _: &ClickEvent, _, cx| {
                                        app.dialog = None;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                ui::button("confirm", "Move to trash", ButtonKind::Primary, accepted)
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
                        finished
                            .failures
                            .iter()
                            .take(LIST_LIMIT)
                            .map(|(path, error)| {
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(div().truncate().child(path.clone()))
                                    .child(
                                        div()
                                            .text_size(unit(12.))
                                            .text_color(theme::faint())
                                            .child(error.clone()),
                                    )
                            }),
                    )
                })
                .children(finished.history_error.as_ref().map(|error| {
                    div()
                        .text_color(theme::danger())
                        .child(format!("The history could not be saved: {error}"))
                }))
                .child(div().flex().justify_end().child(
                    ui::button("done", "Done", ButtonKind::Primary, true).on_click(cx.listener(
                        |app, _: &ClickEvent, _, cx| {
                            app.dialog = None;
                            cx.notify();
                        },
                    )),
                )),
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
        let start = matches!(self.phase, Phase::Start);
        div()
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
            .child(wheel)
            .child(self.render_titlebar(cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .when(!start, |this| this.child(self.render_sidebar(cx)))
                    .child(div().flex().flex_col().flex_1().min_w_0().child(content)),
            )
            .when(!start, |this| this.child(self.render_statusbar(cx)))
            .children(dialog)
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
        .gap(unit(14.))
        .size_full()
        .p(unit(16.))
        .overflow_y_scroll()
}

fn column(width: f32) -> Div {
    div().w(unit(width)).flex_none()
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
                Recent::load(Some(recent)),
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
}
