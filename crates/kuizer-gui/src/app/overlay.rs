//! The command palette, the shortcuts overlay, the list filter bar and the status bar hint.

use super::*;
use crate::commands::{Group, HINTS};

impl Kuizer {
    pub(super) fn render_palette(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let palette = self.palette.as_ref()?;
        let entries = self.palette_entries();
        let active = palette.active.min(entries.len().saturating_sub(1));
        let rows = entries.iter().enumerate().map(|(index, entry)| {
            let label = entry.spec.label.clone();
            let selector = format!("palette-{label}");
            let blocked = entry.blocked;
            div()
                .id(("palette-item", index))
                .debug_selector(move || selector.clone())
                .flex()
                .flex_none()
                .items_center()
                .gap(unit(8.))
                .h(unit(28.))
                .mx(unit(4.))
                .px(unit(8.))
                .rounded(theme::radius_control())
                .text_color(if blocked.is_some() {
                    theme::faint()
                } else {
                    theme::text()
                })
                .when(index == active, |this| this.bg(theme::selected()))
                .when(blocked.is_none(), |this| this.cursor_pointer())
                .child(div().flex_1().min_w_0().truncate().child(label))
                .children(blocked.map(|reason| {
                    div()
                        .flex_none()
                        .max_w(unit(240.))
                        .truncate()
                        .text_size(unit(11.5))
                        .child(reason)
                }))
                .children(
                    entry
                        .spec
                        .keys
                        .iter()
                        .map(|key| ui::kbd(commands::label(key))),
                )
                .on_mouse_move(cx.listener(move |app, _: &MouseMoveEvent, _, cx| {
                    if let Some(palette) = &mut app.palette
                        && palette.active != index
                    {
                        palette.active = index;
                        cx.notify();
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
            .gap(unit(6.))
            .px(unit(12.))
            .py(unit(6.))
            .border_t_1()
            .border_color(theme::border())
            .text_size(unit(11.5))
            .text_color(theme::faint())
            .child(ui::kbd(commands::label("up")))
            .child(ui::kbd(commands::label("down")))
            .child(div().pr(unit(8.)).child("move"))
            .child(ui::kbd(commands::label("enter")))
            .child(div().pr(unit(8.)).child("run"))
            .child(ui::kbd(commands::label("escape")))
            .child("close");
        let panel = div()
            .id("palette")
            .debug_selector(|| "palette".to_owned())
            .flex()
            .flex_col()
            .w(unit(560.))
            .rounded(theme::radius_dialog())
            .bg(theme::elevated())
            .border_1()
            .border_color(theme::border_strong())
            .shadow_lg()
            .font_family(self.font.clone())
            .text_size(unit(13.))
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .p(unit(8.))
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
                    .max_h(unit(340.))
                    .py(unit(4.))
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
                .pt(unit(72.))
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|app, _: &MouseDownEvent, _, cx| {
                        app.palette = None;
                        cx.notify();
                    }),
                )
                .child(panel)
                .into_any_element(),
        )
    }

    pub(super) fn render_shortcuts(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.shortcuts {
            return None;
        }
        let row = |label: SharedString, keys: &[&str]| {
            let selector = format!("shortcut-{label}");
            div()
                .debug_selector(move || selector.clone())
                .flex()
                .items_center()
                .gap(unit(6.))
                .h(unit(24.))
                .child(div().flex_1().min_w_0().truncate().child(label))
                .children(keys.iter().map(|key| ui::kbd(commands::label(key))))
        };
        let column = |title: &'static str| {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(unit(250.))
                .gap(unit(2.))
                .child(div().pb(unit(4.)).child(ui::heading(title)))
        };
        let specs = commands::table();
        let groups = Group::ALL.into_iter().map(|group| {
            column(group.label()).children(
                specs
                    .iter()
                    .filter(|spec| spec.group == group)
                    .map(|spec| row(spec.label.clone(), spec.keys)),
            )
        });
        let fields = column("In the palette, filters and dialogs").children(
            HINTS
                .iter()
                .map(|(label, keys)| row(SharedString::from(*label), keys)),
        );
        let panel = div()
            .id("shortcuts")
            .debug_selector(|| "shortcuts".to_owned())
            .flex()
            .flex_col()
            .gap(unit(14.))
            .w(unit(820.))
            .max_w(gpui::relative(0.92))
            .max_h(gpui::relative(0.9))
            .overflow_y_scroll()
            .p(unit(18.))
            .rounded(theme::radius_dialog())
            .bg(theme::elevated())
            .border_1()
            .border_color(theme::border_strong())
            .shadow_lg()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(unit(6.))
                    .child(
                        div()
                            .flex_1()
                            .text_size(unit(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Keyboard shortcuts"),
                    )
                    .child(ui::kbd(commands::label("escape")))
                    .child(div().text_color(theme::faint()).child("close")),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_x(unit(28.))
                    .gap_y(unit(16.))
                    .children(groups)
                    .child(fields),
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
                        app.shortcuts = false;
                        cx.notify();
                    }),
                )
                .child(panel)
                .into_any_element(),
        )
    }

    /// The filter field above a list, with how many rows it shows.
    pub(super) fn render_filter(&self, cx: &mut Context<Self>) -> Div {
        let text = self.filters.get(&self.tab).map_or("", String::as_str);
        let active = self.filtering;
        let (shown, total) = self.counts(self.tab);
        let key = commands::shortcut(&Command::FocusFilter).unwrap_or_default();
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(unit(8.))
            .px(unit(14.))
            .py(unit(6.))
            .border_b_1()
            .border_color(theme::border())
            .child(
                field::view("filter-field", text, "Filter by path or name", active)
                    .debug_selector(|| "filter".to_owned())
                    .w(unit(300.))
                    .tip(
                        Tooltip::details("Filter this list")
                            .row(key.clone(), "type to filter")
                            .row("Enter", "keep the filter and go back to the list")
                            .row("Esc", "clear the filter")
                            .build(),
                    )
                    .on_click(cx.listener(|app, _: &ClickEvent, window, cx| {
                        app.focus_filter(window, cx);
                    })),
            )
            .when(!active && text.is_empty(), |this| this.child(ui::kbd(key)))
            .child(div().flex_1())
            .child(
                div()
                    .debug_selector(|| "filter-count".to_owned())
                    .text_size(unit(12.))
                    .text_color(theme::muted())
                    .child(format!("{shown} of {total}")),
            )
    }

    /// Opens the palette from the status bar, and says which key does the same.
    pub(super) fn render_palette_hint(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let key = commands::shortcut(&Command::Palette).unwrap_or_default();
        div()
            .id("palette-hint")
            .flex()
            .items_center()
            .gap(unit(6.))
            .h(unit(26.))
            .px(unit(8.))
            .rounded(theme::radius_button())
            .cursor_pointer()
            .text_size(unit(12.5))
            .text_color(theme::muted())
            .hover(|style| style.bg(theme::hover()))
            .tip(tooltip::plain(
                "Search every command, section and recent folder",
            ))
            .child(ui::kbd(key))
            .child("Commands")
            .on_click(cx.listener(|app, _: &ClickEvent, _, cx| app.open_palette(cx)))
    }
}
