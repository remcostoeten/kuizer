use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::SystemTime;

use gpui::{Hsla, Pixels, Rems, Rgba, SharedString, Styled, hsla, px, rems, rgb, rgba};
use kuizer::config::{Accent, Density, Theme};
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

    use super::compact;

    /// The vertical padding of rows that grow with their text, such as settings rows.
    pub fn row_y() -> f32 {
        if compact() { XS } else { M }
    }

    /// The vertical padding of a toolbar around its controls.
    pub fn bar_y() -> f32 {
        if compact() { XXS } else { XS }
    }

    /// The space above a section's title.
    pub fn section() -> f32 {
        if compact() { M } else { L }
    }
}

/// The type scale.
pub mod font {
    pub const CAPTION: f32 = 11.;
    pub const SMALL: f32 = 12.;
    pub const BODY: f32 = 13.;
    pub const TITLE: f32 = 15.;
    pub const HEADING: f32 = 18.;
    pub const FIGURE: f32 = 24.;
    pub const DISPLAY: f32 = 32.;
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
    pub const BAR: f32 = 36.;
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

    use super::compact;

    /// The height of a one-line row. Every row of a virtualized list takes its height from here
    /// or from `row_tall`, so the list stays uniform.
    pub fn row() -> f32 {
        if compact() { 24. } else { 28. }
    }

    /// The height of a row with a second line of detail.
    pub fn row_tall() -> f32 {
        if compact() { 32. } else { 40. }
    }

