//! The section content drawn as its own cached view. A frame that only moves the loader, shows a
//! tooltip or changes something around the content reuses the lists from the frame before
//! instead of building them again. Any change to the app redraws the content too.
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

impl Kuizer {
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
}
