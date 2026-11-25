use super::*;

impl Kuizer {
    pub(super) fn render_overview(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(report) = self.report() else {
            return div().into_any_element();
        };
        if self.map.shown {
            return self.render_map(cx);
        }
        let figures = &self.report_stats;
        let caches = figures.caches.all.bytes;
        let toolchains = figures.toolchains.all.bytes;
        let containers = figures.docker.all.bytes;
        let space = figures.space();
        let charts = figures.charts(report);
        let categories = &charts.categories;
        let projects = &charts.projects;
        let largest_project = projects.first().map_or(0, |bucket| bucket.bytes);
        let largest_cache = report
            .caches
            .first()
            .map_or(0, |(_, candidate)| candidate.usage.allocated);

        scroll("overview")
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(unit(space::L))
                    .child(self.overview_switch(cx)),
            )
            .child(overview_hero(report, figures))
            .child(self.overview_wins(report, cx))
            .child(self.overview_sections(cx))
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

    /// Why the biggest safe artifacts cannot be selected, or `None` when there are some.
    pub(super) fn wins_blocked(&self) -> Option<&'static str> {
        let report = self.report()?;
        self.report_stats
            .charts(report)
            .wins
            .0
            .is_empty()
            .then_some("No safe items to select")
    }

    /// Selects the biggest safe artifacts and opens their list, where Delete removes them.
    pub(super) fn select_wins(&mut self, cx: &mut Context<Self>) {
        let Some(report) = self.report() else {
            return;
        };
        let ids = self.report_stats.charts(report).wins.0.clone();
        for id in ids {
            self.select((Source::Artifact, id));
        }
        self.go_to(Tab::Artifacts, cx);
        if self.safety == Some(Safety::Review) {
            self.safety = None;
            self.filtered();
        }
    }

    /// The first step to take: the largest safe artifacts, one key away from being selected.
    fn overview_wins(&self, report: &Report, cx: &mut Context<Self>) -> Div {
        let (ids, bytes) = &self.report_stats.charts(report).wins;
        let card = ui::card("Biggest safe wins").debug_selector(|| "overview-wins".to_owned());
        if ids.is_empty() {
            return card.child(ui::empty(if self.refreshing() {
                "No safe items found yet, the scan is still running"
            } else {
                "No safe items in this scan"
            }));
        }
        let picks: Vec<&Candidate> = ids
            .iter()
            .filter_map(|&id| self.index.find(report, (Source::Artifact, id)))
            .collect();
        let largest = picks
            .first()
            .map_or(0, |candidate| candidate.usage.allocated);
        let key = self.hint(&Command::SelectWins);
        let button = self
            .button(
                "overview-wins-select",
                format!("Select {}", plural(ids.len(), "item")),
                ButtonKind::Primary,
                self.blocked(&Command::SelectWins).is_none(),
                cx,
                |app, _, cx| app.select_wins(cx),
            )
            .tip(
                Tooltip::new("Selects them and opens Project artifacts. Nothing is deleted yet")
                    .keys(&[&key])
                    .build(),
            );
        card.child(
            div()
                .flex()
                .items_center()
                .gap(unit(space::L))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .gap(unit(space::XXS))
                        .child(ui::tabular(
                            div()
                                .text_size(unit(font::HEADING))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(theme::bytes(*bytes)),
                        ))
                        .child(
                            div()
                                .text_size(unit(font::SMALL))
                                .text_color(theme::faint())
                                .child(format!(
                                    "in the {} largest safe {}. Select them, check the list, then press {} to delete them",
                                    ids.len(),
                                    if ids.len() == 1 { "item" } else { "items" },
                                    self.hint(&Command::Trash),
                                )),
                        ),
                )
                .child(ui::kbd(key))
                .child(button),
        )
        .children(picks.into_iter().take(WIN_ROWS).enumerate().map(|(index, candidate)| {
            ui::bar_row(
                model::short_path(&candidate.path, &report.roots),
                theme::bytes(candidate.usage.allocated),
                theme::share(candidate.usage.allocated, largest),
                theme::bar(),
            )
            .id(("win", index))
            .tip(tooltip::path(&candidate.path))
        }))
    }

    /// The sections that can free space, largest share first to read, each a way into its list.
    fn overview_sections(&self, cx: &mut Context<Self>) -> Div {
        const SECTIONS: [Tab; 6] = [
            Tab::Artifacts,
            Tab::Caches,
            Tab::Toolchains,
            Tab::Docker,
            Tab::Repos,
            Tab::Duplicates,
        ];
        let largest = SECTIONS
            .iter()
            .filter_map(|&tab| self.tab_bytes(tab))
            .max()
            .unwrap_or(0);
        let shortcuts = &self.config.route_shortcuts;
        let hints = shortcuts.enabled && shortcuts.hints;
        let rows = SECTIONS.into_iter().enumerate().map(|(index, tab)| {
            let bytes = self.tab_bytes(tab).unwrap_or(0);
            let (one, many) = tab.noun();
            let found = match self.tab_count(tab) {
                Some(1) => format!("1 {one}"),
                Some(count) => format!("{count} {many}"),
                None => "not searched".to_owned(),
            };
            let row = div()
                .id(("overview-section", index))
                .debug_selector(move || format!("overview-section-{index}"))
                .flex()
                .items_center()
                .gap(unit(space::L))
                .h(unit(size::row_tall()))
                .px(unit(space::M))
                .rounded(theme::radius_control())
                .cursor_pointer()
                .hover(|style| style.bg(theme::hover()))
                .tip(tooltip::term(tab.term()))
                .child(icon::view(
                    tab.icon(),
                    theme::muted(),
                    None,
                    self.zoom.rem_size(),
                ))
                .child(
                    div()
                        .w(unit(col::XXL))
                        .flex_none()
                        .truncate()
                        .font_weight(FontWeight::MEDIUM)
                        .child(tab.label()),
                )
                .child(
                    div()
                        .w(unit(col::L))
                        .flex_none()
                        .truncate()
                        .text_size(unit(font::SMALL))
                        .text_color(theme::faint())
                        .child(found),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h(unit(size::TRACK))
                        .rounded(theme::radius_bar())
                        .bg(theme::track())
                        .when(bytes > 0, |this| {
                            this.child(
                                div()
                                    .h_full()
                                    .w(gpui::relative(theme::share(bytes, largest).max(0.005)))
                                    .rounded(theme::radius_bar())
                                    .bg(theme::bar()),
                            )
                        }),
                )
                .child(
                    ui::figure(theme::bytes(bytes))
                        .w(unit(col::M))
                        .when(bytes == 0, |this| this.text_color(theme::faint())),
                )
                .children(hints.then(|| ui::kbd(self.hint(&Command::GoTo(tab)))));
            self.control(row, cx, move |app, _, cx| app.go_to(tab, cx))
        });
        div()
            .flex()
            .flex_col()
            .p(unit(space::M))
            .rounded(theme::radius_panel())
            .bg(theme::elevated())
            .border_1()
            .border_color(theme::border())
            .children(rows)
    }
}

