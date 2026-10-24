use super::*;

impl Kuizer {
    pub(super) fn render_start(&self, cx: &mut Context<Self>) -> AnyElement {
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
            .h(unit(size::INPUT))
            .px(unit(space::L))
            .rounded(theme::radius_control())
            .bg(theme::background())
            .border_1()
            .border_color(if self.folder_error.is_some() {
                theme::danger()
            } else if self.whole_home {
                theme::border_strong()
            } else {
                theme::focus()
            })
            .cursor(CursorStyle::IBeam)
            .tip(
                Tooltip::details("Keyboard")
                    .row("Enter", "start the scan")
                    .row("Ctrl+V", "paste a path")
                    .row("Ctrl+Backspace", "remove the last folder")
                    .row("Ctrl+U", "clear the field")
                    .build(),
            )
            .when(field_text.is_empty(), |this| {
                this.child(
                    div()
                        .w(unit(size::HAIR))
                        .h(unit(size::CARET))
                        .bg(theme::text()),
                )
                .child(
                    div()
                        .pl(unit(space::XXS))
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
                    this.child(
                        div()
                            .flex_none()
                            .w(unit(size::HAIR))
                            .h(unit(size::CARET))
                            .bg(theme::text()),
                    )
                })
            })
            .on_click(cx.listener(|app, _: &ClickEvent, window, cx| {
                window.focus(&app.focus);
                app.whole_home = false;
                cx.notify();
            }));
        let scope = ui::segments()
            .child(
                ui::segment("scope-folder", "One folder", !self.whole_home)
                    .tip(tooltip::plain("Scan one folder and everything below it"))
                    .on_click(cx.listener(
                    |app, _: &ClickEvent, _, cx| {
                        app.whole_home = false;
                        cx.notify();
                    },
                )),
            )
            .child(
                ui::segment("scope-home", "Whole home folder", self.whole_home)
                    .tip(tooltip::plain(
                        "Scan everything under your home folder. This takes longer than one project folder",
                    ))
                    .on_click(
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
                    .gap(unit(space::M))
                    .child(field)
                    .child(
                        ui::button("browse", "Choose folder", ButtonKind::Secondary, true).tip(tooltip::plain("Pick a folder with the system dialog"))
                            .on_click(cx.listener(|app, _: &ClickEvent, _, cx| {
                                app.choose_folder(cx);
                            })),
                    )
                    .child(
                        ui::button("start-scan", "Start scan", ButtonKind::Primary, true).tip(Tooltip::new("Scan the chosen folder").keys(&["Enter"]).build())
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
                    .text_size(unit(font::SMALL))
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
                    let missing = self.recent.is_gone(&path);
                    let full = tooltip::path(&path);
                    div()
                        .id(("recent", index))
                        .flex()
                        .items_center()
                        .gap(unit(space::M))
                        .h(unit(size::CONTROL))
                        .px(unit(space::M))
                        .rounded(theme::radius_control())
                        .cursor_pointer()
                        .hover(|style| style.bg(theme::hover()))
                        .child(
                            div()
                                .id(("recent-path", index))
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .tip(full)
                                .text_color(if missing {
                                    theme::faint()
                                } else {
                                    theme::text()
                                })
                                .child(label),
                        )
                        .child(
                            div()
                                .text_size(unit(font::SMALL))
                                .text_color(theme::faint())
                                .child(if missing {
                                    "missing"
                                } else {
                                    "double-click to scan"
                                }),
                        )
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
                .fold(div().flex().gap(unit(space::XXS)), |row, category| {
                    let on = options.categories.contains(&category);
                    row.child(
                        div()
                            .id(("category", category as usize))
                            .flex()
                            .items_center()
                            .gap(unit(space::S))
                            .px(unit(space::M))
                            .h(unit(size::SEGMENT))
                            .rounded(theme::radius_control())
                            .cursor_pointer()
                            .text_color(if on { theme::text() } else { theme::faint() })
                            .hover(|style| style.bg(theme::hover()))
                            .tip(tooltip::term(Term::category(category)))
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
                .gap(unit(space::S))
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
                    "Toolchains",
                    "List rustup, Node, Python, Java and Go versions and the projects that pin them",
                )
                .child(toggle("opt-toolchains", options.toolchains, |options| {
                    options.toolchains = !options.toolchains;
                })),
            )
            .child(
                ui::option_row(
                    "Docker and Podman",
                    "List dangling images, stopped containers, build cache and unused volumes",
                )
                .child(toggle("opt-docker", options.docker, |options| {
                    options.docker = !options.docker;
                })),
            )
            .child(
                ui::option_row(
                    "Git repositories",
                    "Find loose git objects to pack and branches that are already merged",
                )
                .child(toggle("opt-repos", options.repos, |options| {
                    options.repos = !options.repos;
                })),
            )
            .child(
                ui::option_row(
                    "Duplicate files",
                    "After the scan, hash files to find identical copies. Reads every file, so it is slower",
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
            .p(unit(space::XXL))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(unit(space::XL))
                    .w_full()
                    .max_w(unit(size::PAGE))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(unit(space::XS))
                            .child(
                                div()
                                    .text_size(unit(font::HEADING))
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

    pub(super) fn start_scan(&mut self, cx: &mut Context<Self>) {
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
        let checked = cx.background_spawn(async move { start::check_folder(&path) });
        self.folder_check = Some(cx.spawn(async move |this, cx| {
            let result = checked.await;
            this.update(cx, |app, cx| match result {
                Ok(root) => {
                    app.folder_error = None;
                    app.roots = scan::normalize_roots(&[root]);
                    app.recent.remember(&app.roots);
                    app.rescan(cx);
                }
                Err(error) => {
                    app.folder_error = Some(error);
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    pub(super) fn cancel_scan(&mut self, cx: &mut Context<Self>) {
        if let Some(cancel) = self.cancel.take() {
            cancel.cancel();
        }
        self.task = None;
        self.scan_over();
        self.set_phase(Phase::Start, cx);
        cx.notify();
    }

    pub(super) fn new_scan(&mut self, cx: &mut Context<Self>) {
        if self.busy() || self.refreshing() {
            return;
        }
        self.selection.clear();
        self.rows.clear();
        self.toast = None;
        self.set_phase(Phase::Start, cx);
        cx.notify();
    }

    pub(super) fn choose_folder(&mut self, cx: &mut Context<Self>) {
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

    pub(super) fn pick_recent(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.folder = display_path(path);
        self.whole_home = false;
        self.folder_error = None;
        cx.notify();
    }

    pub(super) fn type_folder(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.key == "enter" {
            self.start_scan(cx);
            return;
        }
        let paste = || cx.read_from_clipboard().and_then(|item| item.text());
        if !field::edit(&mut self.folder, keystroke, field::PATH, paste) {
            return;
        }
        self.whole_home = false;
        self.folder_error = None;
        cx.notify();
    }

    pub(super) fn render_loading(&self, activity: &Activity) -> AnyElement {
        let status = self.tab.status(activity, &self.options);
        let header = div()
            .flex()
            .items_end()
            .gap(unit(space::XL))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap(unit(space::XXS))
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .child(self.tab.label()),
                    )
                    .child(
                        div()
                            .debug_selector(|| status.to_owned())
                            .text_color(theme::muted())
                            .child(status),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(unit(space::M))
                    .flex_none()
                    .text_size(unit(font::SMALL))
                    .text_color(theme::faint())
                    .child(
                        ui::tabular(div().text_color(theme::muted()))
                            .child(activity::took(self.started.elapsed())),
                    )
                    .child(ui::kbd("Esc"))
                    .child("cancels"),
            );
        let steps = ui::card("Progress").child(
            div().flex().flex_col().children(
                Step::ALL
                    .into_iter()
                    .map(|step| step_row(step, activity, &self.options, &self.roots)),
            ),
        );
        scroll("loading")
            .debug_selector(|| "activity".to_owned())
            .child(header)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_start()
                    .gap(unit(space::L))
                    .child(steps.flex_1().min_w(unit(size::LIST)))
                    .child(
                        found_card(activity, &self.roots)
                            .flex_1()
                            .min_w(unit(size::LIST)),
                    ),
            )
            .into_any_element()
    }
}

pub(super) fn step_row(
    step: Step,
    activity: &Activity,
    options: &ScanOptions,
    roots: &[PathBuf],
) -> Div {
    let state = activity.state(step, options);
    let running = match state {
        State::Active(running) => Some(running),
        _ => None,
    };
    let active = running.is_some();
    let (detail, took) = match &state {
        State::Off => ("Off".to_owned(), String::new()),
        State::Pending => (String::new(), String::new()),
        State::Active(running) => (activity.detail(running), String::new()),
        State::Done(done) => (done.result.clone(), activity::took(done.took)),
    };
    let quiet = matches!(state, State::Off | State::Pending);
    let line = div()
        .flex()
        .items_center()
        .gap(unit(space::L))
        .h(unit(size::ROW))
        .child(step_dot(step, &state))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(if quiet { theme::faint() } else { theme::text() })
                .when(active, |label| label.font_weight(FontWeight::MEDIUM))
                .child(step.label()),
        )
        .child(
            ui::tabular(div())
                .flex_none()
                .text_size(unit(font::SMALL))
                .text_color(if active {
                    theme::text()
                } else {
                    theme::muted()
                })
                .when(quiet, |detail| detail.text_color(theme::faint()))
                .child(detail),
        )
        .child(
            ui::tabular(column(col::XS))
                .flex()
                .justify_end()
                .text_size(unit(font::SMALL))
                .text_color(theme::faint())
                .child(took),
        );
    let mut row = div()
        .debug_selector(move || format!("step-{step:?}"))
        .flex()
        .flex_col()
        .child(line);
    if let Some(running) = running {
        let indent = unit(size::DOT + space::L);
        if let Some((done, total)) = running.count
            && total > 0
        {
            row = row.child(
                div().pl(indent).pb(unit(space::S)).child(
                    div()
                        .w_full()
                        .h(unit(size::TRACK))
                        .bg(theme::track())
                        .child(
                            div()
                                .h_full()
                                .w(gpui::relative(done as f32 / total as f32))
                                .bg(theme::bar()),
                        ),
                ),
            );
        }
        if let Some(current) = &running.current {
            row = row.child(
                div()
                    .debug_selector(|| "current".to_owned())
                    .pl(indent)
                    .pb(unit(space::S))
                    .truncate()
                    .text_size(unit(font::SMALL))
                    .text_color(theme::faint())
                    .child(model::short_path(current, roots)),
            );
        }
    }
    row
}

pub(super) fn step_dot(step: Step, state: &State) -> AnyElement {
    let dot = div()
        .flex_none()
        .size(unit(size::DOT))
        .rounded(theme::radius_dot());
    match state {
        State::Done(_) => dot.bg(theme::faint()).into_any_element(),
        State::Active(_) => chart::breathe(
            ElementId::Name(format!("step-dot-{step:?}").into()),
            dot.bg(theme::primary()),
        ),
        State::Pending => dot
            .border_1()
            .border_color(theme::border_strong())
            .into_any_element(),
        State::Off => dot.bg(theme::border()).into_any_element(),
    }
}

pub(super) fn found_card(activity: &Activity, roots: &[PathBuf]) -> Div {
    let total = div()
        .flex()
        .items_center()
        .justify_between()
        .child(ui::heading("Largest found so far"))
        .child(
            ui::tabular(div())
                .debug_selector(|| "found-total".to_owned())
                .text_size(unit(font::SMALL))
                .text_color(theme::muted())
                .child(format!(
                    "{}, {}",
                    plural(activity.found as usize, "item"),
                    theme::bytes(activity.bytes)
                )),
        );
    let rows: Vec<Div> = activity
        .largest
        .iter()
        .map(|found| {
            div()
                .flex()
                .items_center()
                .gap(unit(space::L))
                .h(unit(size::ROW))
                .border_t_1()
                .border_color(theme::border())
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(model::short_path(&found.path, roots)),
                )
                .child(
                    div()
                        .w(unit(col::XL))
                        .flex_none()
                        .truncate()
                        .text_size(unit(font::SMALL))
                        .text_color(theme::muted())
                        .child(found.name.clone()),
                )
                .child(
                    ui::tabular(column(col::S))
                        .flex()
                        .justify_end()
                        .child(theme::bytes(found.bytes)),
                )
        })
        .collect();
    let card = div()
        .flex()
        .flex_col()
        .min_w_0()
        .gap(unit(space::L))
        .p(unit(space::GUTTER))
        .rounded(theme::radius_panel())
        .bg(theme::panel())
        .border_1()
        .border_color(theme::border())
        .child(total);
    if rows.is_empty() {
        card.child(
            div()
                .text_size(unit(font::SMALL))
                .text_color(theme::faint())
                .child("Nothing found yet"),
        )
    } else {
        card.child(div().flex().flex_col().children(rows))
    }
}
