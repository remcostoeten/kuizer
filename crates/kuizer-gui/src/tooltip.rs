use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    Animation, AnimationExt, AnyView, App, Bounds, Context, Corner, DispatchPhase, Entity,
    FontWeight, Global, IntoElement, KeyDownEvent, MouseDownEvent, MouseMoveEvent, ParentElement,
    Pixels, Point, Render, ScrollWheelEvent, SharedString, Styled, Task, Window, anchored, canvas,
    deferred, div, point, prelude::*, px,
};
use kuizer::output::display_path;

use crate::glossary::Term;
use crate::motion;
use crate::theme::{self, unit};
use crate::ui;

const DELAY: Duration = Duration::from_millis(500);
const ENTER: Duration = Duration::from_millis(125);
const WARM: Duration = Duration::from_millis(300);
const RISE: f32 = 3.;
const GAP: f32 = 6.;
const ROOM: f32 = 160.;

type Build = Rc<dyn Fn(&mut Window, &mut App) -> AnyView>;

pub struct Typeface(pub SharedString);

impl Global for Typeface {}

struct Shared(Entity<Layer>);

impl Global for Shared {}

#[derive(Clone)]
pub struct Tooltip {
    title: Option<SharedString>,
    text: Option<SharedString>,
    keys: Vec<SharedString>,
    rows: Vec<(SharedString, SharedString)>,
}

impl Tooltip {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            title: None,
            text: Some(text.into()),
            keys: Vec::new(),
            rows: Vec::new(),
        }
    }

    pub fn details(title: impl Into<SharedString>) -> Self {
        Self {
            title: Some(title.into()),
            text: None,
            keys: Vec::new(),
            rows: Vec::new(),
        }
    }

    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn keys(mut self, keys: &[&'static str]) -> Self {
        self.keys = keys.iter().map(|key| SharedString::from(*key)).collect();
        self
    }

    pub fn row(mut self, label: impl Into<SharedString>, value: impl Into<SharedString>) -> Self {
        self.rows.push((label.into(), value.into()));
        self
    }

    pub fn note(mut self, text: impl Into<SharedString>) -> Self {
        self.text = Some(text.into());
        self
    }

    pub fn build(self) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
        move |_, cx| cx.new(|_| self.clone()).into()
    }
}

impl Render for Tooltip {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let font = cx.try_global::<Typeface>().map(|face| face.0.clone());
        let heading = self.title.clone().map(|title| {
            div()
                .flex()
                .items_center()
                .gap(unit(12.))
                .child(
                    div()
                        .flex_1()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::text())
                        .child(title),
                )
                .children(self.keys.iter().cloned().map(ui::kbd))
        });
        let keys_inline = self.title.is_none() && !self.keys.is_empty();
        div()
            .flex()
            .flex_col()
            .gap(unit(4.))
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
            .children(heading)
            .when_some(self.text.clone(), |this, text| {
                this.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(unit(12.))
                        .child(div().flex_1().child(text))
                        .when(keys_inline, |this| {
                            this.children(self.keys.iter().cloned().map(ui::kbd))
                        }),
                )
            })
            .when(!self.rows.is_empty(), |this| {
                this.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(unit(2.))
                        .children(self.rows.iter().map(|(label, value)| {
                            div()
                                .flex()
                                .gap(unit(16.))
                                .child(
                                    div()
                                        .flex_1()
                                        .text_color(theme::faint())
                                        .child(label.clone()),
                                )
                                .child(div().text_color(theme::text()).child(value.clone()))
                        })),
                )
            })
    }
}

#[derive(Clone, Copy, PartialEq)]
struct Anchor {
    bounds: Bounds<Pixels>,
    x: Pixels,
}

struct Shown {
    anchor: Anchor,
    view: AnyView,
    animate: bool,
    epoch: usize,
}

#[derive(Default)]
pub struct Layer {
    shown: Option<Shown>,
    pending: Option<(Anchor, Task<()>)>,
    closed: Option<Instant>,
    epoch: usize,
}

impl Layer {
    fn request(
        &mut self,
        anchor: Anchor,
        build: Build,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .shown
            .as_ref()
            .is_some_and(|shown| shown.anchor.bounds == anchor.bounds)
        {
            return;
        }
        let warm =
            self.shown.is_some() || self.closed.is_some_and(|closed| closed.elapsed() < WARM);
        if warm {
            self.show(anchor, &build, false, window, cx);
            return;
        }
        let task = cx.spawn_in(window, async move |layer, cx| {
            cx.background_executor().timer(DELAY).await;
            layer
                .update_in(cx, |layer, window, cx| {
                    if layer
                        .pending
                        .as_ref()
                        .is_some_and(|(pending, _)| pending.bounds == anchor.bounds)
                    {
                        layer.pending = None;
                        layer.show(anchor, &build, true, window, cx);
                    }
                })
                .ok();
        });
        self.pending = Some((anchor, task));
    }

