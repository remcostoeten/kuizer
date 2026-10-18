use gpui::{
    Bounds, BoxShadow, CursorStyle, Decorations, Div, Global, HitboxBehavior, Hsla, MouseButton,
    PathBuilder, Pixels, Point, ResizeEdge, Size, Stateful, Tiling, TitlebarOptions, Window,
    WindowDecorations, canvas, div, point, prelude::*, px,
};

use crate::theme::{self, size, space, unit};

pub const TITLEBAR: Pixels = px(40.);
const SHADOW: Pixels = px(10.);
const TRAFFIC_LIGHTS: Pixels = px(78.);
const ICON: Pixels = px(10.);

struct HoveredEdge(ResizeEdge);

impl Global for HoveredEdge {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    Minimize,
    Maximize,
    Restore,
    Close,
}

/// @name titlebar
/// @description The native titlebar settings: on macOS the bar turns transparent so kuizer draws
/// its own bar behind the traffic lights, which are centred in it.
///
/// @example
/// let options = titlebar();
pub fn titlebar() -> TitlebarOptions {
    TitlebarOptions {
        title: Some("kuizer".into()),
        appears_transparent: true,
        traffic_light_position: Some(point(px(12.), (TITLEBAR - px(14.)) / 2.)),
    }
}

/// @name decorations
/// @description Asks Linux compositors for client side decorations. GPUI falls back to server
/// decorations by itself when the compositor cannot draw them, for example X11 without compositing.
///
/// @example
/// let decorations = decorations();
pub fn decorations() -> Option<WindowDecorations> {
    if cfg!(target_os = "linux") || cfg!(target_os = "freebsd") {
        Some(WindowDecorations::Client)
    } else {
        None
    }
}

/// @name client_drawn
/// @description Whether kuizer draws the window frame and controls itself right now.
///
/// @example
/// let custom = client_drawn(window);
pub fn client_drawn(window: &Window) -> bool {
    matches!(window.window_decorations(), Decorations::Client { .. })
}

/// @name inset
/// @description Room to keep free on the left of the titlebar for the macOS traffic lights.
///
/// @example
/// let padding = inset(window);
pub fn inset(window: &Window) -> Option<Pixels> {
    (cfg!(target_os = "macos") && !window.is_fullscreen()).then_some(TRAFFIC_LIGHTS)
}

/// @name titlebar_down
/// @description Handles a left press on the titlebar: a double click maximizes or restores the
/// window and returns `false`, a single press returns `true` so the next move starts a drag.
///
/// @example
/// let drag = titlebar_down(2, window);
pub fn titlebar_down(clicks: usize, window: &Window) -> bool {
    if clicks < 2 {
        return client_drawn(window);
    }
    if cfg!(target_os = "macos") {
        window.titlebar_double_click();
    } else {
        window.zoom_window();
    }
    false
}

/// @name frame
/// @description Wraps the whole window when kuizer draws its own decorations: a transparent
/// margin for the shadow and resize handles, and a bordered body. With server decorations the
/// content is returned as is.
///
/// @example
/// let root = frame(content, window);
pub fn frame(content: impl IntoElement, window: &mut Window) -> Stateful<Div> {
    let Decorations::Client { tiling } = window.window_decorations() else {
        return div().id("window").size_full().child(content);
    };
    window.set_client_inset(SHADOW);
    div()
        .id("window")
        .size_full()
        .bg(theme::transparent())
        .when(!tiling.top, |this| this.pt(SHADOW))
        .when(!tiling.bottom, |this| this.pb(SHADOW))
        .when(!tiling.left, |this| this.pl(SHADOW))
        .when(!tiling.right, |this| this.pr(SHADOW))
        .on_mouse_move(move |event, window, cx| {
            let size = window.window_bounds().get_bounds().size;
            let edge = resize_edge(event.position, SHADOW, size, tiling);
            if edge != cx.try_global::<HoveredEdge>().map(|hovered| hovered.0) {
                window.refresh();
            }
        })
        .on_mouse_down(MouseButton::Left, move |event, window, _| {
            let size = window.window_bounds().get_bounds().size;
            if let Some(edge) = resize_edge(event.position, SHADOW, size, tiling) {
                window.start_window_resize(edge);
            }
        })
        .child(
            div()
                .size_full()
                .cursor(CursorStyle::Arrow)
                .border_color(theme::border_strong())
                .when(!tiling.top, |this| this.border_t_1())
                .when(!tiling.bottom, |this| this.border_b_1())
                .when(!tiling.left, |this| this.border_l_1())
                .when(!tiling.right, |this| this.border_r_1())
                .when(!tiling.is_tiled(), |this| {
                    this.shadow(vec![BoxShadow {
                        color: Hsla {
                            h: 0.,
                            s: 0.,
                            l: 0.,
                            a: 0.4,
                        },
                        blur_radius: SHADOW / 2.,
                        spread_radius: px(0.),
                        offset: point(px(0.), px(0.)),
                    }])
                })
                .on_mouse_move(|_, _, cx| cx.stop_propagation())
                .child(content),
        )
        .child(resize_cursor(tiling))
}

