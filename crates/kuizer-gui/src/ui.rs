use std::sync::Arc;

use gpui::{
    Animation, AnimationExt, AnyElement, Div, ElementId, FontFeatures, FontWeight, ParentElement,
    Rgba, SharedString, Stateful, Styled, div, prelude::*, relative,
};

use crate::glossary::Term;
use crate::icon::{self, Icon};
use crate::motion;
use crate::theme::{self, font, size, space, unit};
use crate::tooltip::{self, Tip};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Primary,
    Secondary,
    Ghost,
    Danger,
}

pub fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    kind: ButtonKind,
    enabled: bool,
) -> Stateful<Div> {
    let (background, foreground, hover, hover_text) = match kind {
        ButtonKind::Primary => (
            theme::primary(),
            theme::on_primary(),
            theme::text(),
            theme::on_primary(),
        ),
        ButtonKind::Secondary => (
            theme::hover(),
            theme::text(),
            theme::border_strong(),
            theme::text(),
        ),
        ButtonKind::Ghost => (
            theme::transparent(),
            theme::muted(),
            theme::hover(),
            theme::text(),
        ),
        ButtonKind::Danger => (
            theme::danger_solid(),
            theme::on_danger(),
            theme::danger_hover(),
            theme::on_danger(),
        ),
    };
    let base = div()
        .id(id)
        .flex()
        .items_center()
        .gap(unit(space::M))
        .h(unit(size::CONTROL))
        .px(unit(space::L))
        .rounded(theme::radius_button())
        .text_size(unit(font::BODY))
        .font_weight(if kind == ButtonKind::Danger {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::MEDIUM
        })
        .bg(background)
        .text_color(foreground)
        .child(label.into());
    if enabled {
        base.cursor_pointer()
            .hover(move |style| style.bg(hover).text_color(hover_text))
    } else {
        base.opacity(theme::DISABLED)
    }
}

pub fn checkbox(checked: bool) -> Div {
    check_mark(if checked { 1.0 } else { 0.0 })
}

/// A checkbox part way between clear at 0 and checked at 1, for the moment it is toggled.
pub fn check_mark(weight: f32) -> Div {
    let faded = |color: Rgba| Rgba {
        a: color.a * weight,
        ..color
    };
    check_box(if weight >= 0.5 {
        theme::bar()
    } else {
        theme::border_strong()
    })
    .when(weight > 0.0, |this| {
        this.bg(faded(theme::bar())).child(icon::sized(
            Icon::Check,
            faded(theme::on_accent()),
            size::GLYPH,
        ))
    })
}

/// The checkbox of a row that is being deleted: a danger outline around a dash.
pub fn deleting_mark() -> Div {
    check_box(theme::danger()).child(
        div()
            .w(unit(size::DOT))
            .h(unit(size::MARKER))
            .rounded(theme::radius_dot())
            .bg(theme::danger()),
    )
}

fn check_box(edge: Rgba) -> Div {
    div()
        .flex()
        .items_center()
        .justify_center()
        .size(unit(size::CHECK))
        .flex_none()
        .rounded(theme::radius_checkbox())
        .border_1()
        .border_color(edge)
}

pub fn switch(on: bool) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .when(on, |this| this.justify_end())
        .w(unit(size::SWITCH))
        .h(unit(size::SWITCH_HEIGHT))
        .p(unit(space::XXS))
        .rounded(theme::radius_pill())
        .border_1()
        .border_color(if on {
            theme::bar()
        } else {
            theme::border_strong()
        })
        .bg(if on { theme::bar() } else { theme::track() })
        .child(
            div()
                .size(unit(
                    size::SWITCH_HEIGHT - 2. * space::XXS - 2. * size::HAIR,
                ))
                .rounded(theme::radius_pill())
                .bg(if on {
                    theme::on_accent()
                } else {
                    theme::muted()
                }),
        )
}

pub fn card(title: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .flex_col()
        .min_w_0()
        .gap(unit(space::L))
        .p(unit(space::GUTTER))
        .rounded(theme::radius_panel())
        .bg(theme::elevated())
        .border_1()
        .border_color(theme::border())
        .child(heading(title))
}

pub fn heading(title: impl Into<SharedString>) -> Div {
    theme::label(div())
        .text_color(theme::faint())
        .child(title.into().to_uppercase())
}

/// @name amount
/// @description A size set as a figure: the number at `size` and its unit smaller and muted
/// after it, as the sidebar total and section headers show it.
///
/// @example
/// ui::amount(bytes, font::FIGURE)
pub fn amount(bytes: u64, size: f32) -> Div {
    let text = theme::bytes(bytes);
    let (number, suffix) = text.split_once(' ').unwrap_or((text.as_str(), ""));
    tabular(
        div()
            .flex()
            .items_baseline()
            .gap(unit(space::XS))
            .child(
                div()
                    .text_size(unit(size))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(number.to_owned()),
            )
            .child(
                div()
                    .text_size(unit(font::BODY))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme::muted())
                    .child(suffix.to_owned()),
            ),
    )
}