    fn show(
        &mut self,
        anchor: Anchor,
        build: &Build,
        animate: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pending = None;
        self.epoch += 1;
        self.shown = Some(Shown {
            anchor,
            view: build(window, cx),
            animate: animate && self.closed.is_none_or(|closed| closed.elapsed() >= WARM),
            epoch: self.epoch,
        });
        cx.notify();
    }

    fn dismiss(&mut self, cx: &mut Context<Self>) {
        self.pending = None;
        if self.shown.take().is_some() {
            self.closed = Some(Instant::now());
            cx.notify();
        }
    }

    fn track(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        if self
            .pending
            .as_ref()
            .is_some_and(|(pending, _)| !pending.bounds.contains(&position))
        {
            self.pending = None;
        }
        if self
            .shown
            .as_ref()
            .is_some_and(|shown| !shown.anchor.bounds.contains(&position))
        {
            self.dismiss(cx);
        }
    }
}

impl Render for Layer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity().downgrade();
        let active = self.shown.is_some() || self.pending.is_some();
        let listener = canvas(
            |_, _, _| {},
            move |_, (), window, _| {
                if !active {
                    return;
                }
                let moved = this.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture {
                        moved
                            .update(cx, |layer, cx| layer.track(event.position, cx))
                            .ok();
                        window.refresh();
                    }
                });
                let pressed = this.clone();
                window.on_mouse_event(move |_: &MouseDownEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture {
                        pressed.update(cx, |layer, cx| layer.dismiss(cx)).ok();
                        window.refresh();
                    }
                });
                let scrolled = this.clone();
                window.on_mouse_event(move |_: &ScrollWheelEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture {
                        scrolled.update(cx, |layer, cx| layer.dismiss(cx)).ok();
                        window.refresh();
                    }
                });
                let typed = this;
                window.on_key_event(move |_: &KeyDownEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture {
                        typed.update(cx, |layer, cx| layer.dismiss(cx)).ok();
                        window.refresh();
                    }
                });
            },
        )
        .absolute()
        .size_0();
        let card = self.shown.as_ref().map(|shown| {
            let bounds = shown.anchor.bounds;
            let above = bounds.bottom() + unit(ROOM).to_pixels(window.rem_size())
                > window.viewport_size().height;
            let gap = unit(GAP).to_pixels(window.rem_size());
            let x = shown.anchor.x.max(bounds.left());
            let (corner, y) = if above {
                (Corner::BottomLeft, bounds.top() - gap)
            } else {
                (Corner::TopLeft, bounds.bottom() + gap)
            };
            let rise = if motion::reduced() { 0. } else { RISE };
            let direction = if above { -rise } else { rise };
            let content = div().relative().child(shown.view.clone());
            let content = if shown.animate {
                content
                    .with_animation(
                        ("tooltip-enter", shown.epoch),
                        Animation::new(ENTER).with_easing(motion::ease_out),
                        move |card, progress| {
                            card.opacity(progress)
                                .top(unit(direction * (1. - progress)))
                        },
                    )
                    .into_any_element()
            } else {
                content.into_any_element()
            };
            deferred(
                anchored()
                    .anchor(corner)
                    .position(point(x, y))
                    .snap_to_window_with_margin(px(8.))
                    .child(content),
            )
            .with_priority(1)
        });
        div().child(listener).children(card)
    }
}

pub fn layer(cx: &mut App) -> Entity<Layer> {
    if let Some(shared) = cx.try_global::<Shared>() {
        return shared.0.clone();
    }
    let layer = cx.new(|_| Layer::default());
    cx.set_global(Shared(layer.clone()));
    layer
}

