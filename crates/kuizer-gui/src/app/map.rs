//! The space map in the Overview: a squarified treemap of the reclaimable items, with zooming
//! into folders, a breadcrumb back out, selection and the item menu.

use gpui::{Animation, AnimationExt, ElementId, Modifiers, relative};

use super::*;
use crate::squarify::{self, Direction, Rect};
use crate::treemap::{self, Item, Kind, Leaf, Node};

/// The size the layout assumes before the map has been painted once.
const FALLBACK: (f32, f32) = (960., 540.);
const ZOOM: Duration = Duration::from_millis(180);
/// The smallest tile, in pixels, that shows its name and size.
const LABEL: (f32, f32) = (72., 36.);
/// The smallest tile, in pixels, that shows its name.
const NAME: (f32, f32) = (40., 18.);
pub(super) const MAP_ONLY: &str = "Available in the space map";

type CountedFor = (u64, usize, usize);

/// What the space map shows and where it is zoomed to.
#[derive(Default)]
pub(super) struct SpaceMap {
    pub shown: bool,
    pub tree: Option<Node>,
    /// The keys of the folders opened from the top, outermost first.
    pub path: Vec<String>,
    /// The key of the focus tile in the current level, or the first tile when unset.
    pub focus: Option<String>,
    pub bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    pub epoch: usize,
    pub dirty: bool,
    pub built: usize,
    pub selected: Cell<Option<(CountedFor, u64)>>,
}

impl Kuizer {
    /// Whether the space map is on screen and has the keyboard.
    pub(super) fn map_active(&self) -> bool {
        self.tab == Tab::Overview && self.map.shown && self.report().is_some()
    }

    pub(super) fn refresh_map(&mut self) {
        if self.map.shown {
            self.rebuild_map();
        } else {
            self.map.dirty = true;
        }
    }

    pub(super) fn ensure_map(&mut self) {
        if self.map.dirty && self.map.shown {
            self.rebuild_map();
        }
    }

    /// Builds the map again from the report on screen, keeping as much of the zoom path and
    /// the focus tile as still exists.
    pub(super) fn rebuild_map(&mut self) {
        self.map.dirty = false;
        let Some(report) = self.report() else {
            self.map.tree = None;
            return;
        };
        let leaf = |source: Source, candidate: &Candidate| Leaf {
            path: candidate.path.clone(),
            bytes: candidate.usage.allocated,
            item: Item {
                source,
                id: candidate.id,
                category: candidate.rule.category,
                safety: candidate.rule.safety,
            },
        };
        let artifacts = report
            .candidates
            .iter()
            .map(|candidate| leaf(Source::Artifact, candidate))
            .collect();
        let caches = report
            .caches
            .iter()
            .map(|(_, candidate)| leaf(Source::Cache, candidate))
            .collect();
        let tree = treemap::build(&report.roots, artifacts, caches);
        let depth = tree.trail(&self.map.path).len() - 1;
        self.map.path.truncate(depth);
        self.map.tree = Some(tree);
        self.map.built += 1;
        let kept = self.map.focus.as_deref().is_some_and(|focus| {
            self.map_level()
                .is_some_and(|level| level.child(focus).is_some())
        });
        if !kept {
            self.map.focus = None;
        }
    }

    /// The top of the map and every level opened from it.
    pub(super) fn map_trail(&self) -> Vec<&Node> {
        self.map
            .tree
            .as_ref()
            .map(|tree| tree.trail(&self.map.path))
            .unwrap_or_default()
    }

    pub(super) fn map_level(&self) -> Option<&Node> {
        self.map_trail().pop()
    }

    fn map_focus_index(&self) -> Option<usize> {
        let level = self.map_level()?;
        if level.children.is_empty() {
            return None;
        }
        let at = self
            .map
            .focus
            .as_deref()
            .and_then(|focus| level.children.iter().position(|child| child.key == focus));
        Some(at.unwrap_or(0))
    }

    pub(super) fn map_focused(&self) -> Option<&Node> {
        self.map_level()?.children.get(self.map_focus_index()?)
    }

    /// The focus tile as a row the item menu, Reveal and Copy path can act on.
    pub(super) fn map_target(&self) -> Option<Target> {
        let node = self.map_focused()?;
        let item = node.item()?;
        Some(Target::Item(item.source, item.id, node.path.clone()?))
    }