/// How many of the picked items the overview lists by name.
const WIN_ROWS: usize = 5;

/// One figure of the overview hero, set off from its neighbour by a rule.
fn hero_figure(
    label: &'static str,
    value: String,
    dot: Option<gpui::Rgba>,
    term: Term,
) -> Stateful<Div> {
    div()
        .id(label)
        .flex()
        .flex_col()
        .flex_none()
        .gap(unit(space::XS))
        .pl(unit(space::XL))
        .border_l_1()
        .border_color(theme::border())
        .tip(tooltip::term(term))
        .child(
            div()
                .flex()
                .items_center()
                .gap(unit(space::S))
                .text_size(unit(font::SMALL))
                .text_color(theme::muted())
                .children(dot.map(|fill| {
                    div()
                        .size(unit(size::DOT))
                        .flex_none()
                        .rounded(theme::radius_pill())
                        .bg(fill)
                }))
                .child(label),
        )
        .child(ui::tabular(
            div()
                .text_size(unit(font::HEADING))
                .font_weight(FontWeight::SEMIBOLD)
                .child(value),
        ))
}

pub(super) fn overview_hero(report: &Report, figures: &ReportStats) -> Div {
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
    let figures_row =
        div()
            .flex()
            .flex_wrap()
            .items_end()
            .gap_x(unit(space::XXL))
            .gap_y(unit(space::L))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(unit(size::LABEL))
                    .gap(unit(space::XXS))
                    .child(
                        div()
                            .text_size(unit(font::SMALL))
                            .text_color(theme::muted())
                            .child("Could be reclaimed"),
                    )
                    .child(ui::tabular(
                        div()
                            .text_size(unit(font::DISPLAY))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(theme::bytes(reclaimable)),
                    ))
                    .children(disk.map(|(_, size)| {
                        div()
                            .text_size(unit(font::SMALL))
                            .text_color(theme::faint())
                            .child(format!("of a {} disk", theme::bytes(size)))
                    })),
            )
            .child(hero_figure(
                "Safe to remove",
                theme::bytes(safe),
                Some(theme::ok()),
                Term::Safe,
            ))
            .child(hero_figure(
                "Needs review",
                theme::bytes(review),
                Some(theme::review()),
                Term::Review,
            ))
            .children(disk.map(|(free, _)| {
                hero_figure("Disk free", theme::bytes(free), None, Term::DiskFree)
            }));
    div()
        .flex()
        .flex_col()
        .gap(unit(space::XL))
        .p(unit(space::XXL))
        .rounded(theme::radius_panel())
        .bg(theme::elevated())
        .border_1()
        .border_color(theme::border())
        .child(figures_row)
        .child(chart::composition("disk", &segments, whole))
}

pub(super) fn overview_trends(figures: &ReportStats, charts: &Charts) -> Div {
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