pub trait Tip: StatefulInteractiveElement + ParentElement + Sized {
    fn tip(self, build: impl Fn(&mut Window, &mut App) -> AnyView + 'static) -> Self {
        let bounds = Rc::new(Cell::new(None::<Bounds<Pixels>>));
        let probe = {
            let bounds = Rc::clone(&bounds);
            canvas(move |area, _, _| bounds.set(Some(area)), |_, (), _, _| {})
                .absolute()
                .top_0()
                .left_0()
                .size_full()
        };
        let build: Build = Rc::new(build);
        self.child(probe).on_hover(move |hovered, window, cx| {
            let Some(area) = bounds.get() else {
                return;
            };
            let layer = layer(cx);
            layer.update(cx, |layer, cx| {
                let anchor = Anchor {
                    bounds: area,
                    x: window.mouse_position().x,
                };
                if *hovered {
                    layer.request(anchor, Rc::clone(&build), window, cx);
                } else {
                    layer.track(window.mouse_position(), cx);
                }
            });
        })
    }
}

impl<E: StatefulInteractiveElement + ParentElement> Tip for E {}

pub fn plain(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView {
    Tooltip::new(text).build()
}

pub fn path(path: &Path) -> impl Fn(&mut Window, &mut App) -> AnyView + use<> {
    plain(display_path(path))
}

pub fn term(term: Term) -> impl Fn(&mut Window, &mut App) -> AnyView {
    Tooltip::new(term.text()).title(term.title()).build()
}

pub fn titled(
    title: impl Into<SharedString>,
    text: impl Into<SharedString>,
) -> impl Fn(&mut Window, &mut App) -> AnyView {
    Tooltip::new(text).title(title).build()
}

#[cfg(test)]
mod tests {
    use gpui::{Modifiers, TestAppContext, VisualTestContext};

    use super::*;

    struct Target;

    impl Render for Target {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .child(
                    div()
                        .id("first")
                        .w(px(200.))
                        .h(px(40.))
                        .tip(term(Term::Review))
                        .child("review"),
                )
                .child(
                    div()
                        .id("second")
                        .w(px(200.))
                        .h(px(40.))
                        .tip(Tooltip::new("Starts the scan").keys(&["Enter"]).build())
                        .child("start"),
                )
                .child(layer(cx))
        }
    }

    fn hover(cx: &mut VisualTestContext, x: f32, y: f32) {
        cx.simulate_event(MouseMoveEvent {
            position: point(px(x), px(y)),
            pressed_button: None,
            modifiers: Modifiers::default(),
        });
    }

    fn shown(cx: &mut VisualTestContext) -> Option<Bounds<Pixels>> {
        cx.debug_bounds("tooltip")
    }

    fn visible(cx: &mut VisualTestContext) -> bool {
        cx.update(|_, cx| layer(cx).read(cx).shown.is_some())
    }

    fn open(cx: &mut TestAppContext) -> &mut VisualTestContext {
        let (_view, cx) = cx.add_window_view(|_, _| Target);
        cx.run_until_parked();
        cx
    }

    #[gpui::test]
    fn a_tooltip_waits_then_opens_below_its_trigger(cx: &mut TestAppContext) {
        let cx = open(cx);
        hover(cx, 20., 20.);
        cx.executor().advance_clock(Duration::from_millis(300));
        cx.run_until_parked();
        assert!(!visible(cx));
        cx.executor().advance_clock(Duration::from_millis(300));
        cx.run_until_parked();
        let bounds = shown(cx).expect("the tooltip is painted");
        assert!(bounds.origin.y >= px(40.));
        assert!(bounds.origin.x >= px(20.));
    }

    #[gpui::test]
    fn leaving_the_trigger_closes_the_tooltip(cx: &mut TestAppContext) {
        let cx = open(cx);
        hover(cx, 20., 20.);
        cx.executor().advance_clock(DELAY);
        cx.run_until_parked();
        assert!(shown(cx).is_some());
        hover(cx, 600., 400.);
        cx.run_until_parked();
        assert!(!visible(cx));
        assert!(shown(cx).is_none());
    }

    #[gpui::test]
    fn the_next_tooltip_opens_at_once(cx: &mut TestAppContext) {
        let cx = open(cx);
        hover(cx, 20., 20.);
        cx.executor().advance_clock(DELAY);
        cx.run_until_parked();
        hover(cx, 20., 60.);
        cx.run_until_parked();
        let bounds = shown(cx).expect("the second tooltip is painted at once");
        assert!(bounds.origin.y >= px(80.));
    }

    #[gpui::test]
    fn a_click_closes_the_tooltip(cx: &mut TestAppContext) {
        let cx = open(cx);
        hover(cx, 20., 20.);
        cx.executor().advance_clock(DELAY);
        cx.run_until_parked();
        assert!(visible(cx));
        cx.simulate_click(point(px(20.), px(20.)), Modifiers::default());
        assert!(!visible(cx));
    }
}
