use std::borrow::Cow;
use std::f32::consts::PI;
use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, AssetSource, ElementId, Pixels, Rgba, SharedString,
    Transformation, div, point, prelude::*, radians, svg,
};

use crate::motion;
use crate::theme::{self, size, unit};

const VIEWBOX: f32 = 24.;
const HOVER: Duration = Duration::from_millis(450);

/// A monochrome line icon. Icons take the text color they are given, never a hue of their own.
/// Each icon is built from layers, so parts of it can move on their own when it is hovered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Overview,
    Artifacts,
    Caches,
    Toolchains,
    Docker,
    Repos,
    Duplicates,
    History,
    Issues,
    Sidebar,
    Search,
}

/// How a layer moves while its icon plays the hover animation. Shifts are in viewbox units,
/// turns in degrees. Every motion ends where it started.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Motion {
    Still,
    Shift(f32, f32),
    Turn(f32),
    Spin(f32),
    Wiggle(f32),
}

type Layer = (&'static str, Motion);

impl Icon {
    pub const ALL: [Icon; 11] = [
        Self::Overview,
        Self::Artifacts,
        Self::Caches,
        Self::Toolchains,
        Self::Docker,
        Self::Repos,
        Self::Duplicates,
        Self::History,
        Self::Issues,
        Self::Sidebar,
        Self::Search,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Artifacts => "artifacts",
            Self::Caches => "caches",
            Self::Toolchains => "toolchains",
            Self::Docker => "docker",
            Self::Repos => "repos",
            Self::Duplicates => "duplicates",
            Self::History => "history",
            Self::Issues => "issues",
            Self::Sidebar => "sidebar",
            Self::Search => "search",
        }
    }

    pub fn layers(self) -> &'static [Layer] {
        match self {
            Self::Overview => &[
                (
                    r#"<rect width="7" height="9" x="3" y="3" rx="1"/>"#,
                    Motion::Shift(11., 0.),
                ),
                (
                    r#"<rect width="7" height="5" x="14" y="3" rx="1"/>"#,
                    Motion::Shift(0., 13.),
                ),
                (
                    r#"<rect width="7" height="9" x="14" y="12" rx="1"/>"#,
                    Motion::Shift(-11., 0.),
                ),
                (
                    r#"<rect width="7" height="5" x="3" y="16" rx="1"/>"#,
                    Motion::Shift(0., -13.),
                ),
            ],
            Self::Artifacts => &[
                (
                    r#"<path d="M11 21.73a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73z"/><path d="M12 22V12"/><path d="m3.3 7 8.7 5 8.7-5"/>"#,
                    Motion::Shift(0., -1.5),
                ),
                (r#"<path d="m7.5 4.27 9 5.15"/>"#, Motion::Shift(0., -3.)),
            ],
            Self::Caches => &[
                (
                    r#"<ellipse cx="12" cy="5" rx="9" ry="3"/>"#,
                    Motion::Shift(0., -2.5),
                ),
                (
                    r#"<path d="M3 12A9 3 0 0 0 21 12"/>"#,
                    Motion::Shift(0., -1.),
                ),
                (r#"<path d="M3 5V19A9 3 0 0 0 21 19V5"/>"#, Motion::Still),
            ],
            Self::Toolchains => &[(
                r#"<path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z"/>"#,
                Motion::Wiggle(-24.),
            )],
            Self::Docker => &[
                (
                    r#"<path d="M12.83 2.18a2 2 0 0 0-1.66 0L2.6 6.08a1 1 0 0 0 0 1.83l8.58 3.91a2 2 0 0 0 1.66 0l8.58-3.9a1 1 0 0 0 0-1.83Z"/>"#,
                    Motion::Shift(0., -3.),
                ),
                (
                    r#"<path d="m22 12.65-9.17 4.16a2 2 0 0 1-1.66 0L2 12.65"/>"#,
                    Motion::Shift(0., -1.5),
                ),
                (
                    r#"<path d="m22 17.65-9.17 4.16a2 2 0 0 1-1.66 0L2 17.65"/>"#,
                    Motion::Still,
                ),
            ],
            Self::Repos => &[
                (
                    r#"<line x1="6" x2="6" y1="3" y2="15"/><circle cx="6" cy="18" r="3"/>"#,
                    Motion::Still,
                ),
                (
                    r#"<circle cx="18" cy="6" r="3"/><path d="M18 9a9 9 0 0 1-9 9"/>"#,
                    Motion::Shift(1.5, -1.5),
                ),
            ],
            Self::Duplicates => &[
                (
                    r#"<path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/>"#,
                    Motion::Shift(-1.5, -1.5),
                ),
                (
                    r#"<rect width="14" height="14" x="8" y="8" rx="2" ry="2"/>"#,
                    Motion::Shift(1.5, 1.5),
                ),
            ],
            Self::History => &[
                (
                    r#"<path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/>"#,
                    Motion::Turn(-40.),
                ),
                (r#"<path d="M12 7v5l4 2"/>"#, Motion::Spin(-360.)),
            ],
            Self::Issues => &[(
                r#"<path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3"/><path d="M12 9v4"/><path d="M12 17h.01"/>"#,
                Motion::Wiggle(12.),
            )],
            Self::Sidebar => &[
                (
                    r#"<rect width="18" height="18" x="3" y="3" rx="2"/>"#,
                    Motion::Still,
                ),
                (r#"<path d="M9 3v18"/>"#, Motion::Shift(3., 0.)),
            ],
            Self::Search => &[(
                r#"<circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/>"#,
                Motion::Wiggle(14.),
            )],
        }
    }

    fn path(self, layer: usize) -> SharedString {
        format!("icons/{}/{layer}.svg", self.name()).into()
    }
}

impl Motion {
    /// @name at
    /// @description Where a layer is at `progress` (0 to 1) of the hover animation. `unit` is
    /// the size of one viewbox unit on screen.
    ///
    /// @example
    /// let start = Motion::Turn(90.).at(0.0, px(1.));
    pub fn at(self, progress: f32, unit: Pixels) -> Transformation {
        let (x, y, degrees) = self.offset(progress);
        Transformation::translate(point(unit * x, unit * y))
            .with_rotation(radians(degrees.to_radians()))
    }

    fn offset(self, progress: f32) -> (f32, f32, f32) {
        let swing = (progress * PI).sin();
        match self {
            Self::Still => (0., 0., 0.),
            Self::Shift(x, y) => (x * swing, y * swing, 0.),
            Self::Turn(degrees) => (0., 0., degrees * swing),
            Self::Spin(degrees) => (0., 0., degrees * motion::ease_in_out(progress)),
            Self::Wiggle(degrees) => (
                0.,
                0.,
                degrees * (progress * 2. * PI).sin() * (1. - progress),
            ),
        }
    }
}

/// @name view
/// @description Draws an icon at the standard icon size in the given color. With `hover` set,
/// its layers play the hover animation once; a new value replays it. Without motion the icon
/// stays still.
///
/// @example
/// icon::view(Icon::History, theme::muted(), Some(hovers), zoom.rem_size())
pub fn view(icon: Icon, color: Rgba, hover: Option<u64>, rem: Pixels) -> AnyElement {
    let unit_px = rem * (size::ICON / theme::REM / VIEWBOX);
    let animate = hover.filter(|_| !motion::reduced());
    div()
        .relative()
        .flex_none()
        .size(unit(size::ICON))
        .children(
            icon.layers()
                .iter()
                .enumerate()
                .map(move |(index, (_, motion))| {
                    let layer = svg()
                        .path(icon.path(index))
                        .text_color(color)
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full();
                    match (animate, *motion) {
                        (Some(epoch), motion) if motion != Motion::Still => layer
                            .with_animation(
                                ElementId::NamedInteger(
                                    format!("icon-{}-{index}", icon.name()).into(),
                                    epoch,
                                ),
                                Animation::new(HOVER),
                                move |layer, progress| {
                                    layer.with_transformation(motion.at(progress, unit_px))
                                },
                            )
                            .into_any_element(),
                        _ => layer.into_any_element(),
                    }
                }),
        )
        .into_any_element()
}

fn source(fragment: &str) -> Vec<u8> {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">{fragment}</svg>"#
    )
    .into_bytes()
}

fn layer_source(path: &str) -> Option<Vec<u8>> {
    let rest = path.strip_prefix("icons/")?.strip_suffix(".svg")?;
    let (name, layer) = rest.split_once('/')?;
    let icon = Icon::ALL.into_iter().find(|icon| icon.name() == name)?;
    let (fragment, _) = icon.layers().get(layer.parse::<usize>().ok()?)?;
    Some(source(fragment))
}

/// The icon layers, built into the binary and served to GPUI's `svg()` element.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        Ok(layer_source(path).map(Cow::Owned))
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(Icon::ALL
            .into_iter()
            .flat_map(|icon| (0..icon.layers().len()).map(move |layer| icon.path(layer)))
            .filter(|candidate| candidate.starts_with(path))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_layer_is_served_as_an_svg() {
        for icon in Icon::ALL {
            for layer in 0..icon.layers().len() {
                let bytes = Assets
                    .load(&icon.path(layer))
                    .expect("loads")
                    .expect("exists");
                assert!(bytes.starts_with(b"<svg"), "{icon:?} {layer}");
            }
        }
        assert!(
            Assets
                .load("icons/overview/9.svg")
                .expect("loads")
                .is_none()
        );
        assert!(Assets.load("icons/missing/0.svg").expect("loads").is_none());
        assert_eq!(Assets.list("icons/overview/").expect("lists").len(), 4);
    }

    #[test]
    fn every_motion_ends_where_it_started() {
        for icon in Icon::ALL {
            for (_, motion) in icon.layers() {
                for progress in [0., 1.] {
                    let (x, y, degrees) = motion.offset(progress);
                    let resting = x.abs() < 1e-3 && y.abs() < 1e-3;
                    let upright = degrees
                        .rem_euclid(360.)
                        .min(360. - degrees.rem_euclid(360.));
                    assert!(resting && upright < 1e-2, "{icon:?} {motion:?}");
                }
            }
        }
    }

    #[test]
    fn every_moving_layer_moves_halfway() {
        for icon in Icon::ALL {
            for (_, motion) in icon.layers() {
                let (x, y, degrees) = motion.offset(0.3);
                let moved = x != 0. || y != 0. || degrees != 0.;
                assert_eq!(moved, *motion != Motion::Still, "{icon:?} {motion:?}");
            }
        }
    }
}
