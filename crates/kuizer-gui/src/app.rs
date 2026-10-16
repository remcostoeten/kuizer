use std::collections::HashSet;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::mpsc::{self, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{
    AnyElement, ClickEvent, Context, Div, FocusHandle, FontWeight, KeyDownEvent, SharedString,
    Task, Window, div, prelude::*, px, uniform_list,
};
use kuizer::config::Config;
use kuizer::delete::Status;
use kuizer::detect::{Category, Safety};
use kuizer::history::Action;
use kuizer::output::display_path;
use kuizer::scan::Candidate;

use crate::model::{self, Batch, Confirmation, HistoryView, Outcome, Report, Source, Stage};
use crate::theme;
use crate::ui::{self, ButtonKind};

const POLL: Duration = Duration::from_millis(100);
const LIST_LIMIT: usize = 6;
const DUPLICATE_GROUPS: usize = 200;
const ISSUE_LIMIT: usize = 500;
const HISTORY_BARS: usize = 24;

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
        font: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus);
        let mut app = Self {
            config,
            roots,
            font,
            focus,
            phase: Phase::Loading(Stage::default()),
            tab: Tab::Overview,
            safety: None,
            hidden: HashSet::new(),
            rows: Vec::new(),
            selection: HashSet::new(),
            dialog: None,
            task: None,
        };
        app.rescan(cx);
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
        let stage = Arc::new(Mutex::new(Stage::default()));
        let (sender, receiver) = mpsc::channel();
        {
            let config = self.config.clone();
            let roots = self.roots.clone();
            let stage = Arc::clone(&stage);
            std::thread::Builder::new()
                .name("kuizer-gui-scan".to_owned())
                .spawn(move || {
                    let _ = sender.send(model::load(&config, &roots, &stage));
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
                        Some(Err(error)) => Phase::Failed(error),
                        None => Phase::Loading(current),
                    };
                    app.refresh_rows();
                    cx.notify();
                });
                if done || updated.is_err() {
                    break;
                }
            }
        }));
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

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
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
        let roots = self
            .roots
            .iter()
            .map(|root| display_path(root))
            .collect::<Vec<_>>()
            .join(", ");
        let busy = self.busy();
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(12.))
            .h(px(40.))
            .px(px(14.))
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
            .child(
                ui::button("rescan", "Rescan", ButtonKind::Secondary, !busy).on_click(cx.listener(
                    move |app, _: &ClickEvent, _, cx| {
                        if !app.busy() {
                            app.rescan(cx);
                        }
                    },
                )),
            )
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
            .w(px(200.))
            .gap(px(2.))
            .p(px(8.))
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
                    .h(px(28.))
                    .px(px(10.))
                    .rounded(px(5.))
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
                            .text_size(px(12.))
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
            .gap(px(8.))
            .h(px(40.))
            .px(px(14.))
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
            Phase::Loading(stage) => centered(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(6.))
                    .child(div().font_weight(FontWeight::MEDIUM).child("Scanning"))
                    .child(div().text_color(theme::muted()).child(stage.describe())),
            ),
            Phase::Failed(error) => centered(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(6.))
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
                    .gap(px(10.))
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
                    .gap(px(10.))
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
                    .text_size(px(12.))
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
            let active = self.safety == value;
            div()
                .id(id)
                .px(px(10.))
                .h(px(24.))
                .flex()
                .items_center()
                .rounded(px(4.))
                .cursor_pointer()
                .text_color(if active {
                    theme::text()
                } else {
                    theme::muted()
                })
                .when(active, |this| this.bg(theme::selected()))
                .hover(|style| style.text_color(theme::text()))
                .child(label)
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
                .gap(px(6.))
                .px(px(8.))
                .h(px(24.))
                .rounded(px(4.))
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
            .gap(px(12.))
            .px(px(14.))
            .py(px(8.))
            .border_b_1()
            .border_color(theme::border())
            .child(
                div()
                    .flex()
                    .gap(px(2.))
                    .p(px(2.))
                    .rounded(px(6.))
                    .bg(theme::panel())
                    .border_1()
                    .border_color(theme::border())
                    .child(segment("safety-all", "All", None))
                    .child(segment("safety-safe", "Safe", Some(Safety::Safe)))
                    .child(segment("safety-review", "Review", Some(Safety::Review))),
            )
            .child(
                div()
                    .flex()
                    .gap(px(2.))
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
            .child(div().w(px(14.)))
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
                                    .text_size(px(12.))
                                    .text_color(theme::faint())
                                    .child(cache.note),
                            )
                        }),
                )
                .h(px(44.))
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
                    .gap(px(16.))
                    .px(px(14.))
                    .py(px(10.))
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
                        this.child(ui::empty("No global caches found"))
                    }),
            )
            .into_any_element()
    }

    fn render_duplicates(&self) -> AnyElement {
        let Some(report) = self.report() else {
            return div().into_any_element();
        };
        let Some(found) = &report.duplicates else {
            return centered(ui::empty("Duplicate detection did not run"));
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
                    .gap(px(10.))
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
                        .gap(px(4.))
                        .pt(px(8.))
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
                                .gap(px(8.))
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
            .gap(px(4.))
            .h(px(120.))
            .children(recent.iter().map(|run| {
                div()
                    .flex_1()
                    .max_w(px(40.))
                    .h(gpui::relative(
                        theme::share(run.total.bytes, tallest).max(0.02),
                    ))
                    .rounded_t(px(2.))
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
                    .gap(px(10.))
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
                    .rounded(px(6.))
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
                            .gap(px(12.))
                            .h(px(30.))
                            .px(px(14.))
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
                            .text_size(px(12.))
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
                    .gap(px(2.))
                    .py(px(6.))
                    .border_b_1()
                    .border_color(theme::border())
                    .child(div().truncate().child(display_path(&issue.path)))
                    .child(
                        div()
                            .text_size(px(12.))
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
            .gap(px(12.))
            .w(px(520.))
            .p(px(18.))
            .rounded(px(8.))
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
                            .text_size(px(15.))
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
                            .gap(px(4.))
                            .p(px(10.))
                            .rounded(px(6.))
                            .bg(theme::panel())
                            .children(selected.iter().take(LIST_LIMIT).map(|(_, candidate)| {
                                div()
                                    .flex()
                                    .gap(px(8.))
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
                                            .w(px(80.))
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
                                    .h(px(30.))
                                    .px(px(10.))
                                    .rounded(px(5.))
                                    .bg(theme::background())
                                    .border_1()
                                    .border_color(if accepted {
                                        theme::primary()
                                    } else {
                                        theme::border_strong()
                                    })
                                    .child(typed.clone())
                                    .child(div().w(px(1.)).h(px(16.)).bg(theme::text())),
                            )
                        },
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap(px(8.))
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
                        .text_size(px(15.))
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
                                            .text_size(px(12.))
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = self.render_content(cx);
        let dialog = self.render_dialog(cx);
        div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme::background())
            .text_color(theme::text())
            .font_family(self.font.clone())
            .text_size(px(13.))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .child(self.render_titlebar(cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.render_sidebar(cx))
                    .child(div().flex().flex_col().flex_1().min_w_0().child(content)),
            )
            .child(self.render_statusbar(cx))
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
        .gap(px(14.))
        .size_full()
        .p(px(16.))
        .overflow_y_scroll()
}

fn column(width: f32) -> Div {
    div().w(px(width)).flex_none()
}

fn table_header() -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(12.))
        .h(px(30.))
        .px(px(14.))
        .bg(theme::panel())
        .border_b_1()
        .border_color(theme::border())
        .text_size(px(12.))
        .text_color(theme::muted())
}

fn table_row(id: (&'static str, u64), checked: bool) -> gpui::Stateful<Div> {
    div()
        .id(gpui::ElementId::NamedInteger(id.0.into(), id.1))
        .flex()
        .items_center()
        .gap(px(12.))
        .w_full()
        .h(px(30.))
        .px(px(14.))
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
