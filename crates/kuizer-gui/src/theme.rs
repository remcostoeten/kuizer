use std::time::SystemTime;

use gpui::{Pixels, Rems, Rgba, px, rems, rgb, rgba};
use kuizer::detect::Category;
use kuizer::units::ByteSize;

pub const REM: f32 = 16.;

pub fn unit(value: f32) -> Rems {
    rems(value / REM)
}

/// The spacing scale. Every gap, padding and margin in a view is one of these steps.
pub mod space {
    pub const XXS: f32 = 2.;
    pub const XS: f32 = 4.;
    pub const S: f32 = 6.;
    pub const M: f32 = 8.;
    pub const L: f32 = 12.;
    pub const XL: f32 = 16.;
    pub const XXL: f32 = 24.;
    /// The left and right edge shared by the titlebar, sidebar items, lists and status bar.
    pub const GUTTER: f32 = XL;
    /// The inset of a sidebar item inside the sidebar, so item text starts at the gutter.
    pub const INSET: f32 = M;
}

/// The type scale.
pub mod font {
    pub const CAPTION: f32 = 11.;
    pub const SMALL: f32 = 12.;
    pub const BODY: f32 = 13.;
    pub const TITLE: f32 = 15.;
    pub const HEADING: f32 = 18.;
    pub const FIGURE: f32 = 24.;
}

/// Fixed heights and widths of controls, bars and panels.
pub mod size {
    pub const HAIR: f32 = 1.;
    pub const MARKER: f32 = 2.;
    pub const DOT: f32 = 6.;
    pub const ICON: f32 = 16.;
    pub const CHECK: f32 = 14.;
    pub const BADGE: f32 = 18.;
    pub const SEGMENT: f32 = 24.;
    pub const CONTROL: f32 = 28.;
    pub const INPUT: f32 = 32.;
    pub const ROW: f32 = 32.;
    pub const ROW_TALL: f32 = 44.;
    pub const BAR: f32 = 40.;
    pub const TOOLBAR: f32 = 44.;
    pub const SIDEBAR: f32 = 220.;
    pub const SIDEBAR_COLLAPSED: f32 = 48.;
    pub const METER: f32 = 12.;
    pub const TRACK: f32 = 4.;
    pub const SWATCH: f32 = 8.;
    pub const LEGEND: f32 = 10.;
    pub const CARET: f32 = 14.;
    pub const COLUMNS: f32 = 88.;
    pub const CHART: f32 = 120.;
    pub const MAP: f32 = 200.;
    pub const STAT: f32 = 140.;
    pub const ACTIONS: f32 = 196.;
    pub const LABEL: f32 = 240.;
    pub const KEYS: f32 = 250.;
    pub const FILTER: f32 = 300.;
    /// The narrowest the titlebar search shrinks to when the window runs out of room.
    pub const SEARCH: f32 = 160.;
    pub const LIST: f32 = 340.;
    pub const MENU: f32 = 220.;
    pub const DIALOG: f32 = 520.;
    pub const PALETTE: f32 = 560.;
    pub const PAGE: f32 = 760.;
    pub const SHORTCUTS: f32 = 820.;
    /// How far below the titlebar the command palette opens.
    pub const DROP: f32 = 72.;
}

/// Widths of fixed table columns.
pub mod col {
    pub const XS: f32 = 56.;
    pub const S: f32 = 72.;
    pub const M: f32 = 96.;
    pub const L: f32 = 112.;
    pub const XL: f32 = 152.;
    pub const XXL: f32 = 176.;
    pub const WIDE: f32 = 240.;
}

pub fn background() -> Rgba {
    rgb(0x0011_1111)
}

pub fn transparent() -> Rgba {
    rgba(0x0000_0000)
}

pub fn scrim() -> Rgba {
    rgba(0x0000_00a0)
}

pub fn panel() -> Rgba {
    rgb(0x0017_1717)
}

pub fn elevated() -> Rgba {
    rgb(0x001c_1c1c)
}

pub fn hover() -> Rgba {
    rgb(0x0022_2222)
}

pub fn selected() -> Rgba {
    rgb(0x002d_1f16)
}

/// The outline of whatever has the keyboard: the open section, a focused field.
pub fn focus() -> Rgba {
    rgb(0x00a8_a8a8)
}

/// The tint over the row that has the keyboard.
pub fn cursor() -> Rgba {
    rgba(0xffff_ff0a)
}

pub fn border() -> Rgba {
    rgb(0x0029_2929)
}

pub fn border_strong() -> Rgba {
    rgb(0x0038_3838)
}

pub fn text() -> Rgba {
    rgb(0x00fa_fafa)
}

pub fn muted() -> Rgba {
    rgb(0x00a1_a1a1)
}

pub fn faint() -> Rgba {
    rgb(0x0073_7373)
}

