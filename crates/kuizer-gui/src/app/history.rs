use super::*;

impl Kuizer {
    pub(super) fn render_history(&self, cx: &mut Context<Self>) -> AnyElement {
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
        let mut recent: Vec<&kuizer::history::Run> = view
            .history
            .runs
            .iter()
            .rev()
            .filter(|run| run.action == Action::Delete)
            .take(HISTORY_BARS)
            .collect();
        recent.reverse();
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
            .child(column(col::XL).child("STARTED"))
            .child(column(col::S).text_right().child("ITEMS"))
            .child(head(col::M, "Size", Term::Size).text_right())
            .child(head(col::M, "Mode", Term::Mode))
            .child(head(col::L, "Trash", Term::Restore))
            .child(
                div()
                    .id("Command")
                    .flex_1()
                    .tip(tooltip::term(Term::Command))
                    .child("COMMAND"),
            );
        let toolbar = self
            .list_toolbar(cx)
            .child(div().flex_1())
            .child(self.tally(Tab::History, None));
        let summary = div()
            .flex()
            .flex_col()
            .flex_none()
            .gap(unit(space::XL))
            .p(unit(space::GUTTER))
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
            );
        let count = self.listed(Tab::History).slots.len();
        let list = self
            .table_list(
                "history",
                Tab::History,
                count,
                |app, range, cx| app.history_rows(range, cx),
                cx,
            )
            .debug_selector(|| "history-list".to_owned());
        let body = div()
            .flex()
            .flex_col()
            .size_full()
            .child(summary)
            .child(table(
                header,
                if count == 0 {
                    div().flex_1().into_any_element()
                } else {
                    list.into_any_element()
                },
            ))
            .into_any_element();
        framed(toolbar, body)
    }

    fn history_rows(&self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<Stateful<Div>> {
        let Some(report) = self.report() else {
            return Vec::new();
        };
        let Ok(view) = &report.history else {
            return Vec::new();
        };
        let listed = self.listed(Tab::History);
        let can_restore = self.can_restore();
        let refreshing = self.refreshing();
        let cursor = self.cursor_target();
        range
            .filter_map(|row| {
                let index = listed.slots.get(row)?.0;
                let run = view.history.runs.get(index)?;
                let mode = run.deleted.first().map_or("", |entry| match entry.mode {
                    kuizer::delete::Mode::Trash => "trash",
                    kuizer::delete::Mode::Permanent => "permanent",
                    kuizer::delete::Mode::DryRun => "dry run",
                });
                let session = run.id;
                let line = ui::tabular(div())
                    .id(("run-row", session))
                    .flex()
                    .items_center()
                    .gap(unit(space::L))
                    .w_full()
                    .h(unit(size::row()))
                    .px(unit(space::GUTTER))
                    .debug_selector(move || format!("run-{session}"));
                let target = Target::Run(run.id, run.command.clone());
                Some(
                    self.menu_row(line, target, cursor.as_ref(), cx)
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
                        ),
                )
            })
            .collect()
    }

    pub(super) fn restore_cell(
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
                        app.restore(session, cx);
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
}
