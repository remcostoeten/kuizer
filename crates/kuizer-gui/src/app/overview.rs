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
}

pub(super) fn overview_disk(report: &Report, figures: &ReportStats) -> Div {
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