    pub fn toolbar() -> f32 {
        if compact() { 32. } else { 40. }
    }
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
static ACCENT: AtomicUsize = AtomicUsize::new(0);
static COMPACT: AtomicBool = AtomicBool::new(false);

/// @name set_density
/// @description Applies `appearance.density` from the config. Row heights and the paddings in
/// `size` and `space` that are functions follow it on the next render.
///
/// @example
/// set_density(config.appearance.density);
pub fn set_density(density: Density) {
    COMPACT.store(density == Density::Compact, Ordering::Relaxed);
}

fn compact() -> bool {
    COMPACT.load(Ordering::Relaxed)
}

/// @name configure
/// @description Applies `appearance.theme` and `appearance.accent` from the config. Every color
/// below follows them on the next render.
///
/// @example
/// configure(config.appearance.theme, config.appearance.accent);
pub fn configure(theme: Theme, accent: Accent) {
    LIGHT.store(theme == Theme::Light, Ordering::Relaxed);
    let slot = Accent::ALL
        .iter()
        .position(|known| *known == accent)
        .unwrap_or(0);
    ACCENT.store(slot, Ordering::Relaxed);
}

/// The accent-colored parts of the palette, each as a dark and a light value.
struct Tint {
    accent: (u32, u32),
    focus: (u32, u32),
    selected: (u32, u32),
    tile: (u32, u32),
}

fn tint_of(accent: Accent) -> Tint {
    match accent {
        Accent::Purple => Tint {
            accent: (0x007e_64fb, 0x006c_35f5),
            focus: (0x0096_8dfa, 0x006c_35f5),
            selected: (0x001f_1342, 0x00ee_effd),
            tile: (0x0033_2d5d, 0x00cd_cefb),
        },
        Accent::Blue => Tint {
            accent: (0x003b_82f6, 0x0014_63e6),
            focus: (0x005a_96ff, 0x0014_63e6),
            selected: (0x0014_213a, 0x00e3_edff),
            tile: (0x002c_3a55, 0x00bc_d2f5),
        },
        Accent::Teal => Tint {
            accent: (0x000f_9f92, 0x000a_8577),
            focus: (0x0022_b8a9, 0x000a_8577),
            selected: (0x0010_292a, 0x00dc_f5f1),
            tile: (0x0027_474a, 0x00b3_e3dc),
        },
        Accent::Pink => Tint {
            accent: (0x00e8_508f, 0x00d6_246e),
            focus: (0x00f0_66a0, 0x00d6_246e),
            selected: (0x0033_1626, 0x00ff_e5ef),
            tile: (0x004f_2f40, 0x00f2_bdd2),
        },
    }
}

fn tint() -> Tint {
    tint_of(Accent::ALL[ACCENT.load(Ordering::Relaxed)])
}

fn shade(theme: Theme, (dark, light_value): (u32, u32)) -> Rgba {
    pick_in(theme, dark, light_value)
}

/// @name accent_of
/// @description The bar color of an accent in the active theme, for previewing an accent that
/// is not the active one.
///
/// @example
/// let dot = accent_of(Accent::Teal);
pub fn accent_of(accent: Accent) -> Rgba {
    shade(current(), tint_of(accent).accent)
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
    pick_in(theme, 0x000d_0d0d, 0x00ff_ffff)
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
    pick_alpha(0x0000_00a0, 0x1111_1466)
}

fn panel_in(theme: Theme) -> Rgba {
    pick_in(theme, 0x0014_1414, 0x00fa_fafa)
}

pub fn panel() -> Rgba {
    panel_in(current())
}

pub fn elevated() -> Rgba {
    pick(0x001a_1a1a, 0x00ff_ffff)
}

pub fn hover() -> Rgba {
    pick(0x0022_2222, 0x00f0_f0f0)
}

pub fn selected() -> Rgba {
    shade(current(), tint().selected)
}

/// The outline of whatever has the keyboard: the open section, a focused field.
pub fn focus() -> Rgba {
    shade(current(), tint().focus)
}

/// The tint over the row that has the keyboard.
pub fn cursor() -> Rgba {
    pick_alpha(0xffff_ff14, 0x0000_0012)
}

pub fn border() -> Rgba {
    pick(0x0027_2727, 0x00e3_e3e3)
}

pub fn border_strong() -> Rgba {
    pick(0x003d_3d3d, 0x00c7_c7c7)
}

fn text_in(theme: Theme) -> Rgba {
    pick_in(theme, 0x00ff_ffff, 0x000d_0d0d)
}

pub fn text() -> Rgba {
    text_in(current())
}

fn muted_in(theme: Theme) -> Rgba {
    pick_in(theme, 0x00b3_b3b3, 0x004d_4d4d)
}

pub fn muted() -> Rgba {
    muted_in(current())
}

pub fn faint() -> Rgba {
    pick(0x006b_6b6b, 0x008f_8f8f)
}

fn bar_in(theme: Theme) -> Rgba {
    shade(theme, tint().accent)
}

pub fn bar() -> Rgba {
    bar_in(current())
}

/// Text and glyphs drawn on top of the accent color.
pub fn on_accent() -> Rgba {
    rgb(0x00ff_ffff)
}

/// The status dot of an item that is safe to remove.
pub fn ok() -> Rgba {
    pick(0x003f_cf8e, 0x0015_9a5c)
}

pub fn bar_strong() -> Rgba {
    pick(0x00c6_c6c6, 0x0038_3838)
}

pub fn bar_mid() -> Rgba {
    pick(0x0074_7474, 0x0087_8787)
}

pub fn bar_soft() -> Rgba {
    pick(0x0060_6060, 0x00a1_a1a1)
}

pub fn bar_rest() -> Rgba {
    pick(0x0037_3737, 0x00c9_c9c9)
}

pub fn track() -> Rgba {
    pick(0x0025_2525, 0x00e6_e6e6)
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

/// The tint over rows an open delete confirmation covers.
pub fn danger_soft() -> Rgba {
    pick_alpha(0xff66_7f1f, 0xc826_461a)
}

pub fn primary() -> Rgba {
    pick(0x00ff_ffff, 0x000d_0d0d)
}

pub fn on_primary() -> Rgba {
    pick(0x000d_0d0d, 0x00ff_ffff)
}

/// A folder tile of the space map: set back from the items it holds.
pub fn tile_folder() -> Rgba {
    pick(0x001b_1b1b, 0x00f0_f0f0)
}

/// The "n smaller" tile of the space map.
pub fn tile_rest() -> Rgba {
    pick(0x0017_1717, 0x00f4_f4f4)
}

/// An item tile of the space map. Categories differ by a small step in grey, never by hue.
pub fn tile(category: Category) -> Rgba {
    match category {
        Category::Dependencies => pick(0x002a_2a2a, 0x00d3_d3d3),
        Category::Build => pick(0x0027_2727, 0x00d9_d9d9),
        Category::Cache => pick(0x0024_2424, 0x00df_dfdf),
        Category::Temp => pick(0x0021_2121, 0x00e5_e5e5),
    }
}

/// The thin top edge of an item tile, a second neutral cue for its category.
pub fn tile_edge(category: Category) -> Rgba {
    match category {
        Category::Dependencies => pick(0x006e_6e6e, 0x008a_8a8a),
        Category::Build => pick(0x0056_5656, 0x009f_9f9f),
        Category::Cache => pick(0x0044_4444, 0x00b1_b1b1),
        Category::Temp => pick(0x0036_3636, 0x00c2_c2c2),
    }
}

pub fn tile_selected() -> Rgba {
    shade(current(), tint().tile)
}

pub fn radius_button() -> Pixels {
    px(2.)
}

pub fn radius_control() -> Pixels {
    px(2.)
}

pub fn radius_panel() -> Pixels {
    px(4.)
}

pub fn radius_dialog() -> Pixels {
    px(4.)
}

pub fn radius_checkbox() -> Pixels {
    px(2.)
}

pub fn radius_dot() -> Pixels {
    px(1.)
}

pub fn radius_bar() -> Pixels {
    px(1.)
}

pub fn radius_pill() -> Pixels {
    px(999.)
}

pub const FONT_CANDIDATES: [&str; 7] = [
    "Schibsted Grotesk",
    "PP Neue Montreal",
    "Geist",
    "Inter",
    "IBM Plex Sans",
    "Zed Plex Sans",
    "Noto Sans",
];

/// The typefaces for labels, table headings and key badges, in order of preference.
pub const MONO_CANDIDATES: [&str; 7] = [
    "IBM Plex Mono",
    "PP Neue Montreal Mono",
    "Geist Mono",
    "JetBrains Mono",
    "Noto Sans Mono",
    "DejaVu Sans Mono",
    "Menlo",
];

static MONO: OnceLock<SharedString> = OnceLock::new();

/// @name set_mono
/// @description Remembers the monospace typeface the window found installed, for `label`.
///
/// @example
/// set_mono("Geist Mono".into());
pub fn set_mono(font: SharedString) {
    let _ = MONO.set(font);
}

/// @name label
/// @description Styles a small label the way table headings and card titles are set: in the
/// monospace typeface at caption size. The caller passes the text in upper case.
///
/// @example
/// theme::label(div()).child("SIZE")
pub fn label<E: Styled>(element: E) -> E {
    let element = element.text_size(unit(font::CAPTION));
    match MONO.get() {
        Some(font) => element.font_family(font.clone()),
        None => element,
    }
}

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
