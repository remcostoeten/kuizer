use gpui::{
    Div, ElementId, FontWeight, ParentElement, Rgba, SharedString, Stateful, Styled, div,
    prelude::*, px, relative,
};

use crate::theme;

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
        .h(px(26.))
        .px(px(10.))
        .rounded(px(5.))
        .text_size(px(12.5))
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
        .size(px(14.))
        .flex_none()
        .rounded(px(3.))
        .border_1()
        .border_color(if checked {
            theme::primary()
        } else {
            theme::border_strong()
        })
        .when(checked, |this| {
            this.child(div().size(px(6.)).rounded(px(1.5)).bg(theme::primary()))
        })
}

pub fn card(title: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .flex_col()
        .min_w_0()
        .gap(px(10.))
        .p(px(14.))
        .rounded(px(6.))
        .bg(theme::panel())
        .border_1()
        .border_color(theme::border())
        .child(heading(title))
}

pub fn heading(title: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(12.))
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
        .gap(px(2.))
        .flex_1()
        .min_w(px(140.))
        .p(px(14.))
        .rounded(px(6.))
        .bg(theme::panel())
        .border_1()
        .border_color(theme::border())
        .child(
            div()
                .text_size(px(12.))
                .text_color(theme::muted())
                .child(label.into()),
        )
        .child(
            div()
                .text_size(px(20.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(value.into()),
        )
        .child(
            div()
                .truncate()
                .text_size(px(12.))
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
        .gap(px(4.))
        .child(
            div()
                .flex()
                .gap(px(8.))
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
                .h(px(4.))
                .w_full()
                .rounded(px(2.))
                .bg(theme::track())
                .child(
                    div()
                        .h_full()
                        .w(relative(fraction.max(0.005)))
                        .rounded(px(2.))
                        .bg(fill),
                ),
        )
}

pub fn empty(text: impl Into<SharedString>) -> Div {
    div()
        .p(px(14.))
        .text_color(theme::faint())
        .child(text.into())
}

pub fn safety_label(review: bool) -> Div {
    div()
        .text_size(px(12.))
        .text_color(if review {
            theme::review()
        } else {
            theme::muted()
        })
        .child(if review { "review" } else { "safe" })
}
