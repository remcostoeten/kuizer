use std::sync::Arc;

use gpui::{
    Div, ElementId, FontFeatures, FontWeight, ParentElement, Rgba, SharedString, Stateful, Styled,
    div, prelude::*, relative,
};

use crate::glossary::Term;
use crate::theme::{self, font, size, space, unit};
use crate::tooltip::{self, Tip};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Primary,
    Secondary,
    Ghost,
}

pub fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    kind: ButtonKind,
    enabled: bool,
) -> Stateful<Div> {
    let (background, foreground, hover) = match kind {
        ButtonKind::Primary => (theme::primary(), theme::on_primary(), theme::text()),
        ButtonKind::Secondary => (theme::elevated(), theme::text(), theme::hover()),
        ButtonKind::Ghost => (theme::transparent(), theme::muted(), theme::hover()),
    };
    let base = div()
        .id(id)
        .flex()
        .items_center()
        .h(unit(size::CONTROL))
        .px(unit(space::L))
        .rounded(theme::radius_button())
        .text_size(unit(font::BODY))
        .font_weight(FontWeight::MEDIUM)
        .bg(background)
        .text_color(foreground)
        .when(kind == ButtonKind::Secondary, |this| {
            this.border_1().border_color(theme::border_strong())
        })
        .child(label.into());
    if enabled {
        base.cursor_pointer().hover(move |style| style.bg(hover))
    } else {
        base.opacity(theme::DISABLED)
    }
}

pub fn checkbox(checked: bool) -> Div {
    div()
        .flex()
        .items_center()
        .justify_center()
        .size(unit(size::CHECK))
        .flex_none()
        .rounded(theme::radius_checkbox())
        .border_1()
        .border_color(if checked {
            theme::primary()
        } else {
            theme::border_strong()
        })
        .when(checked, |this| {
            this.child(
                div()
                    .size(unit(size::DOT))
                    .rounded(theme::radius_dot())
                    .bg(theme::primary()),
            )
        })
}

pub fn card(title: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .flex_col()
        .min_w_0()
        .gap(unit(space::L))
        .p(unit(space::GUTTER))
        .rounded(theme::radius_panel())
        .bg(theme::panel())
        .border_1()
        .border_color(theme::border())
        .child(heading(title))
}

pub fn heading(title: impl Into<SharedString>) -> Div {
    div()
        .text_size(unit(font::SMALL))
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme::muted())
        .child(title.into())
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
        .p(unit(space::GUTTER))
        .rounded(theme::radius_panel())
        .bg(theme::panel())
        .border_1()
        .border_color(theme::border())
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
                        .w(relative(fraction.max(0.005)))
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

pub fn safety_label(id: impl Into<ElementId>, review: bool) -> Stateful<Div> {
    let (label, tone, term) = if review {
        ("review", Tone::Review, Term::Review)
    } else {
        ("safe", Tone::Neutral, Term::Safe)
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
        .rounded(theme::radius_control())
        .cursor_pointer()
        .text_color(if active {
            theme::text()
        } else {
            theme::muted()
        })
        .when(active, |this| this.bg(theme::selected()))
        .hover(|style| style.text_color(theme::text()))
        .child(label.into())
}

pub fn segments() -> Div {
    div()
        .flex()
        .flex_none()
        .gap(unit(space::XXS))
        .p(unit(space::XXS))
        .rounded(theme::radius_panel())
        .bg(theme::panel())
        .border_1()
        .border_color(theme::border())
}

pub fn option_row(label: impl Into<SharedString>, hint: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .items_center()
        .gap(unit(space::XL))
        .py(unit(space::M))
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
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .min_w(unit(size::BADGE))
        .h(unit(size::BADGE))
        .px(unit(space::XS))
        .rounded(theme::radius_control())
        .bg(theme::background())
        .border_1()
        .border_color(theme::border_strong())
        .text_size(unit(font::CAPTION))
        .text_color(theme::muted())
        .child(key.into())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Review,
}

pub fn tag(id: impl Into<ElementId>, label: impl Into<SharedString>, tone: Tone) -> Stateful<Div> {
    let (background, foreground) = match tone {
        Tone::Neutral => (theme::track(), theme::muted()),
        Tone::Review => (theme::review_soft(), theme::review()),
    };
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .h(unit(size::BADGE))
        .px(unit(space::S))
        .rounded(theme::radius_control())
        .bg(background)
        .text_size(unit(font::CAPTION))
        .text_color(foreground)
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
        .min_h(unit(size::TOOLBAR))
        .px(unit(space::GUTTER))
        .py(unit(space::S))
        .border_b_1()
        .border_color(theme::border())
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
