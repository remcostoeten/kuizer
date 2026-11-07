//! The focus ring over the window's controls. Tab and Shift+Tab move it through the buttons,
//! segments, toggles and sidebar items in window order, the arrow keys step it along, Enter or
//! Space presses the control it is on and Esc gives the keyboard back to the list.
//!
//! GPUI focus stays on the root view the whole time. A focused GPUI element that stops being
//! drawn leaves the window with no key listener on its dispatch path, so a control that
//! disappears while it has the ring would take the keyboard with it.

use gpui::Keystroke;

use super::*;

/// What a control does when it is clicked, or pressed with Enter or Space while it has the ring.
pub(super) type Press = Rc<dyn Fn(&mut Kuizer, &mut Window, &mut Context<Kuizer>)>;

/// The parts of the window that hold controls, in Tab order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Region {
    Titlebar,
    Sidebar,
    Content,
    Statusbar,
    Toast,
}

const REGIONS: usize = 5;

/// How a key moves the ring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Move {
    /// Tab and Shift+Tab: past either end the ring leaves the controls for the list, and the
    /// next step brings it back at the other end.
    Cycle { back: bool },
    /// The arrow keys: the ring stops at the first and the last control.
    Step { back: bool },
}

struct Stop {
    id: ElementId,
    press: Press,
}

/// The controls of the frame on screen. A region is emptied when it is drawn again, so the
/// content keeps its controls while its cached view is reused.
#[derive(Default)]
pub(super) struct Stops {
    regions: [Vec<Stop>; REGIONS],
    current: usize,
    scroll: Option<ScrollHandle>,
}

impl Stops {
    /// Starts drawing `region`: its old controls go, and the controls added next belong to it.
    pub(super) fn enter(&mut self, region: Region) {
        self.current = region as usize;
        self.regions[self.current].clear();
        self.scroll = None;
    }

    /// The scroll container the controls added next sit in, so the ring can scroll them into
    /// view.
    pub(super) fn within(&mut self, scroll: Option<ScrollHandle>) {
        self.scroll = scroll;
    }

    fn push(&mut self, id: ElementId, press: Press) -> Option<ScrollHandle> {
        self.regions[self.current].push(Stop { id, press });
        self.scroll.clone()
    }

    fn ids(&self) -> impl Iterator<Item = &ElementId> {
        self.regions.iter().flatten().map(|stop| &stop.id)
    }

    fn press(&self, id: &ElementId) -> Option<Press> {
        self.regions
            .iter()
            .flatten()
            .find(|stop| stop.id == *id)
            .map(|stop| Rc::clone(&stop.press))
    }

    /// Where the ring lands after `motion` from `from`, or `None` for the list.
    fn moved(&self, from: Option<&ElementId>, motion: Move) -> Option<ElementId> {
        let ids: Vec<&ElementId> = self.ids().collect();
        let last = ids.len().checked_sub(1)?;
        let at = from.and_then(|from| ids.iter().position(|id| *id == from));
        let index = match (motion, at) {
            (Move::Cycle { back: false }, None) => Some(0),
            (Move::Cycle { back: true }, None) => Some(last),
            (Move::Cycle { back: false }, Some(at)) => (at < last).then_some(at + 1),
            (Move::Cycle { back: true }, Some(at)) => at.checked_sub(1),
            (Move::Step { .. }, None) => None,
            (Move::Step { back: false }, Some(at)) => Some((at + 1).min(last)),
            (Move::Step { back: true }, Some(at)) => Some(at.saturating_sub(1)),
        };
        index.map(|index| ids[index].clone())
    }
}

/// Whether a key is Tab or Shift+Tab, the keys that move the ring from anywhere.
pub(super) fn is_tab(keystroke: &Keystroke) -> bool {
    let modifiers = keystroke.modifiers;
    keystroke.key == "tab" && !modifiers.control && !modifiers.alt && !modifiers.platform
}

impl Kuizer {
    /// Starts drawing a region of the window, see `Stops::enter`.
    pub(super) fn region(&self, region: Region) {
        self.stops.borrow_mut().enter(region);
    }

    /// Makes `control` a Tab stop that runs `press` when it is clicked, or when it has the ring
    /// and Enter or Space is pressed.
    pub(super) fn control(
        &self,
        control: Stateful<Div>,
        cx: &Context<Self>,
        press: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        self.control_if(true, control, cx, press)
    }

