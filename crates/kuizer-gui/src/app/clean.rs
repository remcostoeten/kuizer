use gpui::BoxShadow;

use super::*;

impl Kuizer {
    pub(super) fn restore(&mut self, session: u64, cx: &mut Context<Self>) {
        if !self.can_restore() {
            return;
        }
        let Some(history) = self
            .report()
            .and_then(|report| report.history.as_ref().ok())
            .map(|view| view.history.clone())
        else {
            return;
        };
        let spawned = spawn_worker("kuizer-gui-restore", move || {
            model::restore(history, session)
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
        message: Option<(Result<Restored, String>, Option<HistoryView>)>,
        cx: &mut Context<Self>,
    ) {
        let Some((restored, history)) = message else {
            self.show_toast(
                Notice::Failed("Could not restore: the restore stopped unexpectedly".to_owned()),
                cx,
            );
            return;
        };
        if let Phase::Ready(report) = &mut self.phase
            && let Some(history) = history
        {
            report.history = Ok(history);
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

    pub(super) fn trash(&mut self, cx: &mut Context<Self>) {
        if !self.selection.is_empty() {
            self.delete(None, cx);
        }
    }

    /// Starts deleting the selection, or one item when `only` is set, unless something stands
    /// in the way.
    pub(super) fn delete(&mut self, only: Option<(Source, u64)>, cx: &mut Context<Self>) {
        let Some(report) = self.report() else {
            return;
        };
        let items = match only {
            Some(key) => self.item(key).into_iter().collect(),
            None => self.selected(),
        };
        let locked = self
            .locked(items.iter().map(|(source, _)| *source))
            .is_some();
        if items.is_empty() || self.busy() || locked || self.saving_protected_paths() {
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
        let history = report
            .history
            .as_ref()
            .ok()
            .map(|view| (view.history.clone(), view.trash.clone()));
        let delete_config = self.config.delete.clone();
        let engines = self.config.docker.engines.clone();
        let spawned = spawn_worker("kuizer-gui-clean", move || {
            model::clean(batches, &delete_config, &engines, history)
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
            .h(unit(size::SEGMENT))
            .px(unit(space::M))
            .text_size(unit(font::SMALL))
            .tip(
                Tooltip::new("Hide this message")
                    .keys(&[&self.key_hint(Scope::GuiBack, Key::Back)])
                    .build(),
            )
        };
        let line = div()
            .flex()
            .items_center()
            .gap(unit(space::M))
            .min_h(unit(size::SEGMENT));
        let panel = floating(div().id("toast"))
            .debug_selector(|| "toast".to_owned())
            .flex()
            .flex_col()
            .gap(unit(space::S))
            .max_w(unit(size::PALETTE))
            .py(unit(space::S))
            .pl(unit(space::L))
            .pr(unit(space::S))
            .text_size(unit(font::SMALL));
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
                line.child(icon::sized(Icon::Check, theme::ok(), size::CHECK))
                    .child(div().flex_1().child(text.clone()))
                    .child(dismiss()),
            ),
        };
        Some(panel.into_any_element())
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
            Dialog::Branches { .. } | Dialog::Exclude { .. } => return None,
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
                            .tip(
                                Tooltip::new("Close this summary")
                                    .keys(&[&self.key_hint(Scope::Dialog, Key::Confirm)])
                                    .build(),
                            )
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

    /// The floating bar that sums up the selection and deletes it, over the bottom of the list.
    pub(super) fn render_summary(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (stats, shown) = self.flow.bar()?;
        let total = stats.total();
        let count = total.all.count;
        let review = total.review.count;
        let lasting = lasting(&stats);
        let trash = self.trashes(&stats);
        let note = match (trash, lasting) {
            (false, _) => "deleted for good".to_owned(),
            (true, 0) => "moved to the trash".to_owned(),
            (true, lasting) => format!(
                "{} to the trash, {} deleted for good",
                count - lasting,
                plural(lasting, "toolchain or Docker item")
            ),
        };
        let ready = self.report().is_some() && !self.busy();
        let lock = self.trash_lock();
        let bar = floating(div().id("summary-bar"))
            .debug_selector(|| "summary-bar".to_owned())
            .flex()
            .items_center()
            .gap(unit(space::L))
            .min_w(unit(size::SUMMARY))
            .max_w_full()
            .p(unit(space::S))
            .when(shown < 1.0, |this| {
                this.opacity(shown)
                    .relative()
                    .top(unit(space::M * (1.0 - shown)))
            })
            .child(
                theme::mono(ui::tabular(div()))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .h(unit(size::TILE))
                    .min_w(unit(size::TILE))
                    .px(unit(space::M))
                    .rounded(theme::radius_control())
                    .bg(theme::tile_selected())
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(count.to_string()),
            )
            .child(
                ui::tabular(div())
                    .id("summary-selection")
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(unit(space::XS))
                    .text_color(theme::muted())
                    .tip(selection_details(stats))
                    .child("selected ·")
                    .child(
                        div()
                            .text_color(theme::text())
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(theme::bytes(total.all.bytes)),
                    ),
            )
            .when(review > 0, |this| {
                this.child(
                    ui::tag(
                        "summary-review",
                        format!("{review} review"),
                        ui::Tone::Review,
                    )
                    .tip(tooltip::term(Term::Review)),
                )
            })
            .child(div().flex_1())
            .child(
                div()
                    .debug_selector(move || {
                        if trash && lasting == 0 {
                            "trash-note".to_owned()
                        } else {
                            "permanent-warning".to_owned()
                        }
                    })
                    .min_w_0()
                    .truncate()
                    .text_size(unit(font::SMALL))
                    .text_color(theme::faint())
                    .child(note),
            )
            .child(
                self.button(
                    "clear",
                    "Clear",
                    ButtonKind::Ghost,
                    ready,
                    cx,
                    |app, _, cx| {
                        if !app.busy() {
                            app.selection.clear();
                            cx.notify();
                        }
                    },
                )
                .flex_none()
                .pr(unit(space::S))
                .child(ui::kbd(self.key_hint(Scope::Dialog, Key::Back)))
                .tip(tooltip::plain("Clear the selection in every tab")),
            )
            .child(
                self.button(
                    "trash",
                    self.delete_label(&stats),
                    if trash {
                        ButtonKind::Primary
                    } else {
                        ButtonKind::Danger
                    },
                    ready && lock.is_none(),
                    cx,
                    |app, _, cx| app.delete(None, cx),
                )
                .flex_none()
                .h(unit(size::TILE))
                .pl(unit(space::M))
                .pr(unit(space::S))
                .gap(unit(space::S))
                .map(|this| {
                    let ink = if trash {
                        theme::on_primary()
                    } else {
                        theme::on_danger()
                    };
                    let fill = if trash {
                        theme::kbd()
                    } else {
                        theme::kbd_on_danger()
                    };
                    this.child(ui::kbd_on(
                        self.key_hint(Scope::Dialog, Key::Confirm),
                        fill,
                        ink,
                    ))
                })
                .map(|this| match lock {
                    Some(reason) => this.tip(tooltip::plain(reason)),
                    None => this.tip(tooltip::term(Term::Trash)),
                }),
            );
        Some(bar.into_any_element())
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

/// The look shared by the bars that float over the bottom of the list: the selection summary,
/// the toast and the branch confirmation.
pub(super) fn floating(bar: Stateful<Div>) -> Stateful<Div> {
    bar.rounded(theme::radius_dialog())
        .bg(theme::elevated())
        .border_1()
        .border_color(theme::border_strong())
        .shadow(vec![
            BoxShadow {
                color: theme::shadow(),
                offset: point(px(0.), px(space::M)),
                blur_radius: px(space::XXL),
                spread_radius: px(0.),
            },
            BoxShadow {
                color: theme::shadow_contact(),
                offset: point(px(0.), px(size::HAIR)),
                blur_radius: px(space::XXS),
                spread_radius: px(0.),
            },
        ])
        .occlude()
}
