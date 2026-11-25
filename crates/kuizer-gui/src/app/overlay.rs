//! The command palette, the shortcuts overlay, the list filter bar and the titlebar search field.

use super::*;
use crate::commands::{CONTROLS, Group, HINTS};

impl Kuizer {
    pub(super) fn render_palette(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let palette = self.overlays.palette.as_ref()?;
        let entries = self.palette_entries();
        let active = palette.active.min(entries.len().saturating_sub(1));
        let rows =
            entries.iter().enumerate().map(|(index, entry)| {
                let label = entry.spec.label.clone();
                let selector = format!("palette-{label}");
                let blocked = entry.blocked;
                div()
                    .id(("palette-item", index))
                    .debug_selector(move || selector.clone())
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(unit(space::M))
                    .h(unit(size::CONTROL))
                    .mx(unit(space::XS))
                    .px(unit(space::M))
                    .rounded(theme::radius_control())
                    .text_color(if blocked.is_some() {
                        theme::faint()
                    } else {
                        theme::text()
                    })
                    .when(index == active, |this| this.bg(theme::tile_selected()))
                    .when(blocked.is_none(), |this| this.cursor_pointer())
                    .child(div().flex_1().min_w_0().truncate().child(label))
                    .children(blocked.map(|reason| {
                        div()
                            .flex_none()
                            .max_w(unit(size::LABEL))
                            .truncate()
                            .text_size(unit(font::CAPTION))
                            .child(reason)
                    }))
                    .children(entry.spec.command.binding().into_iter().flat_map(
                        |(scope, action)| {
                            self.keymap
                                .keys(scope, action)
                                .map(|key| ui::kbd(commands::label(key)))
                        },
                    ))
                    .on_mouse_move(cx.listener(move |app, _: &MouseMoveEvent, _, cx| {
                        if let Some(palette) = &mut app.overlays.palette
                            && palette.active != index
                        {
                            palette.active = index;
                            app.redraw_chrome(cx);
                        }
                    }))
                    .on_click(cx.listener(move |app, _: &ClickEvent, window, cx| {
                        app.choose_entry(index, window, cx);
                    }))
            });
        let footer = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(unit(space::S))
            .px(unit(space::L))
            .py(unit(space::S))
            .border_t_1()
            .border_color(theme::border())
            .text_size(unit(font::CAPTION))
            .text_color(theme::faint())
            .child(ui::kbd(self.key_hint(Scope::Palette, Key::PalettePrevious)))
            .child(ui::kbd(self.key_hint(Scope::Palette, Key::PaletteNext)))
            .child(div().pr(unit(space::M)).child("move"))
            .child(ui::kbd(self.key_hint(Scope::Palette, Key::PaletteRun)))
            .child(div().pr(unit(space::M)).child("run"))
            .child(ui::kbd(self.key_hint(Scope::Palette, Key::Back)))
            .child("close");
        let panel = div()
            .id("palette")
            .debug_selector(|| "palette".to_owned())
            .flex()
            .flex_col()
            .w(unit(size::PALETTE))
            .rounded(theme::radius_dialog())
            .bg(theme::elevated())
            .border_1()
            .border_color(theme::border_strong())
            .font_family(self.chrome.font.clone())
            .text_size(unit(font::BODY))
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .p(unit(space::M))
                    .border_b_1()
                    .border_color(theme::border())
                    .child(field::view(
                        "palette-input",
                        &palette.query,
                        "Type a command or a recent folder",
                        true,
                    )),
            )
            .child(
                div()
                    .id("palette-list")
                    .flex()
                    .flex_col()
                    .max_h(unit(size::LIST))
                    .py(unit(space::XS))
                    .overflow_y_scroll()
                    .track_scroll(&palette.scroll)
                    .children(rows)
                    .when(entries.is_empty(), |this| {
                        this.child(ui::empty("No command matches"))
                    }),
            )
            .child(footer);
        Some(
            div()
                .id("palette-backdrop")
                .absolute()
                .inset_0()
                .flex()
                .flex_col()
                .items_center()
                .pt(unit(size::DROP))
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|app, _: &MouseDownEvent, _, cx| {
                        app.overlays.palette = None;
                        cx.notify();
                    }),
                )
                .child(panel)
                .into_any_element(),
        )
    }

    pub(super) fn render_shortcuts(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.overlays.shortcuts {
            return None;
        }
        let row = |label: SharedString, keys: Vec<&str>| {
            let selector = format!("shortcut-{label}");
            div()
                .debug_selector(move || selector.clone())
                .flex()
                .items_center()
                .gap(unit(space::S))
                .h(unit(size::SEGMENT))
                .child(div().flex_1().min_w_0().truncate().child(label))
                .children(keys.iter().map(|key| ui::kbd(commands::label(key))))
        };
        let column = |title: &'static str| {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(unit(size::KEYS))
                .gap(unit(space::XXS))
                .child(div().pb(unit(space::XS)).child(ui::heading(title)))
        };
        let specs = commands::table();
        let groups = Group::ALL.into_iter().map(|group| {
            column(group.label()).children(specs.iter().filter(|spec| spec.group == group).map(
                |spec| {
                    row(
                        spec.label.clone(),
                        spec.command
                            .binding()
                            .into_iter()
                            .flat_map(|(scope, action)| self.keymap.keys(scope, action))
                            .collect(),
                    )
                },
            ))
        });
        let fields = column("In the palette, filters and dialogs").children(HINTS.iter().map(
            |(label, scope, actions)| {
                row(
                    SharedString::from(*label),
                    actions
                        .iter()
                        .flat_map(|action| self.keymap.keys(*scope, *action))
                        .collect(),
                )
            },
        ));
        let controls = column("Buttons and controls").children(CONTROLS.iter().map(
            |(label, scope, actions)| {
                row(
                    SharedString::from(*label),
                    actions
                        .iter()
                        .flat_map(|action| self.keymap.keys(*scope, *action))
                        .collect(),
                )
            },
        ));
        let panel = div()
            .id("shortcuts")
            .debug_selector(|| "shortcuts".to_owned())
            .flex()
            .flex_col()
            .gap(unit(space::XL))
            .w(unit(size::SHORTCUTS))
            .max_w(gpui::relative(0.92))
            .max_h(gpui::relative(0.9))
            .overflow_y_scroll()
            .track_scroll(&self.overlays.shortcuts_scroll)
            .p(unit(space::XL))
            .rounded(theme::radius_dialog())
            .bg(theme::elevated())
            .border_1()
            .border_color(theme::border_strong())
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(unit(space::S))
                    .child(
                        div()
                            .flex_1()
                            .text_size(unit(font::TITLE))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Keyboard shortcuts"),
                    )
                    .child(ui::kbd(
                        self.key_hint(Scope::Shortcuts, Key::ShortcutsClose),
                    ))
                    .child(div().text_color(theme::faint()).child("close")),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_x(unit(space::XXL))
                    .gap_y(unit(space::XL))
                    .children(groups)
                    .child(fields)
                    .child(controls),
            );
        Some(
            div()
                .id("shortcuts-backdrop")
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(theme::scrim())
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|app, _: &MouseDownEvent, _, cx| {
                        app.overlays.shortcuts = false;
                        cx.notify();
                    }),
                )
                .child(panel)
                .into_any_element(),
        )
    }

    /// The filter field of a list, with the key that focuses it while it is empty.
    pub(super) fn render_filter(&self, cx: &mut Context<Self>) -> Div {
        let text = self
            .list
            .filters
            .get(&self.list.tab)
            .map_or("", String::as_str);
        let active = self.list.filtering;
        let key = self.hint(&Command::FocusFilter);
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(unit(space::M))
            .child(
                self.control(
                    field::view("filter-field", text, "Filter by path or name", active)
                        .debug_selector(|| "filter".to_owned())
                        .w(unit(size::FILTER))
                        .tip(
                            Tooltip::details("Filter this list")
                                .row(key.clone(), "type to filter")
                                .row("Enter", "keep the filter and go back to the list")
                                .row("Esc", "clear the filter")
                                .build(),
                        ),
                    cx,
                    |app, window, cx| app.focus_filter(window, cx),
                ),
            )
            .when(!active && text.is_empty(), |this| this.child(ui::kbd(key)))
    }

    /// The start of every list toolbar: the filter, and the safety segments on sections whose
    /// rows are graded. Sections add their own controls, a spacer and `tally` after it.
    pub(super) fn list_toolbar(&self, cx: &mut Context<Self>) -> Div {
        let can_select = self.report().is_some() && !self.busy() && self.list.tab.selects();
        ui::toolbar()
            .child(self.render_filter(cx))
            .when(self.list.tab.grades(), |this| {
                this.child(self.safety_segments(cx))
            })
            .when(self.list.tab.selects(), |this| {
                this.child(
                    self.button(
                        "select-safe",
                        "Select safe",
                        ButtonKind::Secondary,
                        can_select,
                        cx,
                        |app, _, cx| {
                            if app.list.tab.selects() && !app.busy() {
                                app.select_safe(cx);
                            }
                        },
                    )
                    .tip(tooltip::term(Term::SelectSafe)),
                )
            })
    }

    fn safety_segments(&self, cx: &mut Context<Self>) -> Div {
        let totals = match self.list.tab {
            Tab::Artifacts => Some(&self.session.report_stats.artifacts),
            Tab::Caches => Some(&self.session.report_stats.caches),
            Tab::Toolchains => Some(&self.session.report_stats.toolchains),
            Tab::Docker => Some(&self.session.report_stats.docker),
            _ => None,
        };
        let segment = |id: &'static str, label: &'static str, value: Option<Safety>| {
            let choice = ui::segment(id, label, self.list.safety == value).when_some(
                value,
                |this, safety| {
                    let term = Term::safety(safety);
                    let tip = Tooltip::new(term.text()).title(term.title());
                    let tip = match totals {
                        Some(totals) => {
                            let tally = match safety {
                                Safety::Safe => &totals.safe,
                                Safety::Review => &totals.review,
                            };
                            tip.row("Items", tally.count.to_string())
                                .row("Size", theme::bytes(tally.bytes))
                        }
                        None => tip,
                    };
                    this.tip(tip.build())
                },
            );
            self.control(choice, cx, move |app, _, cx| {
                app.list.safety = value;
                app.filtered();
                cx.notify();
            })
        };
        ui::segments()
            .child(segment("safety-all", "All", None))
            .child(segment("safety-safe", "Safe", Some(Safety::Safe)))
            .child(segment("safety-review", "Review", Some(Safety::Review)))
    }

    /// The figure at the end of a list toolbar: how many rows are listed out of how many the
    /// section has, and their size when the section measures one.
    pub(super) fn tally(&self, tab: Tab, bytes: Option<u64>) -> Stateful<Div> {
        let (shown, total) = self.counts(tab);
        let (one, many) = tab.noun();
        let noun = if total == 1 { one } else { many };
        let strong = |text: String| {
            div()
                .text_color(theme::text())
                .font_weight(FontWeight::MEDIUM)
                .child(text)
        };
        let count = if shown == total {
            strong(total.to_string())
        } else {
            strong(format!("{shown} of {total}"))
        };
        let tip = match tab {
            Tab::Issues => Some(Tooltip::new(Term::Issues.text()).title(Term::Issues.title())),
            Tab::Repos => Some(Tooltip::new("What git gc frees in the listed repositories")),
            _ if bytes.is_some() => Some(Tooltip::new("Space the listed rows take on disk")),
            _ => None,
        };
        ui::tabular(
            div()
                .id("tally")
                .debug_selector(|| "tally".to_owned())
                .flex()
                .flex_none()
                .items_baseline()
                .gap(unit(space::XS))
                .text_size(unit(font::SMALL))
                .text_color(theme::muted())
                .when_some(tip, |this, tip| this.tip(tip.build()))
                .child(count)
                .child(if bytes.is_some() {
                    format!("{noun},")
                } else {
                    noun.to_owned()
                })
                .children(bytes.map(|bytes| strong(theme::bytes(bytes)))),
        )
    }

    /// Opens the palette from the titlebar, and shows the key that does the same.
    pub(super) fn render_search(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let key = self.hint(&Command::Palette);
        let search = div()
            .id("search")
            .debug_selector(|| "search".to_owned())
            .flex()
            .items_center()
            .gap(unit(space::M))
            .w(unit(size::TRIGGER))
            .min_w_0()
            .h(unit(size::SEGMENT))
            .pl(unit(space::M))
            .pr(unit(space::S))
            .rounded(theme::radius_control())
            .bg(theme::panel())
            .border_1()
            .border_color(theme::border())
            .text_size(unit(font::SMALL))
            .text_color(theme::faint())
            .cursor_pointer()
            .hover(|style| style.bg(theme::hover()).text_color(theme::muted()))
            .tip(
                Tooltip::new("Search every command, section and recent folder")
                    .title("Search or run a command")
                    .keys(&[&key])
                    .build(),
            )
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(icon::sized(Icon::Search, theme::faint(), size::CHECK))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child("Search or run a command"),
            )
            .child(ui::kbd(key));
        self.control(search, cx, |app, _, cx| app.open_palette(cx))
    }
}