/// @name gauge
/// @description The short bar next to a size in a table row, filled to `fraction`.
///
/// @example
/// ui::gauge(0.4, theme::bar_mid(), theme::track())
pub fn gauge(fraction: f32, fill: Rgba, track: Rgba) -> Div {
    div()
        .flex_none()
        .w(unit(size::GAUGE))
        .h(unit(size::TRACK))
        .rounded(theme::radius_bar())
        .bg(track)
        .overflow_hidden()
        .child(
            div()
                .h_full()
                .w(relative(fraction.clamp(0.0, 1.0)))
                .min_w(unit(size::SLIVER))
                .rounded(theme::radius_bar())
                .bg(fill),
        )
}

/// @name tone_of
/// @description The neutral fill for a size: stronger for larger sizes, so the biggest items
/// stand out in a list without a hue.
///
/// @example
/// let fill = ui::tone_of(candidate.usage.allocated);
pub fn tone_of(bytes: u64) -> Rgba {
    const MB: u64 = 1 << 20;
    match bytes {
        _ if bytes >= 1 << 30 => theme::bar_strong(),
        _ if bytes >= 100 * MB => theme::bar_mid(),
        _ if bytes >= MB => theme::bar_soft(),
        _ => theme::bar_rest(),
    }
}

/// @name caption
/// @description A small upper case label in the monospace typeface, as group names, column
/// headings and figure captions are set.
///
/// @example
/// ui::caption("Reclaimable")
pub fn caption(text: impl Into<SharedString>) -> Div {
    theme::label(div())
        .text_color(theme::faint())
        .child(text.into().to_uppercase())
}

pub fn stat(
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
    sub: impl Into<SharedString>,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(unit(space::XXS))
        .flex_1()
        .min_w(unit(size::STAT))
        .pl(unit(space::XL))
        .py(unit(space::XS))
        .border_l_1()
        .border_color(theme::border_strong())
        .child(
            div()
                .text_size(unit(font::SMALL))
                .text_color(theme::muted())
                .child(label.into()),
        )
        .child(tabular(
            div()
                .text_size(unit(font::FIGURE))
                .font_weight(FontWeight::SEMIBOLD)
                .child(value.into()),
        ))
        .child(
            div()
                .truncate()
                .text_size(unit(font::SMALL))
                .text_color(theme::faint())
                .child(sub.into()),
        )
}

pub fn bar_row(
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
    fraction: f32,
    fill: Rgba,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(unit(space::XS))
        .child(
            div()
                .flex()
                .gap(unit(space::M))
                .child(div().flex_1().min_w_0().truncate().child(label.into()))
                .child(tabular(
                    div()
                        .flex_none()
                        .text_color(theme::muted())
                        .child(value.into()),
                )),
        )
        .child(
            div()
                .h(unit(size::TRACK))
                .w_full()
                .rounded(theme::radius_bar())
                .bg(theme::track())
                .child(
                    div()
                        .h_full()
                        .w(relative(fraction))
                        .min_w(unit(size::SLIVER))
                        .rounded(theme::radius_bar())
                        .bg(fill),
                ),
        )
}

pub fn empty(text: impl Into<SharedString>) -> Div {
    div()
        .p(unit(space::GUTTER))
        .text_color(theme::faint())
        .child(text.into())
}

/// @name appear
/// @description Fades `element` in over one motion step the first time it is drawn under `id`,
/// and shows it at once when `appearance.motion` is off.
///
/// @example
/// let state = appear("scan-failed", div().child("The scan failed"));
pub fn appear(
    id: impl Into<ElementId>,
    element: impl Styled + IntoElement + 'static,
) -> AnyElement {
    if motion::reduced() {
        return element.into_any_element();
    }
    element
        .with_animation(
            id,
            Animation::new(motion::STEP).with_easing(motion::ease_out),
            |element, progress| element.opacity(progress),
        )
        .into_any_element()
}

pub fn safety_label(id: impl Into<ElementId>, review: bool) -> Stateful<Div> {
    let (label, tone, term) = if review {
        ("review", Tone::Review, Term::Review)
    } else {
        ("safe", Tone::Safe, Term::Safe)
    };
    tag(id, label, tone).tip(tooltip::term(term))
}

pub fn segment(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    active: bool,
) -> Stateful<Div> {
    div()
        .id(id)
        .px(unit(space::L))
        .h(unit(size::SEGMENT))
        .flex()
        .items_center()
        .rounded(theme::radius_tag())
        .cursor_pointer()
        .text_size(unit(font::SMALL))
        .text_color(if active {
            theme::text()
        } else {
            theme::muted()
        })
        .when(active, |this| {
            this.bg(theme::border_strong())
                .font_weight(FontWeight::MEDIUM)
        })
        .when(!active, |this| {
            this.hover(|style| style.bg(theme::hover()).text_color(theme::text()))
        })
        .child(label.into())
}

pub fn segments() -> Div {
    div()
        .flex()
        .flex_none()
        .gap(unit(space::XXS))
        .p(unit(space::XXS))
        .rounded(theme::radius_control())
        .bg(theme::background())
}

