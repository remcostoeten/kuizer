use super::*;

impl Kuizer {
    pub(super) fn render_titlebar(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let start = matches!(self.session.phase, Phase::Start);
        let roots = self
            .session
            .roots
            .iter()
            .map(|root| display_path(root))
            .collect::<Vec<_>>()
            .join(", ");
        let busy = self.working();
        let refreshing = self.refreshing();
        let scanning = matches!(self.session.phase, Phase::Loading(_)) || refreshing;
        let finished =
            matches!(self.session.phase, Phase::Ready(_) | Phase::Failed(_)) && !refreshing;
        let stale = self.session.stale.map(|saved| {
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
                        .tip(Tooltip::new("Stop the fresh scan").keys(&[&self.key_hint(Scope::GuiBack, Key::Back)]).build()),
                )
        });
        let controls = frame::controls(window);
        let custom = controls.is_some();
        let small = |button: Stateful<Div>| {
            button
                .h(unit(size::SEGMENT))
                .px(unit(space::M))
                .text_size(unit(font::SMALL))
        };
        let actions = div()
            .debug_selector(|| "titlebar-actions".to_owned())
            .flex()
            .flex_none()
            .items_center()
            .gap(unit(space::XS))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .when(scanning, |this| {
                this.child(
                    small(self.button(
                        "stop-scan",
                        "Stop here",
                        ButtonKind::Ghost,
                        true,
                        cx,
                        |app, _, cx| {
                            app.stop_scan(cx);
                        },
                    ))
                    .tip(
                        Tooltip::new("Stop the scan and keep what it found so far")
                            .keys(&[&self.hint(&Command::StopScan)])
                            .build(),
                    ),
                )
                .child(
                    small(self.button(
                        "cancel-scan",
                        "Cancel scan",
                        ButtonKind::Secondary,
                        true,
                        cx,
                        |app, _, cx| app.cancel_scan(cx),
                    ))
                    .tip(
                        Tooltip::new("Stop the scan and go back to the start screen")
                            .keys(&[&self.key_hint(Scope::GuiBack, Key::Back)])
                            .build(),
                    ),
                )
            })
            .when(finished, |this| {
                this.child(
                    small(self.button(
                        "new-scan",
                        "New scan",
                        ButtonKind::Ghost,
                        !busy,
                        cx,
                        |app, _, cx| {
                            app.new_scan(cx);
                        },
                    ))
                    .tip(
                        Tooltip::new("Choose another folder or change the scan options")
                            .keys(&[&self.hint(&Command::NewScan)])
                            .build(),
                    ),
                )
                .child(
                    small(self.button(
                        "rescan",
                        "Rescan",
                        ButtonKind::Ghost,
                        !busy,
                        cx,
                        |app, _, cx| {
                            if !app.working() {
                                app.rescan(cx);
                            }
                        },
                    ))
                    .pl(unit(space::S))
                    .child(ui::kbd(self.hint(&Command::Rescan)))
                    .tip(
                        Tooltip::new("Scan the same folders again with the same options")
                            .keys(&[&self.hint(&Command::Rescan)])
                            .build(),
                    ),
                )
            });
        let toggle_here = !start && frame::inset(window).is_some();
        let place = if start {
            div()
                .id("roots")
                .min_w_0()
                .truncate()
                .text_size(unit(font::SMALL))
                .text_color(theme::faint())
                .child("Choose what to scan")
        } else {
            theme::mono(div())
                .id("roots")
                .flex_shrink_0()
                .max_w(unit(size::LABEL))
                .truncate()
                .px(unit(space::S))
                .py(unit(space::XXS))
                .rounded(theme::radius_tag())
                .bg(theme::kbd())
                .text_size(unit(font::SMALL))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme::muted())
                .tip(tooltip::plain(roots.clone()))
                .child(roots)
        };
        let left = div()
            .flex()
            .flex_1()
            .h_full()
            .items_center()
            .gap(unit(space::L))
            .when(toggle_here, |this| this.child(self.sidebar_toggle(cx)))
            .when(start, |this| this.child(self.brand()))
            .child(
                // Absolute, so the path never sets how narrow this side can get.
                div().relative().flex_1().h_full().child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .gap(unit(space::L))
                        .overflow_hidden()
                        .child(place)
                        .children(self.scan_state()),
                ),
            );
        let right = div()
            .flex()
            .flex_1()
            .items_center()
            .justify_end()
            .gap(unit(space::L))
            .children(stale)
            .child(actions)
            .children(controls);
        div()
            .id("titlebar")
            .debug_selector(|| "titlebar".to_owned())
            .flex()
            .flex_none()
            .items_center()
            .gap(unit(space::L))
            .h(unit(size::BAR))
            .min_h(frame::TITLEBAR)
            .pl(unit(space::L))
            .when(start, |this| {
                this.when_some(frame::inset(window), |this, inset| this.pl(inset))
            })
            .pr(unit(if custom { space::S } else { space::L }))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::titlebar_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::titlebar_up))
            .on_mouse_move(cx.listener(Self::titlebar_move))
            .when(custom, |this| {
                this.on_mouse_down(MouseButton::Right, |event, window, _| {
                    window.show_window_menu(event.position);
                })
            })
            .child(left)
            .child(self.render_search(cx))
            .child(right)
    }

    pub(super) fn brand(&self) -> Div {
        div()
            .debug_selector(|| "brand".to_owned())
            .flex()
            .flex_none()
            .items_center()
            .gap(unit(space::M))
            .font_weight(FontWeight::SEMIBOLD)
            .child(logo())
            .child("kuizer")
    }

    fn render_sidebar_head(&self, window: &Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let collapsed = self.chrome.collapsed;
        let toggle_here = frame::inset(window).is_none();
        div()
            .id("sidebar-head")
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .h(unit(size::BAR))
            .min_h(frame::TITLEBAR)
            .when(collapsed, |this| this.justify_center())
            .when_some(frame::inset(window), |this, inset| this.pl(inset))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::titlebar_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::titlebar_up))
            .on_mouse_move(cx.listener(Self::titlebar_move))
            .when(!collapsed, |this| {
                this.child(div().pl(unit(space::M)).child(self.brand()))
            })
            .when(toggle_here, |this| this.child(self.sidebar_toggle(cx)))
    }

    /// Where the scan stands, as a dot and a few words after the root path in the titlebar.
    fn scan_state(&self) -> Option<Stateful<Div>> {
        let (dot, text, tip) = match &self.session.phase {
            Phase::Start => return None,
            Phase::Loading(_) => (theme::bar(), "Scanning".to_owned(), None),
            Phase::Failed(_) => (theme::danger(), "Scan failed".to_owned(), None),
            Phase::Ready(_) if self.refreshing() => (theme::bar(), "Scanning".to_owned(), None),
            Phase::Ready(report) if report.summary.cancelled => (
                theme::review(),
                "Partial scan".to_owned(),
                Some(
                    "The scan was stopped early, so some folders and sections are missing. Rescan to get everything",
                ),
            ),
            Phase::Ready(report) => {
                let text = match kuizer::units::since(Some(report.scanned)).as_str() {
                    "-" => "Scanned just now".to_owned(),
                    since => format!("Scanned {since}"),
                };
                (theme::ok(), text, None)
            }
        };
        Some(
            div()
                .id("scan-state")
                .flex()
                .min_w_0()
                .items_center()
                .gap(unit(space::S))
                .text_size(unit(font::SMALL))
                .text_color(theme::faint())
                .when_some(tip, |this, tip| this.tip(tooltip::plain(tip)))
                .child(
                    div()
                        .flex_none()
                        .size(unit(size::DOT))
                        .rounded(theme::radius_pill())
                        .bg(dot),
                )
                .child(ui::tabular(div().min_w_0().truncate().child(text))),
        )
    }

    pub(super) fn titlebar_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        self.chrome.moving = frame::titlebar_down(event.click_count, window);
    }

    pub(super) fn titlebar_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.chrome.moving = false;
    }

    pub(super) fn titlebar_move(
        &mut self,
        _: &MouseMoveEvent,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        if self.chrome.moving {
            self.chrome.moving = false;
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
            Tab::Artifacts => self.session.report_stats.artifacts.all.bytes,
            Tab::Caches => self.session.report_stats.caches.all.bytes,
            Tab::Toolchains => self.session.report_stats.toolchains.all.bytes,
            Tab::Docker => self.session.report_stats.docker.all.bytes,
            Tab::Repos => self.reclaimable(report).repos,
            Tab::Duplicates => self.reclaimable(report).duplicates,
        };
        Some(bytes)
    }

    pub(super) fn reclaimable(&self, report: &Report) -> Reclaimable {
        if let Some((generation, figures)) = self.session.reclaimable.get()
            && generation == self.session.report_generation
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
        self.session
            .reclaimable
            .set(Some((self.session.report_generation, figures)));
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

    /// The button at the start of the titlebar that collapses the sidebar to icons and back.
    pub(super) fn sidebar_toggle(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let collapsed = self.chrome.collapsed;
        let toggle = div()
            .id("sidebar-toggle")
            .debug_selector(|| "sidebar-toggle".to_owned())
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(unit(size::SEGMENT))
            .rounded(theme::radius_control())
            .cursor_pointer()
            .hover(|style| style.bg(theme::hover()))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .tip(
                Tooltip::new(if collapsed {
                    "Show section names"
                } else {
                    "Show only icons"
                })
                .keys(&[&self.hint(&Command::Sidebar)])
                .build(),
            )
            .child(icon::view(Icon::Sidebar, theme::muted()));
        self.control(toggle, cx, |app, _, cx| {
            app.chrome.collapsed = !app.chrome.collapsed;
            cx.notify();
        })
    }

    /// The total the scan can free at the top of the sidebar, with a strip that splits it by
    /// section and marks the open one.
    fn render_reclaim(&self) -> Option<Div> {
        let report = self.report()?;
        let stats = &self.session.report_stats;
        let total = stats.space();
        let parts = [
            (Tab::Artifacts, stats.artifacts.all.bytes),
            (Tab::Caches, stats.caches.all.bytes),
            (Tab::Docker, stats.docker.all.bytes),
            (Tab::Toolchains, stats.toolchains.all.bytes),
        ];
        let split = div()
            .flex()
            .gap(unit(space::XXS))
            .h(unit(size::TRACK))
            .children(
                parts
                    .into_iter()
                    .filter(|(_, bytes)| *bytes > 0)
                    .map(|(tab, bytes)| {
                        div()
                            .h_full()
                            .w(relative(theme::share(bytes, total)))
                            .min_w(unit(size::MARKER))
                            .rounded(theme::radius_bar())
                            .bg(if tab == self.list.tab {
                                theme::bar()
                            } else {
                                theme::bar_rest()
                            })
                    }),
            );
        let disk = report
            .disks
            .first()
            .and_then(|disk| Some((disk.available?, disk.total?)));
        Some(
            div()
                .flex()
                .flex_col()
                .flex_none()
                .px(unit(space::M))
                .pt(unit(space::XS))
                .pb(unit(space::L))
                .child(ui::caption("Reclaimable"))
                .child(
                    ui::amount(total, font::FIGURE)
                        .pt(unit(space::XS))
                        .pb(unit(space::M)),
                )
                .child(split)
                .children(disk.map(|(free, size)| {
                    ui::tabular(div())
                        .pt(unit(space::M))
                        .text_size(unit(font::CAPTION))
                        .text_color(theme::faint())
                        .child(format!(
                            "{} free of {}",
                            theme::bytes(free),
                            theme::bytes(size)
                        ))
                })),
        )
    }

    pub(super) fn render_sidebar(&self, window: &Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let collapsed = self.chrome.collapsed;
        let head = self.render_sidebar_head(window, cx);
        self.chrome
            .stops
            .borrow_mut()
            .within(Some(self.chrome.sidebar_scroll.clone()));
        let mut items: Vec<AnyElement> = Vec::new();
        if !collapsed {
            items.extend(self.render_reclaim().map(IntoElement::into_any_element));
        }
        for tab in Tab::ALL {
            let heading = tab.heading().map(|heading| {
                if collapsed {
                    ui::divider().mx(unit(space::M)).into_any_element()
                } else {
                    ui::caption(heading)
                        .flex_none()
                        .px(unit(space::M))
                        .pt(unit(space::L))
                        .pb(unit(space::S))
                        .into_any_element()
                }
            });
            items.extend(heading);
            items.push(self.render_tab(tab, cx).into_any_element());
        }
        self.chrome.stops.borrow_mut().within(None);
        let settings = ui::nav_item("sidebar-settings", false)
            .debug_selector(|| "sidebar-settings".to_owned())
            .when(collapsed, |this| this.justify_center().px_0())
            .tip(
                Tooltip::new("Settings")
                    .keys(&[&self.hint(&Command::Settings)])
                    .build(),
            )
            .child(icon::view(Icon::Settings, theme::faint()))
            .when(!collapsed, |this| {
                this.child(div().flex_1().min_w_0().truncate().child("Settings"))
                    .child(ui::kbd(self.hint(&Command::Settings)))
            });
        let settings = self.control(settings, cx, |app, _, cx| app.open_settings(cx));
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
            .when_some(frame::inset(window).filter(|_| collapsed), |this, inset| {
                this.min_w(inset)
            })
            .pb(unit(space::SURFACE))
            .px(unit(space::INSET))
            .child(head)
            .child(
                div()
                    .id("sidebar-items")
                    .track_scroll(&self.chrome.sidebar_scroll)
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(items),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_none()
                    .pt(unit(space::S))
                    .border_t_1()
                    .border_color(theme::border())
                    .child(settings),
            )
    }

    pub(super) fn render_tab(&self, tab: Tab, cx: &mut Context<Self>) -> Stateful<Div> {
        let active = self.list.tab == tab;
        let collapsed = self.chrome.collapsed;
        let figure = self.tab_figure(tab);
        let warn = tab == Tab::Issues && figure.is_some();
        let shortcuts = &self.config.route_shortcuts;
        let hints = shortcuts.enabled && shortcuts.hints;
        let keys: Vec<String> = self
            .keymap
            .keys(Scope::Gui, tab.action())
            .filter(|_| hints)
            .map(commands::label)
            .collect();
        let tip = Tooltip::new(tab.term().text())
            .title(match (collapsed, &figure) {
                (true, Some(figure)) => format!("{}  {figure}", tab.label()),
                _ => tab.label().to_owned(),
            })
            .keys(&keys.iter().map(String::as_str).collect::<Vec<_>>())
            .build();
        let item = ui::nav_item(tab.label(), active)
            .debug_selector(|| tab.label().to_owned())
            .when(collapsed, |this| this.justify_center().px_0())
            .tip(tip)
            .child(icon::view(
                tab.icon(),
                if active {
                    theme::focus()
                } else {
                    theme::faint()
                },
            ))
            .when(!collapsed, |this| {
                this.child(div().flex_1().min_w_0().truncate().child(tab.label()))
                    .children(figure.map(|figure| {
                        ui::figure(figure)
                            .text_size(unit(font::SMALL))
                            .font_weight(FontWeight::NORMAL)
                            .text_color(match (warn, active) {
                                (true, _) => theme::review(),
                                (false, true) => theme::muted(),
                                (false, false) => theme::faint(),
                            })
                    }))
            });
        self.control(item, cx, move |app, _, cx| app.go_to(tab, cx))
    }

    pub(super) fn render_statusbar(&self) -> AnyElement {
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(unit(space::L))
            .h(unit(size::STATUS))
            .pl(unit(space::S))
            .pr(unit(space::GUTTER))
            .border_t_1()
            .border_color(theme::border())
            .text_size(unit(font::SMALL))
            .text_color(theme::faint())
            .child(self.render_mode())
            .child(self.render_hints())
            .children(self.render_position())
            .into_any_element()
    }

    pub(super) fn render_mode(&self) -> Div {
        let mode = self.mode();
        let review = self.list.selection.stats().total().review.count;
        let (fill, color) = match mode {
            keys::Mode::Normal => (theme::hover(), theme::muted()),
            keys::Mode::Visual | keys::Mode::Record => (theme::bar(), theme::on_accent()),
            keys::Mode::Filter => (theme::review_soft(), theme::review()),
            keys::Mode::Confirm => (theme::danger_solid(), theme::on_danger()),
            keys::Mode::Palette
            | keys::Mode::Settings
            | keys::Mode::Menu
            | keys::Mode::Shortcuts => (theme::tile_selected(), theme::text()),
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(unit(space::S))
            .child(
                theme::label(div())
                    .id("mode")
                    .debug_selector(move || format!("mode-{}", mode.label().to_lowercase()))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .h(unit(size::CHIP))
                    .min_w(unit(size::MODE))
                    .px(unit(space::M))
                    .rounded(theme::radius_tag())
                    .bg(fill)
                    .text_color(color)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(mode.label().to_uppercase())
                    .when(mode == keys::Mode::Visual, |this| {
                        this.tip(
                            Tooltip::new("Motions select the rows they pass")
                                .keys(&[
                                    &self.hint(&Command::Visual),
                                    &self.key_hint(Scope::GuiBack, Key::Back),
                                ])
                                .build(),
                        )
                    }),
            )
            .when(!self.list.count.is_empty(), |this| {
                this.child(
                    theme::label(div())
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap(unit(space::S))
                        .h(unit(size::CHIP))
                        .px(unit(space::S))
                        .rounded(theme::radius_tag())
                        .bg(theme::tile_selected())
                        .text_color(theme::text())
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(self.list.count.clone())
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme::focus())
                                .child("count"),
                        ),
                )
            })
            .when_some(self.asking(), |this, left| {
                this.child(
                    ui::tag(
                        "mode-asking",
                        format!(
                            "Only review items. {} again selects {left}",
                            self.hint(&Command::SelectAll)
                        ),
                        ui::Tone::Review,
                    )
                    .debug_selector(|| "mode-asking".to_owned()),
                )
            })
            .when(review > 0, |this| {
                this.child(
                    ui::tag(
                        "mode-review",
                        format!("{review} review selected"),
                        ui::Tone::Review,
                    )
                    .debug_selector(|| "mode-review".to_owned())
                    .tip(tooltip::term(Term::Review)),
                )
            })
    }

    /// The keys that do the most in the current mode, read from the keymap so a rebind shows.
    fn render_hints(&self) -> Div {
        let list = self.report().is_some() && self.list.tab != Tab::Overview;
        let hints: Vec<(String, &'static str)> = match self.mode() {
            keys::Mode::Normal if list => {
                let mut hints = vec![
                    (self.hint(&Command::Down), "move"),
                    (self.hint(&Command::Toggle), "select"),
                    (self.hint(&Command::Visual), "visual"),
                    (self.hint(&Command::FocusFilter), "filter"),
                ];
                if self.list.tab.selects() {
                    hints.push((self.hint(&Command::Trash), "delete"));
                }
                hints.push((self.hint(&Command::Palette), "commands"));
                hints
            }
            keys::Mode::Normal => vec![
                (self.hint(&Command::Palette), "commands"),
                (self.hint(&Command::Shortcuts), "shortcuts"),
            ],
            keys::Mode::Visual => vec![
                (self.hint(&Command::Down), "extend the selection"),
                (self.hint(&Command::Trash), "delete"),
                (self.key_hint(Scope::GuiBack, Key::Back), "back to normal"),
            ],
            keys::Mode::Filter => vec![
                (
                    self.key_hint(Scope::GuiFilter, Key::ApplyFilter),
                    "keep the filter",
                ),
                (self.key_hint(Scope::GuiFilter, Key::Back), "clear it"),
            ],
            keys::Mode::Record => vec![(self.key_hint(Scope::Settings, Key::Back), "cancel")],
            mode => {
                let (scope, action) = match mode {
                    keys::Mode::Palette => (Scope::Palette, Key::Back),
                    keys::Mode::Settings => (Scope::Settings, Key::Back),
                    keys::Mode::Menu => (Scope::Menu, Key::MenuClose),
                    keys::Mode::Shortcuts => (Scope::Shortcuts, Key::ShortcutsClose),
                    keys::Mode::Confirm => (Scope::Dialog, Key::Back),
                    _ => (Scope::GuiBack, Key::Back),
                };
                vec![(self.key_hint(scope, action), "close")]
            }
        };
        div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap(unit(space::L))
            .overflow_hidden()
            .children(
                hints
                    .into_iter()
                    .filter(|(key, _)| !key.is_empty())
                    .map(|(key, label)| {
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .gap(unit(space::XS))
                            .child(ui::kbd(key))
                            .child(label)
                    }),
            )
    }

    /// Where the cursor is in the artifact list, as row out of rows.
    fn render_position(&self) -> Option<Div> {
        if self.list.tab != Tab::Artifacts || self.list.rows.is_empty() {
            return None;
        }
        let cursor = self.cursor_target()?;
        let row = self.listed_row(None, &cursor, self.row_hint(Tab::Artifacts))?;
        Some(
            ui::tabular(theme::mono(div()))
                .flex_none()
                .text_color(theme::muted())
                .child(format!("{}/{}", row + 1, self.list.rows.len())),
        )
    }
}

/// The kuizer mark: three bars of falling length on the accent.
fn logo() -> Div {
    let line = |width: f32| {
        div()
            .w(unit(width))
            .h(unit(size::MARKER))
            .rounded(theme::radius_dot())
            .bg(theme::on_accent())
    };
    div()
        .flex()
        .flex_col()
        .flex_none()
        .justify_center()
        .gap(unit(size::HAIR))
        .size(unit(size::CHECK))
        .pl(unit(space::XS))
        .rounded(theme::radius_tag())
        .bg(theme::bar())
        .child(line(size::SWATCH))
        .child(line(size::DOT))
        .child(line(size::TRACK))
}
