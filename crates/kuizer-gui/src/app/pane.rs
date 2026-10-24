//! The section content drawn as its own cached view. A frame that only moves the loader, shows a
//! tooltip or changes something around the content reuses the lists from the frame before
//! instead of building them again. Any change to the app redraws the content too, except the ones
//! that go through `Chrome`.
//!
//! Tests leave it uncached, since GPUI's test `debug_bounds` misses elements a cached view reuses.

use gpui::{AnyView, StyleRefinement, Subscription, WeakEntity};

use super::*;

pub(super) struct Pane {
    app: WeakEntity<Kuizer>,
    _redraw: Subscription,
    #[cfg(test)]
    renders: usize,
}

impl Pane {
    fn new(app: &Entity<Kuizer>, cx: &mut Context<Self>) -> Self {
        Self {
            app: app.downgrade(),
            _redraw: cx.observe(app, |_, _, cx| cx.notify()),
            #[cfg(test)]
            renders: 0,
        }
    }
}

impl Render for Pane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        {
            self.renders += 1;
        }
        let Some(app) = self.app.upgrade() else {
            return div().into_any_element();
        };
        app.update(cx, |app, cx| {
            app.cursor_bounds.set(None);
            app.render_content(cx)
        })
    }
}

pub(super) struct Chrome;

impl Render for Chrome {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

impl Kuizer {
    pub(super) fn redraw_chrome(&self, cx: &mut Context<Self>) {
        match &self.chrome {
            Some(chrome) => {
                let id = chrome.entity_id();
                let app: &mut App = cx;
                app.notify(id);
            }
            None => cx.notify(),
        }
    }

    pub(super) fn chrome_view(&mut self, cx: &mut Context<Self>) -> Entity<Chrome> {
        self.chrome
            .get_or_insert_with(|| cx.new(|_| Chrome))
            .clone()
    }

    /// The content of the current section as a cached view that fills its parent.
    pub(super) fn content_pane(&mut self, cx: &mut Context<Self>) -> AnyView {
        let app = cx.entity();
        let pane = self
            .content
            .get_or_insert_with(|| cx.new(|cx| Pane::new(&app, cx)))
            .clone();
        let view = AnyView::from(pane);
        if self.cache_content {
            view.cached(StyleRefinement::default().size_full())
        } else {
            view
        }
    }
}

#[cfg(test)]
mod tests {
    use gpui::{TestAppContext, VisualTestContext};

    use super::*;
    use crate::app::tests::scanned;

    fn renders(view: &Entity<Kuizer>, cx: &mut VisualTestContext) -> usize {
        let pane = view.read_with(cx, |app, _| app.content.clone().expect("the content pane"));
        pane.read_with(cx, |pane, _| pane.renders)
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.update(|window, cx| window.draw(cx).clear());
    }

    #[gpui::test]
    fn the_loader_moves_without_drawing_the_content_again(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        cx.update(bind_keys);
        let (view, cx) = scanned(cx, dir.path(), Tab::Artifacts);
        view.update(cx, |app, cx| {
            app.cache_content = true;
            app.stale = Some(SystemTime::now());
            cx.notify();
        });
        draw(cx);
        let before = renders(&view, cx);
        let loader = view.read_with(cx, |app, _| app.loader.clone().expect("the loader"));
        cx.update(|_, cx| cx.notify(loader.entity_id()));
        draw(cx);
        assert_eq!(
            renders(&view, cx),
            before,
            "a loader frame reuses the content"
        );

        cx.simulate_keystrokes("j");
        draw(cx);
        assert!(
            renders(&view, cx) > before,
            "a change to the app draws it again"
        );
    }

    #[gpui::test]
    fn chrome_changes_and_unseen_progress_leave_the_content_alone(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        cx.update(bind_keys);
        let (view, cx) = scanned(cx, dir.path(), Tab::Artifacts);
        view.update(cx, |app, cx| {
            app.cache_content = true;
            cx.notify();
        });
        draw(cx);
        let before = renders(&view, cx);

        view.update(cx, |app, cx| app.redraw_chrome(cx));
        draw(cx);
        assert_eq!(renders(&view, cx), before, "a hover mark is chrome only");

        view.update(cx, |app, cx| {
            app.cancel = Some(Cancel::new());
            app.pending = HashSet::from([Part::Docker]);
            app.take(vec![Update::Activity(Box::default())], cx);
        });
        draw(cx);
        assert_eq!(
            renders(&view, cx),
            before,
            "progress of a section that is not open is not drawn"
        );

        view.update(cx, |app, cx| {
            app.tab = Tab::Docker;
            app.take(vec![Update::Activity(Box::default())], cx);
        });
        draw(cx);
        assert!(
            renders(&view, cx) > before,
            "the open section shows its own progress"
        );
    }
}