pub fn option_row(label: impl Into<SharedString>, hint: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .items_center()
        .gap(unit(space::XL))
        .py(unit(space::row_y()))
        .border_t_1()
        .border_color(theme::border())
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap(unit(space::XXS))
                .child(div().child(label.into()))
                .child(
                    div()
                        .text_size(unit(font::SMALL))
                        .text_color(theme::faint())
                        .child(hint.into()),
                ),
        )
}

pub fn kbd(key: impl Into<SharedString>) -> Div {
    kbd_on(key, theme::kbd(), theme::muted())
}

/// A key badge on a filled control, in the colors that read on that fill.
pub fn kbd_on(key: impl Into<SharedString>, fill: Rgba, color: Rgba) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .min_w(unit(size::KEY))
        .h(unit(size::KEY))
        .px(unit(space::XS))
        .rounded(theme::radius_tag())
        .bg(fill)
        .text_color(color)
        .font_weight(FontWeight::MEDIUM)
        .map(theme::label)
        .child(key.into())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Safe,
    Review,
    Danger,
}

pub fn tag(id: impl Into<ElementId>, label: impl Into<SharedString>, tone: Tone) -> Stateful<Div> {
    let (background, foreground) = match tone {
        Tone::Neutral => (theme::kbd(), theme::muted()),
        Tone::Safe => (theme::transparent(), theme::faint()),
        Tone::Review => (theme::review_soft(), theme::review()),
        Tone::Danger => (theme::transparent(), theme::danger()),
    };
    theme::label(div())
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap(unit(space::S))
        .h(unit(size::BADGE))
        .when(matches!(tone, Tone::Neutral | Tone::Review), |this| {
            this.px(unit(space::S))
        })
        .rounded(theme::radius_tag())
        .bg(background)
        .font_weight(FontWeight::MEDIUM)
        .text_color(foreground)
        .when(tone == Tone::Safe, |this| {
            this.child(
                div()
                    .size(unit(size::DOT))
                    .flex_none()
                    .rounded(theme::radius_pill())
                    .bg(theme::ok()),
            )
        })
        .child(label.into())
}

/// @name tabular
/// @description Gives digits a fixed width, so sizes and counts line up in columns and do not
/// shift while they change.
///
/// @example
/// ui::tabular(div().child(theme::bytes(bytes)))
pub fn tabular<E: Styled>(mut element: E) -> E {
    element
        .text_style()
        .get_or_insert_with(Default::default)
        .font_features = Some(FontFeatures(Arc::new(vec![("tnum".into(), 1)])));
    element
}

/// @name figure
/// @description A size or count in a table or the sidebar: tabular digits, right aligned, with
/// more weight than the text around it.
///
/// @example
/// ui::figure(theme::bytes(bytes))
pub fn figure(value: impl Into<SharedString>) -> Div {
    tabular(
        div()
            .flex_none()
            .text_right()
            .font_weight(FontWeight::MEDIUM)
            .child(value.into()),
    )
}

/// @name marker
/// @description The faint tint laid over the row that has the keyboard, on top of its own
/// background so it shows on selected rows too.
///
/// @example
/// row.child(ui::marker())
pub fn marker() -> Div {
    div().absolute().inset_0().bg(theme::cursor())
}

/// @name nav_item
/// @description A row of the sidebar or the settings navigation: icon, label and an optional
/// figure on the right. The open item is a fill, not a stripe.
///
/// @example
/// ui::nav_item("History", active).child(label)
pub fn nav_item(id: impl Into<ElementId>, active: bool) -> Stateful<Div> {
    div()
        .id(id)
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .gap(unit(space::M))
        .h(unit(size::row()))
        .px(unit(space::M))
        .rounded(theme::radius_control())
        .cursor_pointer()
        .text_color(if active {
            theme::text()
        } else {
            theme::muted()
        })
        .when(active, |this| {
            this.bg(theme::hover()).font_weight(FontWeight::MEDIUM)
        })
        .when(!active, |this| {
            this.hover(|style| style.bg(theme::hover_row()).text_color(theme::text()))
        })
}

/// @name toolbar
/// @description The bar at the top of a section with its filters and totals, on the shared
/// gutter.
///
/// @example
/// ui::toolbar().child(ui::segments()).child(div().flex_1()).child(total)
pub fn toolbar() -> Div {
    div()
        .flex()
        .flex_none()
        .flex_wrap()
        .items_center()
        .gap_x(unit(space::L))
        .gap_y(unit(space::XS))
        .min_h(unit(size::toolbar()))
        .px(unit(space::GUTTER))
        .py(unit(space::bar_y()))
        .text_color(theme::muted())
}

/// @name divider
/// @description A one pixel horizontal rule between groups in a menu or the sidebar.
///
/// @example
/// menu.child(ui::divider())
pub fn divider() -> Div {
    div()
        .flex_none()
        .h(unit(size::HAIR))
        .my(unit(space::XS))
        .bg(theme::border())
}
