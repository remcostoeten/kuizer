use std::cell::Cell;
use std::path::Path;
use std::time::{Duration, Instant};

use gpui::{
    Animation, AnimationExt, AnyView, App, FontWeight, Global, IntoElement, ParentElement, Render,
    SharedString, Styled, Window, div, prelude::*,
};
use kuizer::output::display_path;

use crate::glossary::Term;
use crate::motion;
use crate::theme::{self, unit};

const ENTER: Duration = Duration::from_millis(125);
const WARM: Duration = Duration::from_secs(1);
const RISE: f32 = 3.;

thread_local! {
    static CLOSED: Cell<Option<Instant>> = const { Cell::new(None) };
}

pub struct Typeface(pub SharedString);

impl Global for Typeface {}

pub struct Tooltip {
    title: Option<SharedString>,
    text: SharedString,
    instant: bool,
}

impl Tooltip {
    pub fn new(title: Option<SharedString>, text: SharedString) -> Self {
        let warm = CLOSED
            .with(Cell::get)
            .is_some_and(|closed| closed.elapsed() < WARM);
        Self {
            title,
            text,
            instant: warm,
        }
    }
}

impl Drop for Tooltip {
    fn drop(&mut self) {
        CLOSED.with(|closed| closed.set(Some(Instant::now())));
    }
}

impl Render for Tooltip {
    fn render(&mut self, _window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let font = cx.try_global::<Typeface>().map(|face| face.0.clone());
        let card = div()
            .relative()
            .flex()
            .flex_col()
            .gap(unit(2.))
            .max_w(unit(340.))
            .px(unit(8.))
            .py(unit(6.))
            .rounded(theme::radius_control())
            .bg(theme::elevated())
            .border_1()
            .border_color(theme::border_strong())
            .shadow_md()
            .debug_selector(|| "tooltip".to_owned())
            .text_size(unit(12.))
            .text_color(theme::muted())
            .when_some(font, |this, font| this.font_family(font))
            .when_some(self.title.clone(), |this, title| {
                this.child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::text())
                        .child(title),
                )
            })
            .child(self.text.clone());
        let shell = div().pl(unit(10.)).pt(unit(18.));
        if self.instant {
            return shell.child(card).into_any_element();
        }
        let rise = if motion::reduced() { 0. } else { RISE };
        shell
            .child(card.with_animation(
                "tooltip-enter",
                Animation::new(ENTER).with_easing(motion::ease_out),
                move |card, progress| card.opacity(progress).top(unit(rise * (1. - progress))),
            ))
            .into_any_element()
    }
}

pub trait Tip: StatefulInteractiveElement {
    fn tip(self, build: impl Fn(&mut Window, &mut App) -> AnyView + 'static) -> Self {
        self.on_hover(|_, window, _| window.refresh())
            .tooltip(build)
    }
}

impl<E: StatefulInteractiveElement> Tip for E {}

pub fn plain(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView {
    let text = text.into();
    move |_, cx| cx.new(|_| Tooltip::new(None, text.clone())).into()
}

pub fn path(path: &Path) -> impl Fn(&mut Window, &mut App) -> AnyView + use<> {
    plain(display_path(path))
}

pub fn term(term: Term) -> impl Fn(&mut Window, &mut App) -> AnyView {
    titled(term.title(), term.text())
}

pub fn titled(
    title: impl Into<SharedString>,
    text: impl Into<SharedString>,
) -> impl Fn(&mut Window, &mut App) -> AnyView {
    let title = title.into();
    let text = text.into();
    move |_, cx| {
        cx.new(|_| Tooltip::new(Some(title.clone()), text.clone()))
            .into()
    }
}

#[cfg(test)]
mod tests {
    use gpui::{Context, Modifiers, MouseMoveEvent, TestAppContext, VisualTestContext, point, px};

    use super::*;

    struct Target;

    impl Render for Target {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(
                div()
                    .id("target")
                    .w(px(200.))
                    .h(px(40.))
                    .tip(term(Term::Review))
                    .child("review"),
            )
        }
    }

    fn hover(cx: &mut VisualTestContext, x: f32, y: f32) {
        cx.simulate_event(MouseMoveEvent {
            position: point(px(x), px(y)),
            pressed_button: None,
            modifiers: Modifiers::default(),
        });
    }

    #[test]
    fn a_tooltip_skips_its_entrance_right_after_another_closed() {
        let first = Tooltip::new(None, "first".into());
        assert!(!first.instant);
        drop(first);
        assert!(Tooltip::new(None, "second".into()).instant);
        CLOSED.with(|closed| closed.set(Some(Instant::now() - WARM * 2)));
        assert!(!Tooltip::new(None, "third".into()).instant);
    }

    #[gpui::test]
    fn hovering_shows_the_tooltip_below_the_cursor_after_the_delay(cx: &mut TestAppContext) {
        let (_view, cx) = cx.add_window_view(|_, _| Target);
        let shown = |cx: &mut VisualTestContext| cx.debug_bounds("tooltip");
        hover(cx, 20., 20.);
        cx.executor().advance_clock(Duration::from_millis(300));
        cx.run_until_parked();
        assert!(shown(cx).is_none());
        cx.executor().advance_clock(Duration::from_millis(300));
        cx.run_until_parked();
        let bounds = shown(cx).expect("the tooltip is painted");
        assert!(bounds.origin.x > px(20.) && bounds.origin.y > px(20.));
        hover(cx, 600., 400.);
        cx.run_until_parked();
        assert!(shown(cx).is_none());
    }
}
