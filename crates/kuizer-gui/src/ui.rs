use gpui::{
    Div, ElementId, FontWeight, ParentElement, Rgba, SharedString, Stateful, Styled, div,
    prelude::*, relative,
};

use crate::glossary::Term;
use crate::theme::{self, unit};
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
        .h(unit(26.))
        .px(unit(10.))
        .rounded(theme::radius_button())
        .text_size(unit(12.5))
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
        base.opacity(0.4)
    }
}

pub fn checkbox(checked: bool) -> Div {
    div()
        .flex()
        .items_center()
        .justify_center()
        .size(unit(14.))
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
                    .size(unit(6.))
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
        .gap(unit(10.))
        .p(unit(14.))
        .rounded(theme::radius_panel())
        .bg(theme::panel())
        .border_1()
        .border_color(theme::border())
        .child(heading(title))
}

pub fn heading(title: impl Into<SharedString>) -> Div {
    div()
        .text_size(unit(12.))
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
        .gap(unit(2.))
        .flex_1()
        .min_w(unit(140.))
        .p(unit(14.))
        .rounded(theme::radius_panel())
        .bg(theme::panel())
        .border_1()
        .border_color(theme::border())
        .child(
            div()
                .text_size(unit(12.))
                .text_color(theme::muted())
                .child(label.into()),
        )
        .child(
            div()
                .text_size(unit(20.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(value.into()),
        )
        .child(
            div()
                .truncate()
                .text_size(unit(12.))
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
        .gap(unit(4.))
        .child(
            div()
                .flex()
                .gap(unit(8.))
                .child(div().flex_1().min_w_0().truncate().child(label.into()))
                .child(
                    div()
                        .flex_none()
                        .text_color(theme::muted())
                        .child(value.into()),
                ),
        )
        .child(
            div()
                .h(unit(4.))
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
        .p(unit(14.))
        .text_color(theme::faint())
        .child(text.into())
}

pub fn safety_label(id: impl Into<ElementId>, review: bool) -> Stateful<Div> {
    div()
        .id(id)
        .tip(tooltip::term(if review {
            Term::Review
        } else {
            Term::Safe
        }))
        .text_size(unit(12.))
        .text_color(if review {
            theme::review()
        } else {
            theme::muted()
        })
        .child(if review { "review" } else { "safe" })
}

pub fn segment(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    active: bool,
) -> Stateful<Div> {
    div()
        .id(id)
        .px(unit(10.))
        .h(unit(24.))
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
        .gap(unit(2.))
        .p(unit(2.))
        .rounded(theme::radius_panel())
        .bg(theme::panel())
        .border_1()
        .border_color(theme::border())
}

pub fn option_row(label: impl Into<SharedString>, hint: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .items_center()
        .gap(unit(16.))
        .py(unit(8.))
        .border_t_1()
        .border_color(theme::border())
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap(unit(2.))
                .child(div().child(label.into()))
                .child(
                    div()
                        .text_size(unit(12.))
                        .text_color(theme::faint())
                        .child(hint.into()),
                ),
        )
}
