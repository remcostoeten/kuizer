use super::*;

impl Kuizer {
    pub(super) fn render_titlebar(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
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

    pub(super) fn titlebar_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        self.moving = frame::titlebar_down(event.click_count, window);
    }

    pub(super) fn titlebar_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.moving = false;
    }

    pub(super) fn titlebar_move(
        &mut self,
        _: &MouseMoveEvent,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        if self.moving {
            self.moving = false;
            window.start_window_move();
        }
    }

    pub(super) fn tab_count(&self, tab: Tab) -> Option<usize> {
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
    pub(super) fn tab_bytes(&self, tab: Tab) -> Option<u64> {
        let report = self.report()?;
        let bytes = match tab {
            Tab::Overview | Tab::History | Tab::Issues => return None,
            Tab::Artifacts => self.report_stats.artifacts.all.bytes,
            Tab::Caches => self.report_stats.caches.all.bytes,
            Tab::Toolchains => self.report_stats.toolchains.all.bytes,
            Tab::Docker => self.report_stats.docker.all.bytes,
            Tab::Repos => self.reclaimable(report).repos,
            Tab::Duplicates => self.reclaimable(report).duplicates,
        };
        Some(bytes)
    }

    pub(super) fn reclaimable(&self, report: &Report) -> Reclaimable {
        if let Some((generation, figures)) = self.reclaimable.get()
            && generation == self.report_generation
        {
            return figures;
        }
        let figures = Reclaimable {
            repos: report
                .repos
                .iter()
                .filter(|repository| repository.collectable(self.config.git.min_gain.0))
                .map(|repository| repository.objects.gain())
                .sum(),
            duplicates: report
                .duplicates
                .as_ref()
                .map_or(0, kuizer::dupes::Duplicates::reclaimable),
        };
        self.reclaimable
            .set(Some((self.report_generation, figures)));
        figures
    }

    /// What the sidebar shows to the right of a section: the space it can free, or for history
    /// and issues how many entries there are.
    pub(super) fn tab_figure(&self, tab: Tab) -> Option<String> {
        match self.tab_bytes(tab) {
            Some(bytes) if bytes > 0 => Some(theme::bytes(bytes)),
            _ => self
                .tab_count(tab)
                .filter(|&count| count > 0)
                .map(|count| count.to_string()),
        }
    }

    pub(super) fn render_sidebar(&self, cx: &mut Context<Self>) -> Stateful<Div> {
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
                .keys(&[hint(&Command::Sidebar)])
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

    pub(super) fn hover_icon(&mut self, icon: Icon, hovered: bool, cx: &mut Context<Self>) {
        if hovered && !motion::reduced() {
            *self.icon_hovers.entry(icon.name()).or_default() += 1;
            self.redraw_chrome(cx);
        }
    }

    pub(super) fn icon_epoch(&self, icon: Icon) -> Option<u64> {
        self.icon_hovers.get(icon.name()).copied()
    }

    pub(super) fn render_tab(&self, tab: Tab, cx: &mut Context<Self>) -> Stateful<Div> {
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

    pub(super) fn render_statusbar(&self, cx: &mut Context<Self>) -> Div {
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
}
