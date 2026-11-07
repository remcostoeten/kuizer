use gpui::{Animation, AnimationExt};

use super::*;

const INLINE_PATHS: usize = 3;
const CONFIRM_IN: Duration = Duration::from_millis(120);

impl Kuizer {
    pub(super) fn restore(&mut self, session: u64, cx: &mut Context<Self>) {
        if !self.can_restore() {
            return;
        }
        let spawned = spawn_worker("kuizer-gui-restore", move || {
            let restored = model::restore(session);
            let history = HistoryView::load();
            (restored, history)
        });
        let receiver = match spawned {
            Ok(receiver) => receiver,
            Err(error) => {
                self.show_toast(Notice::Failed(format!("Could not restore: {error}")), cx);
                return;
            }
        };
        self.show_toast(Notice::Restoring { session }, cx);
        self.restore_task = Some(cx.spawn(async move |this, cx| {
            let message = receiver.await.ok();
            let _ = this.update(cx, |app, cx| app.restored(message, cx));
        }));
    }

    pub(super) fn restored(
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

    pub(super) fn open_confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm(None, window, cx);
    }

    /// Opens the delete confirmation for the selection, or for one item when `only` is set.
    pub(super) fn confirm(
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
        let stats = SelectionStats::of(items.iter().copied());
        let preview = items
            .iter()
            .take(LIST_LIMIT)
            .map(|(source, candidate)| (*source, candidate.id))
            .collect();
        self.dialog = Some(Dialog::Confirm {
            only,
            stats,
            preview,
        });
        window.focus(&self.focus);
        cx.notify();
    }

    /// What the open confirmation acts on: one item from a context menu, or the selection.
    pub(super) fn confirming(&self) -> Vec<(Source, &Candidate)> {
        match &self.dialog {
            Some(Dialog::Confirm {
                only: Some(key), ..
            }) => self.item(*key).into_iter().collect(),
            _ => self.selected(),
        }
    }

    pub(super) fn accept(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.dialog, Some(Dialog::Confirm { .. })) || self.refreshing() {
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
        let spawned = spawn_worker("kuizer-gui-clean", move || {
            let outcome = model::clean(batches);
            let history = HistoryView::load();
            (outcome, history)
        });
        let receiver = match spawned {
            Ok(receiver) => receiver,
            Err(error) => {
                self.dialog = Some(Dialog::Finished(Finished {
                    removed: 0,
                    removed_bytes: 0,
                    failures: vec![(String::new(), error)],
                    history_error: None,
                }));
                cx.notify();
                return;
            }
        };
        self.dialog = Some(Dialog::Running);
        cx.notify();
        self.task = Some(cx.spawn(async move |this, cx| {
            let message = receiver.await.ok();
            let _ = this.update(cx, |app, cx| app.finish(message, cx));
        }));
    }

    pub(super) fn finish(
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
        self.refresh_map();
        let clean = finished.failures.is_empty() && finished.history_error.is_none();
        if clean && finished.removed > 0 {
            self.show_toast(
                Notice::Info(format!(
                    "{} deleted, {} freed",
                    plural(finished.removed, "item"),
                    theme::bytes(finished.removed_bytes)
                )),
                cx,
            );
        }
        self.dialog = (!clean).then_some(Dialog::Finished(finished));
        cx.notify();
    }

    pub(super) fn render_toast(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let toast = self.toast.as_ref()?;
        let dismiss = || {
            self.button(
                "dismiss-toast",
                "Dismiss",
                ButtonKind::Ghost,
                true,
                cx,
                |app, _, cx| {
                    app.dismiss_toast(cx);
                },
            )
            .debug_selector(|| "dismiss-toast".to_owned())
            .tip(Tooltip::new("Hide this message").keys(&["Esc"]).build())
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

    pub(super) fn render_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
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
            Dialog::Confirm { .. } => return None,
            Dialog::Branches(scope) => self.render_branch_dialog(scope, panel, cx),
            Dialog::Running => panel
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Deleting items"),
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
                        .child(if finished.removed == 0 {
                            "Nothing was deleted".to_owned()
                        } else {
                            format!(
                                "Deleted {}, {}",
                                plural(finished.removed, "item"),
                                theme::bytes(finished.removed_bytes)
                            )
                        }),
                )
                .when(!finished.failures.is_empty(), |this| {
                    this.child(
                        div()
                            .text_color(theme::danger())
                            .child(format!("{} could not be deleted", finished.failures.len())),
                    )
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

    /// The delete confirmation, drawn in place of the status bar.
    pub(super) fn render_confirm(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let Some(Dialog::Confirm { stats, preview, .. }) = &self.dialog else {
            return None;
        };
        let total = stats.total();
        let count = total.all.count;
        let bytes = total.all.bytes;
        let review = total.review.count;
        let title = format!("Delete {} for good?", plural(count, "item"));
        let selected: Vec<(Source, &Candidate)> =
            preview.iter().filter_map(|&key| self.item(key)).collect();
        let panel = div()
            .id("confirm-panel")
            .debug_selector(|| "confirm-panel".to_owned())
            .flex()
            .flex_none()
            .flex_col()
            .gap(unit(space::S))
            .py(unit(space::M))
            .px(unit(space::GUTTER))
            .bg(theme::panel())
            .border_t_1()
            .border_color(theme::border_strong())
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(unit(space::M))
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                    .child(
                        div()
                            .text_color(theme::muted())
                            .child(format!("{} in total.", theme::bytes(bytes))),
                    ),
            )
            .child(
                div()
                    .debug_selector(|| "permanent-warning".to_owned())
                    .text_color(theme::danger())
                    .child(format!(
                        "This cannot be undone.{}",
                        if stats.source(Source::Toolchain).all.count > 0 {
                            " The tool downloads a toolchain version again when it is needed."
                        } else {
                            ""
                        }
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .text_color(theme::muted())
                    .children(selected.iter().take(INLINE_PATHS).map(|(_, candidate)| {
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
                                    .child(theme::bytes(candidate.usage.allocated)),
                            )
                    }))
                    .when(count > INLINE_PATHS, |this| {
                        this.child(
                            div()
                                .text_color(theme::faint())
                                .child(format!("and {} more", count - INLINE_PATHS)),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(unit(space::M))
                    .when(review > 0, |this| {
                        this.child(
                            div()
                                .text_color(theme::review())
                                .child(format!("{review} marked review.")),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        ui::button("cancel", "Cancel", ButtonKind::Secondary, true)
                            .tip(
                                Tooltip::new("Keep everything where it is")
                                    .keys(&["Esc"])
                                    .build(),
                            )
                            .on_click(cx.listener(|app, _: &ClickEvent, _, cx| {
                                app.dialog = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        ui::button("confirm", "Delete", ButtonKind::Primary, true)
                            .tip(
                                Tooltip::new("Delete these items for good")
                                    .keys(&["Enter"])
                                    .build(),
                            )
                            .on_click(cx.listener(|app, _: &ClickEvent, _, cx| {
                                app.accept(cx);
                            })),
                    ),
            );
        Some(if motion::reduced() {
            panel.into_any_element()
        } else {
            panel
                .with_animation(
                    "confirm-in",
                    Animation::new(CONFIRM_IN).with_easing(motion::ease_out),
                    |panel, progress| panel.opacity(0.3 + 0.7 * progress),
                )
                .into_any_element()
        })
    }

    pub(super) fn dialog_path(&self, candidate: &Candidate) -> String {
        self.shown_path(&candidate.path)
    }
}

pub(super) fn selection_details(
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
