use ratatui::style::Color;

use crate::detect::Category;

pub const MUTED: Color = Color::Indexed(244);
pub const FAINT: Color = Color::Indexed(240);
pub const CURSOR: Color = Color::Indexed(237);
pub const WARNING: Color = Color::Yellow;
pub const SAFE: Color = Color::Green;
pub const SELECTED: Color = Color::Indexed(22);
pub const ON_FILL: Color = Color::Black;
pub const SEARCH_FILL: Color = Color::White;
pub const KEY_FG: Color = Color::White;
pub const KEY_BG: Color = Color::Indexed(238);

pub fn category(category: Category) -> Color {
    match category {
        Category::Dependencies => Color::Cyan,
        Category::Build => Color::Blue,
        Category::Cache => Color::Magenta,
        Category::Temp => Color::Indexed(180),
    }
}

pub fn size(bytes: u64) -> Color {
    match bytes {
        b if b >= 1 << 30 => Color::Red,
        b if b >= 100 << 20 => Color::Yellow,
        _ => Color::Reset,
    }
}
