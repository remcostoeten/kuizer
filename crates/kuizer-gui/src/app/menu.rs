use gpui::{Animation, AnimationExt};

use super::*;

pub(super) struct Leaving {
    path: PathBuf,
    pattern: String,
    at: Instant,
}

impl Choice {
    fn binding(self) -> Key {
        match self {
            Self::Reveal => Key::MenuReveal,
            Self::CopyPath => Key::MenuCopyPath,
            Self::Exclude => Key::MenuExclude,
            Self::Toggle => Key::MenuToggle,
            Self::Trash => Key::MenuDelete,
            Self::Restore => Key::MenuRestore,
            Self::CopyCommand => Key::MenuCopyCommand,
            Self::Gc => Key::MenuGc,
            Self::Branches => Key::MenuBranches,
        }
    }
}

impl Kuizer {
    pub(super) fn menu_items(&self, target: &Target) -> Vec<MenuItem> {
        let reveal = MenuItem::new(Choice::Reveal, "Reveal in file manager")
            .tip("Open the folder that holds this item");
        let copy = MenuItem::new(Choice::CopyPath, "Copy path");
        let exclude = MenuItem::new(Choice::Exclude, "Always exclude")
            .tip(EXCLUDE_TIP)
            .divided();
        match target {
            Target::Item(source, id, _) => {
                let selected = self.selection.contains(&(*source, *id));
                let external = *source == Source::Docker;
                let remove = MenuItem::new(
                    Choice::Trash,
                    if source.trashed(self.config.delete.mode) {
                        "Move to trash"
                    } else {
                        "Delete"
                    },
                )
                .tip("Remove only this item, right away");
                vec![
                    reveal.unless(external.then_some(NOT_ON_DISK)),
                    copy,
                    MenuItem::new(Choice::Toggle, if selected { "Deselect" } else { "Select" })
                        .divided()
                        .unless(self.restoring().then_some(WAIT_FOR_RESTORE)),
                    remove.unless(self.locked([*source].into_iter())),
                    exclude.unless(external.then_some(NOT_ON_DISK)),
                ]
            }
            Target::Duplicate(_) | Target::Issue(_) => vec![reveal, copy, exclude],
            Target::Repo(root) => {
                let scope = GitScope::One(root.clone());
                vec![
                    reveal.tip("Open the folder that holds this repository"),
                    copy,
                    MenuItem::new(Choice::Gc, "Run git gc")
                        .tip("Pack loose objects now")
                        .divided()
                        .unless(self.gc_blocked(Some(&scope))),
                    MenuItem::new(Choice::Branches, "Delete merged branches")
                        .tip("Delete the merged local branches, after a confirmation")
                        .unless(self.branches_blocked(Some(&scope))),
                    exclude,
                ]
            }
            Target::Run(session, _) => {
                let restorable = self.report().is_some_and(|report| {
                    report.history.as_ref().is_ok_and(|view| {
                        view.history
                            .runs
                            .iter()
                            .position(|run| run.id == *session)
                            .is_some_and(|index| {
                                matches!(view.trash_state(index), TrashState::Restorable { .. })
                            })
                    })
                });
                let lock = if !restorable {
                    Some("Nothing from this cleanup is left in the trash")
                } else if self.restoring() {
                    Some(WAIT_FOR_RESTORE)
                } else if !self.can_restore() {
                    Some(WAIT_FOR_REFRESH)
                } else {
                    None
                };
                vec![
                    MenuItem::new(Choice::Restore, "Restore")
                        .tip("Put what is still in the trash back where it was")
                        .unless(lock),
                    MenuItem::new(Choice::CopyCommand, "Copy command"),
                ]
            }
        }
    }

    pub(super) fn open_menu(
        &mut self,
        target: Target,
        position: Point<Pixels>,
        keyboard: bool,
        cx: &mut Context<Self>,
    ) {
        if self.dialog.is_some() || !self.known(&target) {
            return;
        }
        let active = if keyboard {
            self.menu_items(&target)
                .iter()
                .position(|item| item.enabled)
        } else {
            None
        };
        self.cursors.insert(target.tab(), target.clone());
        self.menu = Some(Menu {
            target,
            position,
            active,
        });
        cx.notify();
    }