    /// Like `control`, for a control that can be disabled. A disabled control is skipped by Tab
    /// and the arrow keys; a click still reaches `press`.
    pub(super) fn control_if(
        &self,
        enabled: bool,
        control: Stateful<Div>,
        cx: &Context<Self>,
        press: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        let press: Press = Rc::new(press);
        let clicked = Rc::clone(&press);
        self.stop(enabled, control, press)
            .on_click(cx.listener(move |app, _: &ClickEvent, window, cx| clicked(app, window, cx)))
    }

    /// A button that is a Tab stop while it is enabled, see `control_if`.
    pub(super) fn button(
        &self,
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        kind: ButtonKind,
        enabled: bool,
        cx: &Context<Self>,
        press: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        self.control_if(enabled, ui::button(id, label, kind, enabled), cx, press)
    }

    /// Adds `control` to the Tab order without handling its clicks, for a control whose click
    /// does something other than Enter.
    pub(super) fn reachable(
        &self,
        control: Stateful<Div>,
        press: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        self.stop(true, control, Rc::new(press))
    }

    fn stop(&self, enabled: bool, mut control: Stateful<Div>, press: Press) -> Stateful<Div> {
        let Some(id) = control
            .interactivity()
            .element_id
            .clone()
            .filter(|_| enabled)
        else {
            return control;
        };
        let ringed = self.ring.as_ref() == Some(&id) && !self.typing();
        let scroll = self.stops.borrow_mut().push(id, press);
        control.when(ringed, |this| {
            this.child(outline(scroll, Rc::clone(&self.reveal_ring)))
        })
    }

    /// Whether the ring is on a control of the frame on screen.
    pub(super) fn ringed(&self) -> bool {
        self.ring
            .as_ref()
            .is_some_and(|ring| self.stops.borrow().ids().any(|id| id == ring))
    }

    /// Handles a key for the ring: Tab from anywhere, and while a control has the ring, Enter,
    /// Space, Esc and the arrows. Returns false for a key the rest of the app should handle; any
    /// other key gives the keyboard back to the list first.
    pub(super) fn ring_key(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let modifiers = keystroke.modifiers;
        let plain = !modifiers.control && !modifiers.alt && !modifiers.platform;
        if !self.ringed() {
            self.ring = None;
        }
        if is_tab(keystroke) {
            self.move_ring(Move::Cycle {
                back: modifiers.shift,
            });
            cx.notify();
            return true;
        }
        let Some(ring) = self.ring.clone() else {
            return false;
        };
        let back = match keystroke.key.as_str() {
            "left" | "up" if plain && !modifiers.shift => Some(true),
            "right" | "down" if plain && !modifiers.shift => Some(false),
            _ => None,
        };
        if let Some(back) = back {
            self.move_ring(Move::Step { back });
        } else if matches!(keystroke.key.as_str(), "enter" | "space") && plain && !modifiers.shift {
            let press = self.stops.borrow().press(&ring);
            if let Some(press) = press {
                press(self, window, cx);
            }
        } else if keystroke.key == "escape" {
            self.ring = None;
        } else {
            self.ring = None;
            cx.notify();
            return false;
        }
        cx.notify();
        true
    }

    fn move_ring(&mut self, motion: Move) {
        self.ring = self.stops.borrow().moved(self.ring.as_ref(), motion);
        self.reveal_ring.set(self.ring.is_some());
    }
}

/// The outline on the control that has the ring. It sits inside the control, so a scroll
/// container never clips it, and scrolls the control into view after the ring moves to it.
fn outline(scroll: Option<ScrollHandle>, reveal: Rc<Cell<bool>>) -> Div {
    div()
        .debug_selector(|| "ring".to_owned())
        .absolute()
        .inset_0()
        .rounded(theme::radius_control())
        .border_1()
        .border_color(theme::focus())
        .when_some(scroll, |this, scroll| {
            this.child(
                canvas(
                    move |area, window, _| {
                        if reveal.take() {
                            keep_visible(&scroll, area, window);
                        }
                    },
                    |_, (), _, _| {},
                )
                .size_full(),
            )
        })
}

#[cfg(test)]
mod tests {
    use gpui::{TestAppContext, VisualTestContext};

    use super::*;
    use crate::app::tests::{open, scanned, type_text};