    fn map_selected(&self, level: &Node) -> u64 {
        let key = (self.selection.generation(), self.map.built, self.map.epoch);
        if let Some((counted, bytes)) = self.map.selected.get()
            && counted == key
        {
            return bytes;
        }
        let bytes = level
            .leaves()
            .iter()
            .filter(|(item, _)| self.selection.contains(&(item.source, item.id)))
            .map(|(_, bytes)| bytes)
            .sum();
        self.map.selected.set(Some((key, bytes)));
        bytes
    }

    /// Opens a folder tile of the current level as the new level.
    pub(super) fn map_open(&mut self, key: &str) {
        if self
            .map_level()
            .and_then(|level| level.child(key))
            .is_some_and(Node::opens)
        {
            self.map.path.push(key.to_owned());
            self.map.focus = None;
            self.map.epoch += 1;
        }
    }

    /// Zooms out to the level at `depth`, zero being the top, with the focus on the folder the
    /// map came out of.
    pub(super) fn map_back(&mut self, depth: usize) {
        if depth >= self.map.path.len() {
            return;
        }
        self.map.focus = self.map.path.get(depth).cloned();
        self.map.path.truncate(depth);
        self.map.epoch += 1;
    }

    pub(super) fn map_move(&mut self, direction: Direction) {
        let (Some(level), Some(from)) = (self.map_level(), self.map_focus_index()) else {
            return;
        };
        let rects = self.map_rects(level);
        if let Some(next) = squarify::neighbor(&rects, from, direction) {
            self.map.focus = Some(level.children[next].key.clone());
        }
    }

    pub(super) fn map_toggle(&mut self, cx: &mut Context<Self>) {
        if let Some(item) = self.map_focused().and_then(Node::item) {
            self.toggle(item.source, item.id, cx);
        }
    }

    fn map_size(&self) -> (f32, f32) {
        self.map
            .bounds
            .get()
            .map(|bounds| (f32::from(bounds.size.width), f32::from(bounds.size.height)))
            .filter(|(width, height)| *width > 0. && *height > 0.)
            .unwrap_or(FALLBACK)
    }

    fn map_rects(&self, level: &Node) -> Vec<Rect> {
        let (width, height) = self.map_size();
        let sizes: Vec<u64> = level.children.iter().map(|child| child.bytes).collect();
        squarify::squarify(&sizes, Rect::new(0., 0., width, height))
    }

    fn click_tile(&mut self, key: &str, modifiers: Modifiers, cx: &mut Context<Self>) {
        let Some(node) = self.map_level().and_then(|level| level.child(key)) else {
            return;
        };
        let (opens, item) = (node.opens(), node.item());
        self.filtering = false;
        if modifiers.secondary() || modifiers.shift {
            if let Some(item) = item
                && !self.busy()
            {
                self.toggle(item.source, item.id, cx);
            }
            self.map.focus = Some(key.to_owned());
        } else if opens {
            self.map_open(key);
        } else {
            self.map.focus = Some(key.to_owned());
        }
        cx.notify();
    }

    fn right_click_tile(&mut self, key: &str, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(node) = self.map_level().and_then(|level| level.child(key)) else {
            return;
        };
        let (Some(item), Some(path)) = (node.item(), node.path.clone()) else {
            return;
        };
        self.map.focus = Some(key.to_owned());
        self.open_menu(
            Target::Item(item.source, item.id, path),
            position,
            false,
            cx,
        );
    }

    /// The Summary and Space map switch at the top of the Overview.
    pub(super) fn overview_switch(&self, cx: &mut Context<Self>) -> Div {
        let key = commands::shortcut(&Command::SpaceMap).unwrap_or_default();
        ui::segments()
            .child(
                ui::segment("view-summary", "Summary", !self.map.shown)
                    .debug_selector(|| "view-summary".to_owned())
                    .tip(tooltip::plain("Totals and charts of the scan"))
                    .on_click(cx.listener(|app, _: &ClickEvent, _, cx| {
                        app.map.shown = false;
                        cx.notify();
                    })),
            )
            .child(
                ui::segment("view-map", "Space map", self.map.shown)
                    .debug_selector(|| "view-map".to_owned())
                    .tip(
                        Tooltip::details("Space map")
                            .row(key, "show or hide")
                            .note("Every reclaimable item as a tile sized by its space, inside the folders that hold it. Click a folder to open it.")
                            .build(),
                    )
                    .on_click(cx.listener(|app, _: &ClickEvent, _, cx| {
                        app.map.shown = true;
                        app.ensure_map();
                        cx.notify();
                    })),
            )
    }