fn resize_cursor(tiling: Tiling) -> impl IntoElement {
    canvas(
        |_, window, _| {
            window.insert_hitbox(
                Bounds::new(
                    point(px(0.), px(0.)),
                    window.window_bounds().get_bounds().size,
                ),
                HitboxBehavior::Normal,
            )
        },
        move |_, hitbox, window, cx| {
            let size = window.window_bounds().get_bounds().size;
            let Some(edge) = resize_edge(window.mouse_position(), SHADOW, size, tiling) else {
                if cx.has_global::<HoveredEdge>() {
                    cx.remove_global::<HoveredEdge>();
                }
                return;
            };
            cx.set_global(HoveredEdge(edge));
            window.set_cursor_style(cursor(edge), &hitbox);
        },
    )
    .absolute()
    .size_full()
}

fn cursor(edge: ResizeEdge) -> CursorStyle {
    match edge {
        ResizeEdge::Top | ResizeEdge::Bottom => CursorStyle::ResizeUpDown,
        ResizeEdge::Left | ResizeEdge::Right => CursorStyle::ResizeLeftRight,
        ResizeEdge::TopLeft | ResizeEdge::BottomRight => CursorStyle::ResizeUpLeftDownRight,
        ResizeEdge::TopRight | ResizeEdge::BottomLeft => CursorStyle::ResizeUpRightDownLeft,
    }
}

/// @name resize_edge
/// @description Which edge or corner of the window a point is on, given the width of the
/// invisible margin. Corners are a bit larger than the margin so they are easy to grab, and tiled
/// edges never resize.
///
/// @example
/// let edge = resize_edge(point(px(2.), px(2.)), px(10.), size(px(800.), px(600.)), Tiling::default());
/// assert_eq!(edge, Some(ResizeEdge::TopLeft));
pub fn resize_edge(
    position: Point<Pixels>,
    margin: Pixels,
    window: Size<Pixels>,
    tiling: Tiling,
) -> Option<ResizeEdge> {
    let corner = margin * 1.5;
    if Bounds::new(Point::default(), window)
        .inset(corner)
        .contains(&position)
    {
        return None;
    }
    let top = position.y < corner && !tiling.top;
    let bottom = position.y > window.height - corner && !tiling.bottom;
    let left = position.x < corner && !tiling.left;
    let right = position.x > window.width - corner && !tiling.right;
    match (top, bottom, left, right) {
        (true, _, true, _) => return Some(ResizeEdge::TopLeft),
        (true, _, _, true) => return Some(ResizeEdge::TopRight),
        (_, true, true, _) => return Some(ResizeEdge::BottomLeft),
        (_, true, _, true) => return Some(ResizeEdge::BottomRight),
        _ => {}
    }
    if !tiling.top && position.y < margin {
        Some(ResizeEdge::Top)
    } else if !tiling.bottom && position.y > window.height - margin {
        Some(ResizeEdge::Bottom)
    } else if !tiling.left && position.x < margin {
        Some(ResizeEdge::Left)
    } else if !tiling.right && position.x > window.width - margin {
        Some(ResizeEdge::Right)
    } else {
        None
    }
}

/// @name controls
/// @description Minimize, maximize or restore, and close buttons for the right of the titlebar,
/// only when kuizer draws its own decorations and only for the controls the compositor offers.
///
/// @example
/// let buttons = controls(window);
pub fn controls(window: &Window) -> Option<Div> {
    if !client_drawn(window) {
        return None;
    }
    let offered = window.window_controls();
    let maximize = if window.is_maximized() {
        Control::Restore
    } else {
        Control::Maximize
    };
    Some(
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(unit(space::XXS))
            .when(offered.minimize, |this| {
                this.child(control(Control::Minimize))
            })
            .when(offered.maximize, |this| this.child(control(maximize)))
            .child(control(Control::Close)),
    )
}

