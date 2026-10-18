use std::time::SystemTime;

use gpui::{Pixels, Rems, Rgba, px, rems, rgb, rgba};
use kuizer::detect::Category;
use kuizer::units::ByteSize;

pub const REM: f32 = 16.;

pub fn unit(value: f32) -> Rems {
    rems(value / REM)
}

pub fn background() -> Rgba {
    rgb(0x0018_1818)
}

pub fn transparent() -> Rgba {
    rgba(0x0000_0000)
}

pub fn scrim() -> Rgba {
    rgba(0x0000_00a0)
}

pub fn panel() -> Rgba {
    rgb(0x001d_1d1d)
}

pub fn elevated() -> Rgba {
    rgb(0x0023_2323)
}

pub fn hover() -> Rgba {
    rgb(0x0029_2929)
}

pub fn selected() -> Rgba {
    rgb(0x002f_2f2f)
}

pub fn cursor() -> Rgba {
    rgb(0x0090_9090)
}

pub fn border() -> Rgba {
    rgb(0x002a_2a2a)
}

pub fn border_strong() -> Rgba {
    rgb(0x003a_3a3a)
}

pub fn text() -> Rgba {
    rgb(0x00d8_d8d8)
}

pub fn muted() -> Rgba {
    rgb(0x0090_9090)
}

pub fn faint() -> Rgba {
    rgb(0x0060_6060)
}

pub fn bar() -> Rgba {
    rgb(0x008c_8c8c)
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

pub fn skeleton() -> Rgba {
    rgb(0x0027_2727)
}

pub fn track() -> Rgba {
    rgb(0x0026_2626)
}

pub fn review() -> Rgba {
    rgb(0x00c2_a36e)
}

pub fn review_soft() -> Rgba {
    rgba(0xc2a3_6e24)
}

pub fn danger() -> Rgba {
    rgb(0x00d0_7f7f)
}

pub fn primary() -> Rgba {
    rgb(0x00e6_e6e6)
}

pub fn on_primary() -> Rgba {
    rgb(0x0016_1616)
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