pub fn bar() -> Rgba {
    rgb(0x00d9_7757)
}

pub fn bar_strong() -> Rgba {
    rgb(0x00c8_c8c8)
}

pub fn bar_mid() -> Rgba {
    rgb(0x0076_7676)
}

pub fn bar_soft() -> Rgba {
    rgb(0x0062_6262)
}

pub fn bar_rest() -> Rgba {
    rgb(0x0038_3838)
}

pub fn track() -> Rgba {
    rgb(0x0026_2626)
}

pub fn review() -> Rgba {
    rgba(0xffd2_36e6)
}

pub fn review_soft() -> Rgba {
    rgba(0xc2a3_6e24)
}

pub fn danger() -> Rgba {
    rgba(0xff66_7fbf)
}

pub fn primary() -> Rgba {
    rgb(0x00fa_fafa)
}

pub fn on_primary() -> Rgba {
    rgb(0x0011_1111)
}

/// A folder tile of the space map: darker than the items it holds.
pub fn tile_folder() -> Rgba {
    rgb(0x0020_2020)
}

/// The "n smaller" tile of the space map.
pub fn tile_rest() -> Rgba {
    rgb(0x001c_1c1c)
}

/// An item tile of the space map. Categories differ by a small step in grey, never by hue.
pub fn tile(category: Category) -> Rgba {
    match category {
        Category::Dependencies => rgb(0x002c_2c2c),
        Category::Build => rgb(0x0029_2929),
        Category::Cache => rgb(0x0026_2626),
        Category::Temp => rgb(0x0023_2323),
    }
}

/// The thin top edge of an item tile, a second neutral cue for its category.
pub fn tile_edge(category: Category) -> Rgba {
    match category {
        Category::Dependencies => rgb(0x0070_7070),
        Category::Build => rgb(0x0058_5858),
        Category::Cache => rgb(0x0046_4646),
        Category::Temp => rgb(0x0038_3838),
    }
}

pub fn tile_selected() -> Rgba {
    rgb(0x003c_3c3c)
}

pub fn radius_button() -> Pixels {
    px(0.)
}

pub fn radius_control() -> Pixels {
    px(0.)
}

pub fn radius_panel() -> Pixels {
    px(0.)
}

pub fn radius_dialog() -> Pixels {
    px(0.)
}

pub fn radius_checkbox() -> Pixels {
    px(0.)
}

pub fn radius_dot() -> Pixels {
    px(0.)
}

pub fn radius_bar() -> Pixels {
    px(0.)
}

pub const FONT_CANDIDATES: [&str; 5] = [
    "Geist",
    "Inter",
    "IBM Plex Sans",
    "Zed Plex Sans",
    "Noto Sans",
];

pub fn bytes(value: u64) -> String {
    ByteSize(value).to_string()
}

/// @name age
/// @description Formats how long ago a time was in a short form such as `3d` or `5mo`.
///
/// @example
/// assert_eq!(age(Some(SystemTime::now())), "now");
pub fn age(time: Option<SystemTime>) -> String {
    let Some(elapsed) = time.and_then(|time| SystemTime::now().duration_since(time).ok()) else {
        return String::new();
    };
    let seconds = elapsed.as_secs();
    match seconds {
        0..60 => "now".to_owned(),
        60..3_600 => format!("{}m", seconds / 60),
        3_600..86_400 => format!("{}h", seconds / 3_600),
        86_400..2_592_000 => format!("{}d", seconds / 86_400),
        2_592_000..31_536_000 => format!("{}mo", seconds / 2_592_000),
        _ => format!("{}y", seconds / 31_536_000),
    }
}

/// @name share
/// @description Returns `part` as a fraction of `whole` between 0 and 1, for bar widths.
///
/// @example
/// assert_eq!(share(1, 4), 0.25);
#[allow(
    clippy::cast_precision_loss,
    reason = "bar widths only need a few significant digits"
)]
pub fn share(part: u64, whole: u64) -> f32 {
    if whole == 0 {
        return 0.0;
    }
    (part as f64 / whole as f64).clamp(0.0, 1.0) as f32
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn age_uses_the_largest_whole_unit() {
        let now = SystemTime::now();
        assert_eq!(age(None), "");
        assert_eq!(age(Some(now)), "now");
        assert_eq!(age(Some(now - Duration::from_secs(7_200))), "2h");
        assert_eq!(age(Some(now - Duration::from_secs(3 * 86_400))), "3d");
        assert_eq!(age(Some(now - Duration::from_secs(400 * 86_400))), "1y");
    }

    #[test]
    fn share_is_clamped_and_safe_for_zero() {
        assert!((share(1, 4) - 0.25).abs() < f32::EPSILON);
        assert!(share(5, 0).abs() < f32::EPSILON);
        assert!((share(9, 4) - 1.0).abs() < f32::EPSILON);
    }
}