fn control(kind: Control) -> Stateful<Div> {
    let id = match kind {
        Control::Minimize => "window-minimize",
        Control::Maximize => "window-maximize",
        Control::Restore => "window-restore",
        Control::Close => "window-close",
    };
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .size(unit(size::CONTROL))
        .rounded(theme::radius_button())
        .cursor_pointer()
        .hover(|style| style.bg(theme::hover()))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            press(kind, window);
        })
        .child(icon(kind))
}

fn press(kind: Control, window: &mut Window) {
    match kind {
        Control::Minimize => window.minimize_window(),
        Control::Maximize | Control::Restore => window.zoom_window(),
        Control::Close => window.remove_window(),
    }
}

fn icon(kind: Control) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| {
            let origin = bounds.center() - point(ICON / 2., ICON / 2.);
            let mut path = PathBuilder::stroke(px(1.));
            for line in lines(kind) {
                path.add_polygon(
                    &line.map(|(x, y)| origin + point(ICON * x, ICON * y)),
                    false,
                );
            }
            if let Ok(path) = path.build() {
                window.paint_path(path, theme::muted());
            }
        },
    )
    .size(ICON)
}

fn lines(kind: Control) -> Vec<[(f32, f32); 2]> {
    match kind {
        Control::Minimize => vec![[(0., 0.5), (1., 0.5)]],
        Control::Maximize => square(0., 0., 1.),
        Control::Restore => {
            let mut lines = square(0., 0.25, 0.75);
            lines.extend([
                [(0.25, 0.25), (0.25, 0.)],
                [(0.25, 0.), (1., 0.)],
                [(1., 0.), (1., 0.75)],
                [(1., 0.75), (0.75, 0.75)],
            ]);
            lines
        }
        Control::Close => vec![[(0., 0.), (1., 1.)], [(1., 0.), (0., 1.)]],
    }
}

fn square(x: f32, y: f32, side: f32) -> Vec<[(f32, f32); 2]> {
    let (right, bottom) = (x + side, y + side);
    vec![
        [(x, y), (right, y)],
        [(right, y), (right, bottom)],
        [(right, bottom), (x, bottom)],
        [(x, bottom), (x, y)],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::size;

    const MARGIN: Pixels = px(10.);

    fn window() -> Size<Pixels> {
        size(px(800.), px(600.))
    }

    fn edge(x: f32, y: f32, tiling: Tiling) -> Option<ResizeEdge> {
        resize_edge(point(px(x), px(y)), MARGIN, window(), tiling)
    }

    #[test]
    fn edges_and_corners_resize() {
        let free = Tiling::default();
        assert_eq!(edge(2., 2., free), Some(ResizeEdge::TopLeft));
        assert_eq!(edge(798., 2., free), Some(ResizeEdge::TopRight));
        assert_eq!(edge(2., 598., free), Some(ResizeEdge::BottomLeft));
        assert_eq!(edge(798., 598., free), Some(ResizeEdge::BottomRight));
        assert_eq!(edge(400., 4., free), Some(ResizeEdge::Top));
        assert_eq!(edge(400., 596., free), Some(ResizeEdge::Bottom));
        assert_eq!(edge(4., 300., free), Some(ResizeEdge::Left));
        assert_eq!(edge(796., 300., free), Some(ResizeEdge::Right));
    }

    #[test]
    fn corners_are_wider_than_the_margin() {
        assert_eq!(edge(13., 2., Tiling::default()), Some(ResizeEdge::TopLeft));
    }

    #[test]
    fn the_inside_never_resizes() {
        assert_eq!(edge(400., 300., Tiling::default()), None);
        assert_eq!(edge(400., 12., Tiling::default()), None);
    }

    #[test]
    fn tiled_edges_never_resize() {
        let tiled = Tiling::tiled();
        assert_eq!(edge(2., 2., tiled), None);
        assert_eq!(edge(400., 4., tiled), None);
        let left = Tiling {
            left: true,
            ..Tiling::default()
        };
        assert_eq!(edge(2., 2., left), Some(ResizeEdge::Top));
        assert_eq!(edge(4., 300., left), None);
    }

    #[test]
    fn icons_stay_inside_their_box() {
        for kind in [
            Control::Minimize,
            Control::Maximize,
            Control::Restore,
            Control::Close,
        ] {
            for line in lines(kind) {
                for (x, y) in line {
                    assert!((0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y));
                }
            }
        }
    }

    #[test]
    fn decorations_are_client_side_on_linux_only() {
        assert_eq!(
            decorations().is_some(),
            cfg!(any(target_os = "linux", target_os = "freebsd"))
        );
    }
}