    pub(super) fn render_map(&self, cx: &mut Context<Self>) -> AnyElement {
        let trail = self.map_trail();
        let Some(level) = trail.last().copied() else {
            return div().into_any_element();
        };
        let selected = self.map_selected(level);
        let depth = trail.len() - 1;
        let crumbs = trail.iter().enumerate().flat_map(|(index, node)| {
            let current = index == depth;
            let separator = (index > 0).then(|| {
                div()
                    .flex_none()
                    .text_color(theme::faint())
                    .child("/")
                    .into_any_element()
            });
            let crumb = div()
                .id(("map-crumb", index))
                .debug_selector(move || format!("map-crumb-{index}"))
                .flex_none()
                .max_w(unit(size::LABEL))
                .truncate()
                .px(unit(space::XS))
                .rounded(theme::radius_control())
                .text_color(if current {
                    theme::text()
                } else {
                    theme::muted()
                })
                .when(!current, |this| {
                    this.cursor_pointer()
                        .hover(|style| style.bg(theme::hover()).text_color(theme::text()))
                        .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                            app.map_back(index);
                            cx.notify();
                        }))
                })
                .when_some(node.path.as_ref(), |this, path| {
                    this.tip(tooltip::path(path))
                })
                .child(node.name.clone())
                .into_any_element();
            separator.into_iter().chain([crumb])
        });
        let crumbs: Vec<AnyElement> = crumbs.collect();
        let switch = self.overview_switch(cx);
        let header = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(unit(space::L))
            .child(switch)
            .child(
                div()
                    .id("map-trail")
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap(unit(space::XXS))
                    .overflow_hidden()
                    .children(crumbs),
            )
            .child(
                div()
                    .id("map-total")
                    .debug_selector(|| "map-total".to_owned())
                    .flex_none()
                    .text_color(theme::muted())
                    .tip(
                        Tooltip::details(level.name.clone())
                            .row("Size", theme::bytes(level.bytes))
                            .row("Items", level.items.to_string())
                            .row("Selected", theme::bytes(selected))
                            .build(),
                    )
                    .child(format!(
                        "{} selected of {}, {}",
                        theme::bytes(selected),
                        theme::bytes(level.bytes),
                        plural(level.items, "item")
                    )),
            );
        let body = if level.children.is_empty() {
            centered(ui::empty("No reclaimable items found"))
        } else {
            self.render_tiles(level, cx)
        };
        let legend = div()
            .flex()
            .flex_none()
            .flex_wrap()
            .items_center()
            .gap_x(unit(space::XL))
            .gap_y(unit(space::XS))
            .text_size(unit(font::CAPTION))
            .text_color(theme::faint())
            .children(Category::ALL.into_iter().map(|category| {
                div()
                    .flex()
                    .items_center()
                    .gap(unit(space::S))
                    .child(
                        div()
                            .size(unit(size::LEGEND))
                            .flex_none()
                            .bg(theme::tile(category))
                            .border_t_2()
                            .border_color(theme::tile_edge(category)),
                    )
                    .child(category.as_str())
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(unit(space::S))
                    .child(
                        div()
                            .size(unit(size::LEGEND))
                            .flex_none()
                            .bg(theme::tile_folder())
                            .border_1()
                            .border_color(theme::border_strong()),
                    )
                    .child("folder"),
            )
            .child(div().flex_1())
            .child("Click a folder to open it. Ctrl+click or Space selects an item. Backspace goes back.");
        div()
            .flex()
            .flex_col()
            .gap(unit(space::L))
            .size_full()
            .p(unit(space::XL))
            .child(header)
            .child(body)
            .child(legend)
            .into_any_element()
    }

    fn render_tiles(&self, level: &Node, cx: &mut Context<Self>) -> AnyElement {
        let (width, height) = self.map_size();
        let rects = self.map_rects(level);
        let focus = self.map_focus_index();
        let tiles: Vec<AnyElement> = level
            .children
            .iter()
            .zip(rects)
            .enumerate()
            .filter(|(_, (_, rect))| rect.area() > 0.)
            .map(|(index, (node, rect))| {
                self.render_tile(index, node, rect, (width, height), focus == Some(index), cx)
            })
            .collect();
        let bounds = Rc::clone(&self.map.bounds);
        let probe = canvas(
            move |area, window, _| {
                if bounds.get().map(|known| known.size) != Some(area.size) {
                    window.request_animation_frame();
                }
                bounds.set(Some(area));
            },
            |_, (), _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        let layer = div().absolute().inset_0().children(tiles);
        let layer = if motion::reduced() {
            layer.into_any_element()
        } else {
            layer
                .with_animation(
                    ("map-zoom", self.map.epoch),
                    Animation::new(ZOOM).with_easing(motion::ease_out),
                    |layer, progress| layer.opacity(0.35 + 0.65 * progress),
                )
                .into_any_element()
        };
        div()
            .id("map")
            .debug_selector(|| "map".to_owned())
            .relative()
            .flex_1()
            .min_h(unit(size::MAP))
            .w_full()
            .overflow_hidden()
            .bg(theme::panel())
            .child(probe)
            .child(layer)
            .into_any_element()
    }

    fn render_tile(
        &self,
        index: usize,
        node: &Node,
        rect: Rect,
        (width, height): (f32, f32),
        focused: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let item = node.item();
        let selected = item.is_some_and(|item| self.selection.contains(&(item.source, item.id)));
        let (fill, edge) = match node.kind {
            Kind::Item(item) if selected => (
                theme::tile_selected(),
                Some(theme::tile_edge(item.category)),
            ),
            Kind::Item(item) => (
                theme::tile(item.category),
                Some(theme::tile_edge(item.category)),
            ),
            Kind::Rest(_) => (theme::tile_rest(), None),
            Kind::Group | Kind::Folder => (theme::tile_folder(), None),
        };
        let labelled = rect.w >= LABEL.0 && rect.h >= LABEL.1;
        let named = rect.w >= NAME.0 && rect.h >= NAME.1;
        let mut details = Tooltip::details(node.name.clone()).row("Size", theme::bytes(node.bytes));
        details = match node.kind {
            Kind::Item(item) => details
                .row("Category", item.category.as_str())
                .row("Safety", item.safety.as_str())
                .row(
                    "Kind",
                    match item.source {
                        Source::Artifact => "project artifact",
                        Source::Cache => "global cache",
                        Source::Toolchain => "toolchain",
                        Source::Docker => "docker object",
                    },
                ),
            Kind::Rest(count) => details.row("Items", count.to_string()).note(
                "Each of these is too small to show on its own. Project artifacts lists every item.",
            ),
            Kind::Group | Kind::Folder => details.row("Items", node.items.to_string()),
        };
        if let Some(path) = &node.path {
            details = details.note(display_path(path));
        }
        let outline = (focused || selected).then(|| {
            if selected {
                theme::bar_strong()
            } else {
                theme::focus()
            }
        });
        let marker = focused.then(|| Rc::clone(&self.cursor_bounds));
        let inner = div()
            .relative()
            .size_full()
            .overflow_hidden()
            .flex()
            .flex_col()
            .gap(unit(size::HAIR))
            .px(unit(space::S))
            .py(unit(space::XS))
            .bg(fill)
            .when(matches!(node.kind, Kind::Group | Kind::Folder), |this| {
                this.border_1().border_color(theme::border_strong())
            })
            .when_some(outline, |this, color| {
                if focused {
                    this.border_2().border_color(color)
                } else {
                    this.border_1().border_color(color)
                }
            })
            .hover(|style| style.opacity(0.8))
            .when_some(edge, |this, edge| {
                this.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .h(unit(size::MARKER))
                        .bg(edge),
                )
            })
            .when_some(marker, |this, bounds| {
                this.child(
                    canvas(move |area, _, _| bounds.set(Some(area)), |_, (), _, _| {})
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full(),
                )
            })
            .when(named, |this| {
                this.child(
                    div()
                        .truncate()
                        .text_size(unit(font::SMALL))
                        .text_color(if matches!(node.kind, Kind::Rest(_)) {
                            theme::muted()
                        } else {
                            theme::text()
                        })
                        .child(node.name.clone()),
                )
            })
            .when(labelled, |this| {
                this.child(
                    div()
                        .truncate()
                        .text_size(unit(font::CAPTION))
                        .text_color(theme::muted())
                        .child(theme::bytes(node.bytes)),
                )
            })
            .when(labelled && node.opens(), |this| {
                this.child(
                    div()
                        .truncate()
                        .text_size(unit(font::CAPTION))
                        .text_color(theme::faint())
                        .child(plural(node.items, "item")),
                )
            })
            .when(selected && named, |this| {
                this.child(
                    div()
                        .debug_selector(move || format!("map-check-{index}"))
                        .absolute()
                        .top(unit(space::XS))
                        .right(unit(space::XS))
                        .child(ui::checkbox(true)),
                )
            });
        let key = node.key.clone();
        let menu_key = node.key.clone();
        div()
            .id(ElementId::from(SharedString::from(format!(
                "map-tile:{}",
                node.key
            ))))
            .debug_selector(move || format!("map-tile-{index}"))
            .absolute()
            .left(relative(rect.x / width))
            .top(relative(rect.y / height))
            .w(relative(rect.w / width))
            .h(relative(rect.h / height))
            .p(unit(size::HAIR))
            .when(node.opens(), |this| this.cursor_pointer())
            .tip(details.build())
            .on_click(cx.listener(move |app, event: &ClickEvent, _, cx| {
                app.click_tile(&key, event.modifiers(), cx);
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |app, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    app.right_click_tile(&menu_key, event.position, cx);
                }),
            )
            .child(inner)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use gpui::{Entity, TestAppContext, VisualTestContext, size};

    use super::*;
    use crate::app::tests::{menu_target, open, type_text, wait_for_refresh};

    /// Two projects in `team` and one in `solo`, so the top level has a folder and an item.
    fn workspace(dir: &Path) -> PathBuf {
        let root = dir.join("work");
        for (project, bytes) in [("team/p1", 96), ("team/p2", 48), ("solo", 24)] {
            let folder = root.join(project);
            let modules = folder.join("node_modules").join("dep");
            std::fs::create_dir_all(&modules).unwrap();
            std::fs::write(folder.join("package.json"), "{}").unwrap();
            std::fs::write(modules.join("index.js"), vec![b'x'; bytes * 1024]).unwrap();
        }
        root
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.update(|window, cx| window.draw(cx).clear());
        cx.update(|window, cx| window.draw(cx).clear());
    }

    fn mapped<'a>(
        cx: &'a mut TestAppContext,
        dir: &Path,
    ) -> (Entity<Kuizer>, &'a mut VisualTestContext) {
        cx.update(bind_keys);
        let root = workspace(dir);
        let (view, cx) = open(cx, dir.join("recent"));
        cx.simulate_resize(size(px(1200.), px(800.)));
        type_text(cx, &root.display().to_string());
        cx.simulate_keystrokes("enter");
        wait_for_refresh(&view, cx);
        cx.simulate_keystrokes("m");
        view.read_with(cx, |app, _| {
            assert!(app.map_active(), "M opens the space map");
            assert_eq!(app.tab, Tab::Overview);
        });
        draw(cx);
        (view, cx)
    }

    #[gpui::test]
    fn a_hidden_map_is_built_when_it_is_shown(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        cx.update(bind_keys);
        let root = workspace(dir.path());
        let (view, cx) = open(cx, dir.path().join("recent"));
        type_text(cx, &root.display().to_string());
        cx.simulate_keystrokes("enter");
        wait_for_refresh(&view, cx);
        view.read_with(cx, |app, _| {
            assert!(
                app.map.tree.is_none(),
                "nothing is built for a map nobody opened"
            );
            assert!(app.map.dirty);
        });
        cx.simulate_keystrokes("m");
        view.read_with(cx, |app, _| {
            assert!(!app.map.dirty);
            assert!(app.map_level().is_some_and(|level| level.items > 0));
        });
    }

    fn level(view: &Entity<Kuizer>, cx: &mut VisualTestContext) -> Vec<(String, u64)> {
        view.read_with(cx, |app, _| {
            app.map_level()
                .unwrap()
                .children
                .iter()
                .map(|child| (child.name.clone(), child.bytes))
                .collect()
        })
    }

    fn path(view: &Entity<Kuizer>, cx: &mut VisualTestContext) -> Vec<String> {
        view.read_with(cx, |app, _| app.map.path.clone())
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str, modifiers: Modifiers) {
        let bounds = cx.debug_bounds(selector).expect("the tile is painted");
        cx.simulate_click(bounds.center(), modifiers);
        draw(cx);
    }

    fn focused_item(view: &Entity<Kuizer>, cx: &mut VisualTestContext) -> (Source, u64) {
        view.read_with(cx, |app, _| {
            let item = app
                .map_focused()
                .and_then(Node::item)
                .expect("an item tile");
            (item.source, item.id)
        })
    }

    #[gpui::test]
    fn the_space_map_draws_tiles_sized_by_their_space(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (view, cx) = mapped(cx, dir.path());
        let tiles = level(&view, cx);
        assert_eq!(tiles.len(), 2);
        assert_eq!(tiles[0].0, "team");
        assert_eq!(tiles[1].0, "solo/node_modules");
        let map = cx.debug_bounds("map").expect("the map is painted");
        let first = cx.debug_bounds("map-tile-0").expect("the folder tile");
        let second = cx.debug_bounds("map-tile-1").expect("the item tile");
        let area =
            |bounds: Bounds<Pixels>| f32::from(bounds.size.width) * f32::from(bounds.size.height);
        #[allow(clippy::cast_precision_loss, reason = "test sizes are small")]
        let want = tiles[0].1 as f32 / tiles[1].1 as f32;
        let got = area(first) / area(second);
        assert!(
            (got - want).abs() / want < 0.02,
            "areas {got} and sizes {want}"
        );
        assert!(
            (area(first) + area(second) - area(map)).abs() / area(map) < 0.01,
            "the tiles fill the map"
        );
        assert!(map.contains(&first.center()) && map.contains(&second.center()));
        assert!(cx.debug_bounds("view-map").is_some());

        cx.simulate_keystrokes("m");
        view.read_with(cx, |app, _| {
            assert!(!app.map.shown, "M goes back to the summary")
        });
        cx.simulate_keystrokes("2 m");
        view.read_with(cx, |app, _| {
            assert_eq!(app.tab, Tab::Overview, "M opens the map from any section");
            assert!(app.map.shown);
        });
    }

    #[gpui::test]
    fn clicking_a_folder_zooms_in_and_the_breadcrumb_leads_back(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (view, cx) = mapped(cx, dir.path());
        click(cx, "map-tile-0", Modifiers::default());
        assert_eq!(path(&view, cx).len(), 1);
        assert!(path(&view, cx)[0].ends_with("work/team"));
        let names: Vec<String> = level(&view, cx).into_iter().map(|(name, _)| name).collect();
        assert_eq!(names, ["p1/node_modules", "p2/node_modules"]);
        let top = cx.debug_bounds("map-crumb-0").expect("the top crumb");
        let team = cx.debug_bounds("map-crumb-1").expect("the opened folder");
        assert!(team.left() > top.right(), "the trail reads left to right");
        view.read_with(cx, |app, _| {
            let trail: Vec<&str> = app
                .map_trail()
                .iter()
                .map(|node| node.name.as_str())
                .collect();
            assert_eq!(trail[1], "team");
        });

        cx.simulate_keystrokes("backspace");
        assert!(path(&view, cx).is_empty(), "Backspace zooms out");
        view.read_with(cx, |app, _| {
            assert_eq!(
                app.map_focus_index(),
                Some(0),
                "the focus is on the folder it left"
            );
        });
        cx.simulate_keystrokes("enter");
        assert_eq!(path(&view, cx).len(), 1, "Enter opens the focus tile");
        cx.simulate_keystrokes("escape");
        assert!(path(&view, cx).is_empty(), "Escape zooms out");
        cx.simulate_keystrokes("enter");
        draw(cx);
        click(cx, "map-crumb-0", Modifiers::default());
        assert!(path(&view, cx).is_empty(), "the breadcrumb zooms out");
        cx.simulate_keystrokes("right enter");
        assert!(path(&view, cx).is_empty(), "an item tile does not open");
        view.read_with(cx, |app, _| {
            assert!(app.dialog.is_none(), "Enter in the map never trashes");
        });
    }

    #[gpui::test]
    fn selecting_a_tile_selects_the_item_in_the_artifacts_list(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (view, cx) = mapped(cx, dir.path());
        cx.simulate_keystrokes("enter");
        draw(cx);
        let first = focused_item(&view, cx);
        cx.simulate_keystrokes("space");
        view.read_with(cx, |app, _| {
            assert_eq!(app.selection, HashSet::from([first]));
        });
        draw(cx);
        assert!(
            cx.debug_bounds("map-check-0").is_some(),
            "the tile shows it is selected"
        );
        cx.simulate_keystrokes("right");
        let second = focused_item(&view, cx);
        assert_ne!(first, second, "the arrow moved the focus");

        click(cx, "map-tile-1", Modifiers::secondary_key());
        view.read_with(cx, |app, _| {
            assert_eq!(app.selection, HashSet::from([first, second]));
            assert!(
                app.map.path.len() == 1,
                "a modified click selects instead of opening"
            );
        });
        cx.simulate_keystrokes("shift-2");
        view.read_with(cx, |app, _| {
            assert_eq!(app.tab, Tab::Artifacts);
            let listed: HashSet<(Source, u64)> = app
                .selected()
                .iter()
                .map(|(source, candidate)| (*source, candidate.id))
                .collect();
            assert_eq!(listed, HashSet::from([first, second]));
        });
        cx.simulate_keystrokes("ctrl-shift-a 1");
        view.read_with(cx, |app, _| {
            assert!(app.selection.is_empty(), "the list cleared it");
            let level = app.map_level().unwrap();
            assert_eq!(level.leaves().len(), 2);
        });
        draw(cx);
        assert!(cx.debug_bounds("map-total").is_some());
    }

    #[gpui::test]
    fn right_clicking_an_item_tile_opens_the_item_menu(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (view, cx) = mapped(cx, dir.path());
        let folder = cx.debug_bounds("map-tile-0").unwrap();
        cx.simulate_mouse_down(folder.center(), MouseButton::Right, Modifiers::default());
        cx.simulate_mouse_up(folder.center(), MouseButton::Right, Modifiers::default());
        assert!(
            menu_target(&view, cx).is_none(),
            "a folder has no item menu"
        );

        let tile = cx.debug_bounds("map-tile-1").unwrap();
        cx.simulate_mouse_down(tile.center(), MouseButton::Right, Modifiers::default());
        cx.simulate_mouse_up(tile.center(), MouseButton::Right, Modifiers::default());
        let (source, id) = focused_item(&view, cx);
        let Some(Target::Item(menu_source, menu_id, menu_path)) = menu_target(&view, cx) else {
            panic!("the item menu is open");
        };
        assert_eq!((menu_source, menu_id), (source, id));
        assert!(menu_path.ends_with("solo/node_modules"));
        draw(cx);
        assert!(cx.debug_bounds("menu").is_some(), "the menu is painted");
        cx.simulate_keystrokes("s");
        view.read_with(cx, |app, _| {
            assert!(app.menu.is_none());
            assert!(
                app.selection.contains(&(source, id)),
                "Select in the menu selects it"
            );
        });
        cx.simulate_keystrokes("shift-f10");
        assert!(
            menu_target(&view, cx).is_some(),
            "the keyboard opens it on the focus tile"
        );
    }

    #[gpui::test]
    fn the_zoom_path_survives_a_refresh_and_follows_exclusions(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (view, cx) = mapped(cx, dir.path());
        cx.simulate_keystrokes("enter");
        let opened = path(&view, cx);
        assert_eq!(opened.len(), 1);
        view.update(cx, |app, cx| app.rescan(cx));
        view.read_with(cx, |app, _| assert!(app.refreshing()));
        assert_eq!(path(&view, cx), opened, "kept while the fresh scan runs");
        wait_for_refresh(&view, cx);
        assert_eq!(path(&view, cx), opened, "kept once the fresh scan is in");
        assert_eq!(level(&view, cx).len(), 2);
        draw(cx);

        let tile = cx.debug_bounds("map-tile-1").unwrap();
        cx.simulate_mouse_down(tile.center(), MouseButton::Right, Modifiers::default());
        cx.simulate_mouse_up(tile.center(), MouseButton::Right, Modifiers::default());
        cx.simulate_keystrokes("x");
        view.read_with(cx, |app, _| {
            assert!(
                app.map.path.is_empty(),
                "the folder is gone, so the map goes up"
            );
            let names: Vec<&str> = app
                .map_level()
                .unwrap()
                .children
                .iter()
                .map(|child| child.name.as_str())
                .collect();
            assert_eq!(names, ["team/p1/node_modules", "solo/node_modules"]);
        });
    }
}
