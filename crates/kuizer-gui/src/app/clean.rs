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
        let locked = self
            .locked(items.iter().map(|(source, _)| *source))
            .is_some();
        if items.is_empty() || self.busy() || locked {
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
            typed: String::new(),
            cancel: false,
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

    /// Whether the open confirmation may run: a selection with review items needs its item count
    /// typed first, as the CLI asks.
    pub(super) fn confirmed(&self) -> bool {
        let Some(Dialog::Confirm { stats, typed, .. }) = &self.dialog else {
            return false;
        };
        let total = stats.total();
        total.review.count == 0 || *typed == total.all.count.to_string()
    }

    pub(super) fn accept(&mut self, cx: &mut Context<Self>) {
        let Some(report) = self.report() else {
            return;
        };
        let items = self.confirming();
        let locked = self
            .locked(items.iter().map(|(source, _)| *source))
            .is_some();
        if !self.confirmed() || locked {
            return;
        }
        let mut paths: HashMap<Source, HashSet<PathBuf>> = HashMap::new();
        for (source, candidate) in &items {
            paths
                .entry(*source)
                .or_default()
                .insert(candidate.path.clone());
        }
        let count = items.len();
        let mut artifacts = Vec::new();
        let mut caches = Vec::new();
        let mut toolchains = Vec::new();
        let mut docker = Vec::new();
        for (source, candidate) in items {
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
        let mode = self.config.delete.mode;
        let spawned = spawn_worker("kuizer-gui-clean", move || {
            let outcome = model::clean(batches, mode);
            let history = HistoryView::load();
            (outcome, history)
        });
        let receiver = match spawned {
            Ok(receiver) => receiver,
            Err(error) => {
                self.dialog = Some(Dialog::Finished(Finished {
                    removed: 0,
                    removed_bytes: 0,
                    trashed: 0,
                    trashed_bytes: 0,
                    failures: vec![(String::new(), error)],
                    history_error: None,
                }));
                cx.notify();
                return;
            }
        };
        self.dialog = None;
        let task = cx.spawn(async move |this, cx| {
            let message = receiver.await.ok();
            let _ = this.update(cx, |app, cx| app.finish(message, cx));
        });
        self.cleaning = Some(Cleaning {
            paths,
            started: Instant::now(),
            _task: task,
        });
        self.show_toast(Notice::Deleting(count), cx);
    }

    /// Keeps deleted items out of the scan saved for the next launch: a running scan leaves
    /// them out when it saves, and the snapshot already on disk is rewritten without them.
    fn forget_in_snapshot(&self, removed: &HashMap<Source, HashSet<PathBuf>>) {
        let paths: HashSet<PathBuf> = removed.values().flatten().cloned().collect();
        if paths.is_empty() {
            return;
        }
        if self.cancel.is_some() {
            self.forgotten.add(paths.iter().cloned());
        }
        let (snapshots, roots) = (self.snapshots.clone(), self.roots.clone());
        let spawned = std::thread::Builder::new()
            .name("kuizer-gui-forget".to_owned())
            .spawn(move || snapshots.forget(&roots, &paths));
        if let Err(error) = spawned {
            eprintln!("kuizer-gui: cannot update the saved scan: {error}");
        }
    }

    pub(super) fn finish(
        &mut self,
        message: Option<(Outcome, Result<HistoryView, String>)>,
        cx: &mut Context<Self>,
    ) {
        let cleaning = self.cleaning.take();
        if matches!(
            self.toast,
            Some(Toast {
                notice: Notice::Deleting(_),
                ..
            })
        ) {
            self.toast = None;
        }
        let Some((outcome, history)) = message else {
            self.settle(cleaning, HashMap::new(), cx);
            self.dialog = Some(Dialog::Finished(Finished {
                removed: 0,
                removed_bytes: 0,
                trashed: 0,
                trashed_bytes: 0,
                failures: vec![(String::new(), "the cleanup stopped unexpectedly".to_owned())],
                history_error: None,
            }));
            cx.notify();
            return;
        };
        let mut removed: HashMap<Source, HashSet<PathBuf>> = HashMap::new();
        for (source, removal) in outcome.removed() {
            removed
                .entry(*source)
                .or_default()
                .insert(removal.candidate.path.clone());
        }
        let trashed = || {
            outcome
                .removed()
                .filter(|(source, _)| outcome.trashed(*source))
        };
        let finished = Finished {
            trashed: trashed().count(),
            trashed_bytes: trashed()
                .map(|(_, removal)| removal.candidate.usage.allocated)
                .sum(),
            removed: outcome.removed().count(),
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
        let deselected: Vec<(Source, u64)> = self
            .selected()
            .into_iter()
            .filter(|(source, candidate)| {
                removed
                    .get(source)
                    .is_some_and(|paths| paths.contains(&candidate.path))
            })
            .map(|(source, candidate)| (source, candidate.id))
            .collect();
        for key in &deselected {
            self.selection.remove(key);
            self.flow.mark(*key);
        }
        if let Phase::Ready(report) = &mut self.phase {
            report.history = history;
        }
        self.forget_in_snapshot(&removed);
        if self.cancel.is_some() {
            self.history_fresh = true;
        }
        self.settle(cleaning, removed, cx);
        let clean = finished.failures.is_empty() && finished.history_error.is_none();
        if clean && finished.removed > 0 {
            self.show_toast(Notice::Info(finished.summary()), cx);
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
            Notice::Deleting(count) => panel.child(
                line.child(
                    div()
                        .debug_selector(|| "toast-deleting".to_owned())
                        .text_color(theme::muted())
                        .child(format!("Deleting {}", plural(*count, "item"))),
                ),
            ),
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
            Dialog::Branches { scope, cancel } => {
                self.render_branch_dialog(scope, *cancel, panel, cx)
            }
            Dialog::Finished(finished) => panel
                .child(
                    div()
                        .text_size(unit(font::TITLE))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(if finished.removed == 0 {
                            "Nothing was deleted".to_owned()
                        } else {
                            finished.summary()
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
        let Some(Dialog::Confirm {
            stats,
            preview,
            typed,
            cancel,
            ..
        }) = &self.dialog
        else {
            return None;
        };
        let cancel = *cancel;
        let ring = || ring::outline(None, Rc::new(Cell::new(false)));
        let confirmed = self.confirmed();
        let total = stats.total();
        let count = total.all.count;
        let bytes = total.all.bytes;
        let review = total.review.count;
        let lasting = lasting(stats);
        let trash = self.trashes(stats);
        let title = if trash {
            format!("Move {} to the trash?", plural(count - lasting, "item"))
        } else {
            format!("Delete {} for good?", plural(count, "item"))
        };
        let toolchain_note = if stats.source(Source::Toolchain).all.count > 0 {
            " The tool downloads a toolchain version again when it is needed."
        } else {
            ""
        };
        let warning = if !trash {
            format!("This cannot be undone.{toolchain_note}")
        } else if lasting > 0 {
            format!(
                "{} deleted for good and cannot be restored.{toolchain_note}",
                plural(lasting, "toolchain or Docker item")
            )
        } else {
            "The space is freed once the trash is emptied. History restores them until then."
                .to_owned()
        };
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
                    .child(self.render_mode())
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                    .child(
                        div()
                            .text_color(theme::muted())
                            .child(format!("{} in total.", theme::bytes(bytes))),
                    ),
            )
            .child(
                div()
                    .debug_selector(move || {
                        if trash && lasting == 0 {
                            "trash-note".to_owned()
                        } else {
                            "permanent-warning".to_owned()
                        }
                    })
                    .text_color(if trash && lasting == 0 {
                        theme::muted()
                    } else {
                        theme::danger()
                    })
                    .child(warning),
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
                                .child(format!("{review} marked review. Type {count} to confirm:")),
                        )
                        .child(
                            div().w(unit(col::M)).child(
                                field::view("confirm-count", typed, count.to_string(), true)
                                    .debug_selector(|| "confirm-count".to_owned()),
                            ),
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
                            .when(cancel, |this| this.child(ring()))
                            .on_click(cx.listener(|app, _: &ClickEvent, _, cx| {
                                app.dialog = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        ui::button(
                            "confirm",
                            self.delete_label(stats),
                            ButtonKind::Primary,
                            confirmed,
                        )
                        .tip(
                            Tooltip::new(if !confirmed {
                                "Type the item count first"
                            } else if trash {
                                "Move these items to the trash"
                            } else {
                                "Delete these items for good"
                            })
                            .keys(&["Enter"])
                            .build(),
                        )
                        .when(!cancel, |this| this.child(ring()))
                        .on_click(cx.listener(
                            |app, _: &ClickEvent, _, cx| {
                                app.accept(cx);
                            },
                        )),
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

    /// Whether removing these items moves any of them to the trash. An empty selection answers
    /// for the setting alone.
    pub(super) fn trashes(&self, stats: &SelectionStats) -> bool {
        let count = stats.total().all.count;
        self.config.delete.mode == DeleteMode::Trash && (count == 0 || lasting(stats) < count)
    }

    pub(super) fn delete_label(&self, stats: &SelectionStats) -> &'static str {
        if self.trashes(stats) {
            "Move to trash"
        } else {
            "Delete"
        }
    }

    pub(super) fn dialog_path(&self, candidate: &Candidate) -> String {
        self.shown_path(&candidate.path)
    }
}

fn lasting(stats: &SelectionStats) -> usize {
    Source::ALL
        .into_iter()
        .filter(|source| source.permanent())
        .map(|source| stats.source(source).all.count)
        .sum()
}

impl Finished {
    fn summary(&self) -> String {
        let deleted = self.removed - self.trashed;
        let freed = theme::bytes(self.removed_bytes - self.trashed_bytes);
        let trashed = format!(
            "{} moved to the trash, {}",
            plural(self.trashed, "item"),
            theme::bytes(self.trashed_bytes)
        );
        match (self.trashed, deleted) {
            (0, _) => format!("{} deleted, {freed} freed", plural(deleted, "item")),
            (_, 0) => trashed,
            _ => format!(
                "{trashed}. {} deleted, {freed} freed",
                plural(deleted, "item")
            ),
        }
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