    pub(super) fn open_menu_at_cursor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.cursor_target() else {
            return;
        };
        let indent = unit(space::XXL).to_pixels(window.rem_size());
        let position = self.cursor_bounds.get().map_or_else(
            || {
                let size = window.viewport_size();
                point(size.width / 2., size.height / 3.)
            },
            |bounds| point(bounds.left() + indent, bounds.bottom()),
        );
        self.open_menu(target, position, true, cx);
    }

    pub(super) fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.menu.take().is_some() {
            cx.notify();
        }
    }

    pub(super) fn menu_key(&mut self, event: &KeyDownEvent, repeat: bool, cx: &mut Context<Self>) {
        let Some(menu) = &self.menu else {
            return;
        };
        let items = self.menu_items(&menu.target);
        let enabled: Vec<usize> = items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.enabled)
            .map(|(index, _)| index)
            .collect();
        let at = menu
            .active
            .and_then(|active| enabled.iter().position(|&index| index == active));
        let count = enabled.len();
        let stroke = commands::stroke(&event.keystroke);
        let next = match self.keymap.lookup(Scope::Menu, stroke) {
            Some(Key::MenuClose) => {
                self.close_menu(cx);
                return;
            }
            Some(Key::MenuDown) if count > 0 => Some(enabled[at.map_or(0, |at| (at + 1) % count)]),
            Some(Key::MenuUp) if count > 0 => {
                Some(enabled[at.map_or(count - 1, |at| (at + count - 1) % count)])
            }
            Some(Key::MenuFirst) => enabled.first().copied(),
            Some(Key::MenuLast) => enabled.last().copied(),
            Some(Key::MenuRun) => {
                if let Some(active) = menu.active
                    && !repeat
                {
                    self.choose(active, cx);
                }
                return;
            }
            _ => {
                if !repeat
                    && let Some(index) = items.iter().position(|item| {
                        item.enabled
                            && self
                                .keymap
                                .matches(Scope::Menu, item.choice.binding(), stroke)
                    })
                {
                    self.choose(index, cx);
                }
                return;
            }
        };
        if let Some(menu) = &mut self.menu {
            menu.active = next;
        }
        self.redraw_chrome(cx);
    }

    pub(super) fn choose(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(menu) = self.menu.take() else {
            return;
        };
        let choice = self
            .menu_items(&menu.target)
            .get(index)
            .filter(|item| item.enabled)
            .map(|item| item.choice);
        let Some(choice) = choice else {
            self.menu = Some(menu);
            return;
        };
        cx.notify();
        self.run_choice(choice, menu.target, cx);
    }

    pub(super) fn menu_unavailable(&self, target: &Target, choice: Choice) -> Option<&'static str> {
        match self
            .menu_items(target)
            .into_iter()
            .find(|item| item.choice == choice)
        {
            Some(item) if item.enabled => None,
            Some(item) => item.tip,
            _ => Some(match choice {
                Choice::Restore | Choice::CopyCommand => "Focus a cleanup in History",
                _ => "Focus a file or folder first",
            }),
        }
    }

    pub(super) fn run_choice(&mut self, choice: Choice, target: Target, cx: &mut Context<Self>) {
        let path = target.path().map(Path::to_path_buf);
        match (choice, target) {
            (Choice::Reveal, _) => {
                if let Some(path) = path {
                    self.reveal_path(path, cx);
                }
            }
            (Choice::CopyPath, _) => {
                if let Some(path) = path {
                    cx.write_to_clipboard(ClipboardItem::new_string(
                        path.to_string_lossy().into_owned(),
                    ));
                }
            }
            (Choice::Exclude, _) => {
                if let Some(path) = path {
                    self.ask_exclude(path, cx);
                }
            }
            (Choice::Toggle, Target::Item(source, id, _)) => self.toggle(source, id, cx),
            (Choice::Trash, Target::Item(source, id, _)) => {
                self.delete(Some((source, id)), cx);
            }
            (Choice::Restore, Target::Run(session, _)) => {
                self.restore(session, cx);
            }
            (Choice::CopyCommand, Target::Run(_, command)) => {
                cx.write_to_clipboard(ClipboardItem::new_string(command));
            }
            (Choice::Gc, Target::Repo(root)) => self.collect_garbage(&GitScope::One(root), cx),
            (Choice::Branches, Target::Repo(root)) => {
                self.ask_branches(GitScope::One(root), cx);
            }
            _ => {}
        }
    }

    /// Opens the file manager on a worker so a slow or missing file manager never blocks the
    /// window, and explains a failure in the message bar.
    pub(super) fn reveal_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let open = self.reveal;
        let opened = cx.background_executor().spawn(async move { open(&path) });
        cx.spawn(async move |this, cx| {
            if let Err(error) = opened.await {
                let _ = this.update(cx, |app, cx| {
                    app.tell(
                        Notice::Failed(format!("Could not open the file manager: {error}")),
                        cx,
                    );
                });
            }
        })
        .detach();
    }

    /// Opens the inline confirmation that shows the pattern to add to `scan.exclude`, with
    /// focus on Cancel so a held key or immediate Enter changes nothing.
    pub(super) fn ask_exclude(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let shown = self.shown_path(&path);
        if self.config_path.is_none() {
            self.tell(
                Notice::Failed(format!(
                    "Could not exclude {shown}: this system has no config folder"
                )),
                cx,
            );
            return;
        }
        if self.excluding.contains(&path) {
            return;
        }
        let pattern = kuizer::config::exclude_pattern(&path);
        self.menu = None;
        self.dialog = Some(Dialog::Exclude {
            path,
            pattern,
            cancel: true,
        });
        cx.notify();
    }

    /// Closes the exclude confirmation without changing the config file.
    pub(super) fn close_exclude(&mut self, cx: &mut Context<Self>) {
        if let Some(Dialog::Exclude { path, pattern, .. }) = self.dialog.take() {
            self.leave_exclude(path, pattern, cx);
        }
        cx.notify();
    }

    fn leave_exclude(&mut self, path: PathBuf, pattern: String, cx: &mut Context<Self>) {
        if motion::reduced() {
            return;
        }
        let at = Instant::now();
        self.exclude_leaving = Some(Leaving { path, pattern, at });
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(motion::STEP).await;
            let _ = this.update(cx, |app, cx| {
                if app
                    .exclude_leaving
                    .as_ref()
                    .is_some_and(|leaving| leaving.at == at)
                {
                    app.exclude_leaving = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn accept_exclude(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::Exclude { path, pattern, .. }) = self.dialog.take() else {
            return;
        };
        self.leave_exclude(path.clone(), pattern, cx);
        self.exclude(path, cx);
    }

    /// The inline confirmation before adding a path to `scan.exclude`, floating over the
    /// bottom of the list like the selection summary.
    pub(super) fn render_exclude_confirm(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (path, pattern, focus) = match (&self.dialog, &self.exclude_leaving) {
            (
                Some(Dialog::Exclude {
                    path,
                    pattern,
                    cancel,
                }),
                _,
            ) => (path, pattern, Some(*cancel)),
            (_, Some(leaving)) => (&leaving.path, &leaving.pattern, None),
            _ => return None,
        };
        let bar = self.exclude_bar(path, pattern, focus, cx);
        if motion::reduced() {
            return Some(bar.into_any_element());
        }
        let open = focus.is_some();
        Some(
            bar.with_animation(
                if open {
                    "exclude-confirm-in"
                } else {
                    "exclude-confirm-out"
                },
                Animation::new(motion::STEP).with_easing(motion::ease_out),
                move |bar, progress| {
                    let shown = if open { progress } else { 1.0 - progress };
                    bar.opacity(shown)
                        .relative()
                        .top(unit(space::M * (1.0 - shown)))
                },
            )
            .into_any_element(),
        )
    }

    fn exclude_bar(
        &self,
        path: &Path,
        pattern: &str,
        focus: Option<bool>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let ring = || ring::outline(None, Rc::new(Cell::new(false)));
        let open = focus.is_some();
        let shown = self.shown_path(path);
        clean::floating(div().id("exclude-confirm"))
            .debug_selector(|| "exclude-confirm".to_owned())
            .flex()
            .flex_col()
            .gap(unit(space::M))
            .w(unit(size::SUMMARY))
            .max_w_full()
            .p(unit(space::L))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .truncate()
                    .child(format!("Always exclude {shown}?")),
            )
            .child(
                div()
                    .text_size(unit(font::SMALL))
                    .text_color(theme::muted())
                    .child(format!(
                        "Adds {pattern} to scan.exclude in the config file so future scans skip it."
                    )),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(unit(space::M))
                    .child(
                        ui::button("exclude-cancel", "Cancel", ButtonKind::Secondary, open)
                            .debug_selector(|| "cancel-exclude".to_owned())
                            .tip(
                                Tooltip::new("Keep scanning this path")
                                    .keys(&[&self.key_hint(Scope::Dialog, Key::Back)])
                                    .build(),
                            )
                            .when(focus == Some(true), |this| this.child(ring()))
                            .when(open, |this| {
                                this.on_click(cx.listener(|app, _: &ClickEvent, _, cx| {
                                    app.close_exclude(cx);
                                }))
                            }),
                    )
                    .child(
                        ui::button(
                            "exclude-accept",
                            "Always exclude",
                            ButtonKind::Primary,
                            open,
                        )
                        .debug_selector(|| "confirm-exclude".to_owned())
                        .tip(
                            Tooltip::new("Add this pattern to scan.exclude")
                                .keys(&[&self.key_hint(Scope::Dialog, Key::Confirm)])
                                .build(),
                        )
                        .when(focus == Some(false), |this| this.child(ring()))
                        .when(open, |this| {
                            this.on_click(cx.listener(|app, _: &ClickEvent, _, cx| {
                                app.accept_exclude(cx);
                            }))
                        }),
                    ),
            )
    }

    /// Adds the path to `scan.exclude` in the config file, so no later scan shows it, and
    /// removes it from the lists on screen.
    pub(super) fn exclude(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let shown = self.shown_path(&path);
        let pattern = kuizer::config::exclude_pattern(&path);
        let Some(file) = self.config_path.clone() else {
            self.tell(
                Notice::Failed(format!(
                    "Could not exclude {shown}: this system has no config folder"
                )),
                cx,
            );
            return;
        };
        if !self.excluding.insert(path.clone()) {
            return;
        }
        let written = cx.background_spawn({
            let pattern = pattern.clone();
            async move {
                let _one_writer = CONFIG_WRITE
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                kuizer::config::add_exclude(&file, &pattern)
                    .map(drop)
                    .map_err(|error| error.to_string())
            }
        });
        cx.spawn(async move |this, cx| {
            let written = written.await;
            let _ = this.update(cx, |app, cx| {
                app.excluded_path(path, pattern, shown, written, cx);
            });
        })
        .detach();
    }

    pub(super) fn excluded_path(
        &mut self,
        path: PathBuf,
        pattern: String,
        shown: String,
        written: Result<(), String>,
        cx: &mut Context<Self>,
    ) {
        self.excluding.remove(&path);
        if let Err(error) = written {
            self.tell(
                Notice::Failed(format!("Could not exclude {shown}: {error}")),
                cx,
            );
            return;
        }
        if !self.config.scan.exclude.contains(&pattern) {
            self.config.scan.exclude.push(pattern);
        }
        self.excluded.push(path);
        self.prune(cx);
        self.tell(
            Notice::Info(format!("{shown} is excluded from every scan")),
            cx,
        );
        cx.notify();
    }

    /// Removes excluded paths from the report on screen and forgets selections of items that
    /// are gone.
    fn prune(&mut self, cx: &Context<Self>) {
        if let Phase::Ready(report) = &mut self.phase {
            report.drop_paths(&self.excluded);
            report.remove(&self.gone);
        }
        self.refresh_stats();
        self.refresh_rows();
        self.refresh_map(cx);
        if self
            .menu
            .as_ref()
            .is_some_and(|menu| !self.known(&menu.target))
        {
            self.menu = None;
        }
    }

    /// Whether a menu target is still on screen: a row of its list, or an item of the report
    /// that the space map shows whatever the list filters are.
    pub(super) fn known(&self, target: &Target) -> bool {
        self.target_exists(target)
            || matches!(target, Target::Item(source, id, _) if self.item((*source, *id)).is_some())
    }

    /// Adds the right-click menu to a row, and marks the row the keyboard menu would open for.
    pub(super) fn menu_row<E: InteractiveElement + ParentElement + Styled + FluentBuilder>(
        &self,
        row: E,
        target: Target,
        cursor: Option<&Target>,
        cx: &mut Context<Self>,
    ) -> E {
        let open = self.menu.as_ref().is_some_and(|menu| menu.target == target);
        let look = match &target {
            Target::Item(source, id, path) => Some(self.look(*source, *id, path)),
            _ => None,
        };
        let tab = target.tab();
        let marked = cursor == Some(&target);
        let bounds = marked.then(|| Rc::clone(&self.cursor_bounds));
        let clicked = target.clone();
        row.when_some(bounds, |this, bounds| {
            this.child(
                canvas(move |area, _, _| bounds.set(Some(area)), |_, (), _, _| {})
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
            )
            .child(ui::marker().debug_selector(|| "cursor".to_owned()))
        })
        .when_some(look, |this, look| self.flow_row(this, look))
        .when(open, |this| this.bg(theme::hover()))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |app, event: &MouseDownEvent, _, cx| {
                if event.modifiers.shift && !app.busy() {
                    app.select_to(&clicked);
                }
                app.cursors.insert(tab, clicked.clone());
                app.range = None;
                app.filtering = false;
                cx.notify();
            }),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |app, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                app.open_menu(target.clone(), event.position, false, cx);
            }),
        )
    }

    pub(super) fn render_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.menu.as_ref()?;
        let items = self.menu_items(&menu.target);
        let active = menu.active;
        let panel = div()
            .id("menu")
            .debug_selector(|| "menu".to_owned())
            .flex()
            .flex_col()
            .min_w(unit(size::MENU))
            .py(unit(space::XS))
            .rounded(theme::radius_panel())
            .bg(theme::elevated())
            .border_1()
            .border_color(theme::border_strong())
            .text_size(unit(font::BODY))
            .font_family(self.font.clone())
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .children(items.into_iter().enumerate().map(|(index, item)| {
                let label = item.label;
                let enabled = item.enabled;
                let divider = item.divided.then(|| {
                    div()
                        .h(unit(size::HAIR))
                        .my(unit(space::XS))
                        .bg(theme::border())
                });
                let row = div()
                    .id(("menu-item", index))
                    .debug_selector(move || format!("menu-{label}"))
                    .flex()
                    .items_center()
                    .gap(unit(space::XL))
                    .h(unit(size::CONTROL))
                    .mx(unit(space::XS))
                    .px(unit(space::M))
                    .rounded(theme::radius_control())
                    .text_color(if enabled {
                        theme::text()
                    } else {
                        theme::faint()
                    })
                    .when(enabled && active == Some(index), |this| {
                        this.bg(theme::tile_selected())
                    })
                    .when(enabled, |this| this.cursor_pointer())
                    .when_some(item.tip, |this, tip| this.tip(tooltip::plain(tip)))
                    .child(div().flex_1().child(label))
                    .child(ui::kbd(self.key_hint(Scope::Menu, item.choice.binding())))
                    .on_mouse_move(cx.listener(move |app, _: &MouseMoveEvent, _, cx| {
                        if let Some(menu) = &mut app.menu
                            && enabled
                            && menu.active != Some(index)
                        {
                            menu.active = Some(index);
                            app.redraw_chrome(cx);
                        }
                    }))
                    .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                        app.choose(index, cx);
                    }));
                div().flex().flex_col().children(divider).child(row)
            }));
        Some(
            div()
                .id("menu-backdrop")
                .absolute()
                .inset_0()
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|app, _: &MouseDownEvent, _, cx| app.close_menu(cx)),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(|app, _: &MouseDownEvent, _, cx| app.close_menu(cx)),
                )
                .child(
                    deferred(
                        anchored()
                            .position(menu.position)
                            .snap_to_window_with_margin(px(8.))
                            .child(panel),
                    )
                    .with_priority(1),
                )
                .into_any_element(),
        )
    }
}
