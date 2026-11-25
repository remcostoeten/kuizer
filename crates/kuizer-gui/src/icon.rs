use std::borrow::Cow;

use gpui::{AnyElement, AssetSource, Rgba, SharedString, prelude::*, svg};

use crate::theme::{size, unit};

/// A monochrome line icon. Icons take the text color they are given, never a hue of their own.
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
    Folder,
    Settings,
    Check,
}

impl Icon {
    pub const ALL: [Icon; 14] = [
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
        Self::Folder,
        Self::Settings,
        Self::Check,
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
            Self::Folder => "folder",
            Self::Settings => "settings",
            Self::Check => "check",
        }
    }

    fn fragment(self) -> &'static str {
        match self {
            Self::Overview => {
                r#"<rect width="7" height="9" x="3" y="3" rx="1"/><rect width="7" height="5" x="14" y="3" rx="1"/><rect width="7" height="9" x="14" y="12" rx="1"/><rect width="7" height="5" x="3" y="16" rx="1"/>"#
            }
            Self::Artifacts => {
                r#"<path d="M11 21.73a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73z"/><path d="M12 22V12"/><path d="m3.3 7 8.7 5 8.7-5"/><path d="m7.5 4.27 9 5.15"/>"#
            }
            Self::Caches => {
                r#"<ellipse cx="12" cy="5" rx="9" ry="3"/><path d="M3 12A9 3 0 0 0 21 12"/><path d="M3 5V19A9 3 0 0 0 21 19V5"/>"#
            }
            Self::Toolchains => {
                r#"<path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z"/>"#
            }
            Self::Docker => {
                r#"<path d="M12.83 2.18a2 2 0 0 0-1.66 0L2.6 6.08a1 1 0 0 0 0 1.83l8.58 3.91a2 2 0 0 0 1.66 0l8.58-3.9a1 1 0 0 0 0-1.83Z"/><path d="m22 12.65-9.17 4.16a2 2 0 0 1-1.66 0L2 12.65"/><path d="m22 17.65-9.17 4.16a2 2 0 0 1-1.66 0L2 17.65"/>"#
            }
            Self::Repos => {
                r#"<line x1="6" x2="6" y1="3" y2="15"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="6" r="3"/><path d="M18 9a9 9 0 0 1-9 9"/>"#
            }
            Self::Duplicates => {
                r#"<path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/><rect width="14" height="14" x="8" y="8" rx="2" ry="2"/>"#
            }
            Self::History => {
                r#"<path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/><path d="M12 7v5l4 2"/>"#
            }
            Self::Issues => {
                r#"<path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3"/><path d="M12 9v4"/><path d="M12 17h.01"/>"#
            }
            Self::Sidebar => {
                r#"<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M9 3v18"/>"#
            }
            Self::Search => r#"<circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/>"#,
            Self::Folder => {
                r#"<path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>"#
            }
            Self::Settings => {
                r#"<path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/>"#
            }
            Self::Check => r#"<path stroke-width="3.5" d="M4.5 12.5 9.5 17.5 19.5 6.5"/>"#,
        }
    }

    fn path(self) -> SharedString {
        format!("icons/{}.svg", self.name()).into()
    }
}

/// @name view
/// @description Draws an icon at the standard icon size in the given color.
///
/// @example
/// icon::view(Icon::History, theme::muted())
pub fn view(icon: Icon, color: Rgba) -> AnyElement {
    sized(icon, color, size::ICON)
}

/// @name sized
/// @description Draws an icon like `view`, at `extent` instead of the standard icon size, for a
/// glyph inside a checkbox or a small button.
///
/// @example
/// icon::sized(Icon::Check, theme::on_accent(), size::GLYPH)
pub fn sized(icon: Icon, color: Rgba, extent: f32) -> AnyElement {
    svg()
        .path(icon.path())
        .text_color(color)
        .flex_none()
        .size(unit(extent))
        .into_any_element()
}

fn source(fragment: &str) -> Vec<u8> {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">{fragment}</svg>"#
    )
    .into_bytes()
}

fn icon_source(path: &str) -> Option<Vec<u8>> {
    let name = path.strip_prefix("icons/")?.strip_suffix(".svg")?;
    let icon = Icon::ALL.into_iter().find(|icon| icon.name() == name)?;
    Some(source(icon.fragment()))
}

/// The icons, built into the binary and served to GPUI's `svg()` element.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        Ok(icon_source(path).map(Cow::Owned))
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(Icon::ALL
            .into_iter()
            .map(Icon::path)
            .filter(|candidate| candidate.starts_with(path))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_is_served_as_an_svg() {
        for icon in Icon::ALL {
            let bytes = Assets.load(&icon.path()).expect("loads").expect("exists");
            assert!(bytes.starts_with(b"<svg"), "{icon:?}");
        }
        assert!(Assets.load("icons/missing.svg").expect("loads").is_none());
        assert_eq!(Assets.list("icons/").expect("lists").len(), Icon::ALL.len());
    }
}
