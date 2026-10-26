use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

use gpui::{Hsla, Pixels, Rems, Rgba, hsla, px, rems, rgb, rgba};
use kuizer::config::Theme;
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
    pub const GLYPH: f32 = 10.;
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
    pub const FILTER: f32 = 240.;
    pub const LIST: f32 = 340.;
    pub const MENU: f32 = 220.;
    pub const DIALOG: f32 = 520.;
    pub const PALETTE: f32 = 560.;
    pub const PAGE: f32 = 760.;
    pub const SHORTCUTS: f32 = 820.;
    pub const SETTINGS: f32 = 840.;
    pub const SETTINGS_HEIGHT: f32 = 560.;
    pub const SETTINGS_CONTENT: f32 = 600.;
    pub const CARD: f32 = 168.;
    pub const PREVIEW: f32 = 72.;
    pub const SETTINGS_NAV: f32 = 200.;
    pub const SWITCH: f32 = 32.;
    pub const SWITCH_HEIGHT: f32 = 18.;
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

pub const DISABLED: f32 = 0.4;

pub const DIM: f32 = 0.8;

static LIGHT: AtomicBool = AtomicBool::new(false);

/// @name configure
/// @description Applies `appearance.theme` from the config. Every color below follows it on the
/// next render.
///
/// @example
/// configure(config.appearance.theme);
pub fn configure(theme: Theme) {
    LIGHT.store(theme == Theme::Light, Ordering::Relaxed);
}

fn light() -> bool {
    LIGHT.load(Ordering::Relaxed)
}

fn current() -> Theme {
    if light() { Theme::Light } else { Theme::Dark }
}

fn pick_in(theme: Theme, dark: u32, light_value: u32) -> Rgba {
    rgb(if theme == Theme::Light {
        light_value
    } else {
        dark
    })
}

fn pick(dark: u32, light_value: u32) -> Rgba {
    pick_in(current(), dark, light_value)
}

/// The colors of one theme, for drawing a preview of it while another theme is active.
pub struct Swatch {
    pub background: Rgba,
    pub panel: Rgba,
    pub text: Rgba,
    pub muted: Rgba,
    pub accent: Rgba,
}

/// @name swatch
/// @description The main colors of `theme`, independent of the theme that is active.
///
/// @example
/// let preview = swatch(Theme::Light);
pub fn swatch(theme: Theme) -> Swatch {
    Swatch {
        background: background_in(theme),
        panel: panel_in(theme),
        text: text_in(theme),
        muted: muted_in(theme),
        accent: bar_in(theme),
    }
}

fn pick_alpha(dark: u32, light_value: u32) -> Rgba {
    rgba(if light() { light_value } else { dark })
}

fn background_in(theme: Theme) -> Rgba {
    pick_in(theme, 0x0011_1111, 0x00f4_f4f1)
}

pub fn background() -> Rgba {
    background_in(current())
}

pub fn transparent() -> Rgba {
    rgba(0x0000_0000)
}

pub fn shadow() -> Hsla {
    hsla(0., 0., 0., if light() { 0.16 } else { 0.4 })
}

pub fn scrim() -> Rgba {
    pick_alpha(0x0000_00a0, 0x1a1a_1866)
}

fn panel_in(theme: Theme) -> Rgba {
    pick_in(theme, 0x0017_1717, 0x00ec_ece8)
}

pub fn panel() -> Rgba {
    panel_in(current())
}

pub fn elevated() -> Rgba {
    pick(0x001c_1c1c, 0x00fa_faf8)
}

pub fn hover() -> Rgba {
    pick(0x0022_2222, 0x00e3_e3de)
}

pub fn selected() -> Rgba {
    pick(0x002d_1f16, 0x00f2_e2d8)
}

/// The outline of whatever has the keyboard: the open section, a focused field.
pub fn focus() -> Rgba {
    pick(0x00a8_a8a8, 0x0055_5550)
}

/// The tint over the row that has the keyboard.
pub fn cursor() -> Rgba {
    pick_alpha(0xffff_ff0a, 0x0000_000a)
}

pub fn border() -> Rgba {
    pick(0x0029_2929, 0x00dc_dcd6)
}

pub fn border_strong() -> Rgba {
    pick(0x0038_3838, 0x00c6_c6bf)
}

fn text_in(theme: Theme) -> Rgba {
    pick_in(theme, 0x00fa_fafa, 0x001a_1a18)
}

pub fn text() -> Rgba {
    text_in(current())
}

fn muted_in(theme: Theme) -> Rgba {
    pick_in(theme, 0x00a1_a1a1, 0x0060_605b)
}

pub fn muted() -> Rgba {
    muted_in(current())
}

pub fn faint() -> Rgba {
    pick(0x0073_7373, 0x0080_807a)
}

fn bar_in(theme: Theme) -> Rgba {
    pick_in(theme, 0x00d9_7757, 0x00cf_6a48)
}

pub fn bar() -> Rgba {
    bar_in(current())
}

pub fn bar_strong() -> Rgba {
    pick(0x00c8_c8c8, 0x003a_3a37)
}

pub fn bar_mid() -> Rgba {
    pick(0x0076_7676, 0x0089_8984)
}

pub fn bar_soft() -> Rgba {
    pick(0x0062_6262, 0x00a3_a39d)
}

pub fn bar_rest() -> Rgba {
    pick(0x0038_3838, 0x00c8_c8c2)
}

pub fn track() -> Rgba {
    pick(0x0026_2626, 0x00e1_e1dc)
}

pub fn review() -> Rgba {
    pick_alpha(0xffd2_36e6, 0x9a63_08f2)
}

pub fn review_soft() -> Rgba {
    pick_alpha(0xc2a3_6e24, 0xb482_0a24)
}

pub fn danger() -> Rgba {
    pick_alpha(0xff66_7fbf, 0xc826_46e6)
}

pub fn primary() -> Rgba {
    pick(0x00fa_fafa, 0x001a_1a18)
}

pub fn on_primary() -> Rgba {
    pick(0x0011_1111, 0x00f4_f4f1)
}

/// A folder tile of the space map: set back from the items it holds.
pub fn tile_folder() -> Rgba {
    pick(0x0020_2020, 0x00e8_e8e3)
}

/// The "n smaller" tile of the space map.
pub fn tile_rest() -> Rgba {
    pick(0x001c_1c1c, 0x00ec_ece7)
}

/// An item tile of the space map. Categories differ by a small step in grey, never by hue.
pub fn tile(category: Category) -> Rgba {
    match category {
        Category::Dependencies => pick(0x002c_2c2c, 0x00cd_cdc7),
        Category::Build => pick(0x0029_2929, 0x00d3_d3cd),
        Category::Cache => pick(0x0026_2626, 0x00d9_d9d3),
        Category::Temp => pick(0x0023_2323, 0x00df_dfd9),
    }
}

/// The thin top edge of an item tile, a second neutral cue for its category.
pub fn tile_edge(category: Category) -> Rgba {
    match category {
        Category::Dependencies => pick(0x0070_7070, 0x008c_8c86),
        Category::Build => pick(0x0058_5858, 0x00a1_a19b),
        Category::Cache => pick(0x0046_4646, 0x00b3_b3ad),
        Category::Temp => pick(0x0038_3838, 0x00c4_c4be),
    }
}

pub fn tile_selected() -> Rgba {
    pick(0x003c_3c3c, 0x00b9_b9b2)
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