    fn stops(regions: &[(Region, &[&'static str])]) -> Stops {
        let mut stops = Stops::default();
        for (region, ids) in regions {
            stops.enter(*region);
            for id in *ids {
                stops.push(ElementId::from(*id), Rc::new(|_, _, _| {}));
            }
        }
        stops
    }

    #[test]
    fn tab_follows_the_window_order_and_passes_through_the_list() {
        let stops = stops(&[
            (Region::Statusbar, &["trash"]),
            (Region::Content, &["filter", "safe"]),
            (Region::Titlebar, &["search"]),
        ]);
        let ring = |from: Option<&'static str>, motion| {
            stops.moved(from.map(ElementId::from).as_ref(), motion)
        };
        let tab = Move::Cycle { back: false };
        let shift_tab = Move::Cycle { back: true };
        assert_eq!(ring(None, tab), Some("search".into()));
        assert_eq!(ring(Some("search"), tab), Some("filter".into()));
        assert_eq!(ring(Some("safe"), tab), Some("trash".into()));
        assert_eq!(ring(Some("trash"), tab), None, "past the end is the list");
        assert_eq!(ring(None, shift_tab), Some("trash".into()));
        assert_eq!(ring(Some("search"), shift_tab), None);
        assert_eq!(
            ring(Some("trash"), Move::Step { back: false }),
            Some("trash".into())
        );
        assert_eq!(ring(None, Move::Step { back: false }), None);
    }

    fn ring(view: &Entity<Kuizer>, cx: &mut VisualTestContext) -> Option<ElementId> {
        view.read_with(cx, |app, _| app.ring.clone())
    }

    fn tab_to(view: &Entity<Kuizer>, cx: &mut VisualTestContext, id: &'static str) {
        for _ in 0..80 {
            if ring(view, cx) == Some(id.into()) {
                return;
            }
            cx.simulate_keystrokes("tab");
        }
        panic!("Tab never reached {id}");
    }

    #[gpui::test]
    fn the_start_screen_works_from_the_keyboard(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (view, cx) = open(cx, dir.path().join("recent"));
        cx.update(|window, cx| window.draw(cx).clear());

        cx.simulate_keystrokes("tab");
        assert_eq!(
            ring(&view, cx),
            Some("search".into()),
            "the titlebar comes first"
        );
        cx.simulate_keystrokes("tab right");
        assert_eq!(ring(&view, cx), Some("scope-home".into()));
        cx.update(|window, cx| window.draw(cx).clear());
        assert!(cx.debug_bounds("ring").is_some(), "the ring is drawn");
        cx.simulate_keystrokes("enter");
        view.read_with(cx, |app, _| assert!(app.whole_home));

        type_text(cx, "~");
        view.read_with(cx, |app, _| {
            assert_eq!(app.ring, None, "typing goes back to the folder field");
            assert_eq!(app.folder, "~");
            assert!(!app.whole_home);
        });

        cx.simulate_keystrokes("shift-tab");
        assert_eq!(
            ring(&view, cx),
            Some("opt-one-fs".into()),
            "Shift+Tab wraps"
        );
        let before = view.read_with(cx, |app, _| app.options.one_file_system);
        cx.simulate_keystrokes("space");
        view.read_with(cx, |app, _| assert_ne!(app.options.one_file_system, before));
        cx.simulate_keystrokes("escape");
        assert_eq!(ring(&view, cx), None);

        cx.simulate_keystrokes("ctrl-,");
        view.read_with(cx, |app, _| {
            assert!(
                app.settings.is_some(),
                "commands the field leaves alone still run"
            )
        });
    }

    #[gpui::test]
    fn tab_reaches_the_sidebar_and_a_control_that_goes_away_keeps_the_keyboard(
        cx: &mut TestAppContext,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let (view, cx) = scanned(cx, dir.path(), Tab::Overview);

        tab_to(&view, cx, Tab::History.label());
        cx.simulate_keystrokes("enter");
        view.read_with(cx, |app, _| assert_eq!(app.tab, Tab::History));
        cx.simulate_keystrokes("j");
        assert_eq!(ring(&view, cx), None, "a list key gives the keyboard back");

        tab_to(&view, cx, "new-scan");
        cx.simulate_keystrokes("enter");
        view.read_with(cx, |app, _| assert!(matches!(app.phase, Phase::Start)));
        cx.simulate_keystrokes("tab");
        assert_eq!(
            ring(&view, cx),
            Some("search".into()),
            "with its button gone, Tab starts again from the top"
        );
    }
}
