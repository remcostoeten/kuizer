use std::borrow::Cow;

use gpui::{AssetSource, Rgba, SharedString, Svg, prelude::*, svg};

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
}

impl Icon {
    pub const ALL: [Icon; 10] = [
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
    ];

    pub fn path(self) -> &'static str {
        match self {
            Self::Overview => "icons/overview.svg",
            Self::Artifacts => "icons/artifacts.svg",
            Self::Caches => "icons/caches.svg",
            Self::Toolchains => "icons/toolchains.svg",
            Self::Docker => "icons/docker.svg",
            Self::Repos => "icons/repos.svg",
            Self::Duplicates => "icons/duplicates.svg",
            Self::History => "icons/history.svg",
            Self::Issues => "icons/issues.svg",
            Self::Sidebar => "icons/sidebar.svg",
        }
    }

    fn source(self) -> &'static [u8] {
        match self {
            Self::Overview => include_bytes!("../assets/icons/overview.svg"),
            Self::Artifacts => include_bytes!("../assets/icons/artifacts.svg"),
            Self::Caches => include_bytes!("../assets/icons/caches.svg"),
            Self::Toolchains => include_bytes!("../assets/icons/toolchains.svg"),
            Self::Docker => include_bytes!("../assets/icons/docker.svg"),
            Self::Repos => include_bytes!("../assets/icons/repos.svg"),
            Self::Duplicates => include_bytes!("../assets/icons/duplicates.svg"),
            Self::History => include_bytes!("../assets/icons/history.svg"),
            Self::Issues => include_bytes!("../assets/icons/issues.svg"),
            Self::Sidebar => include_bytes!("../assets/icons/sidebar.svg"),
        }
    }
}

/// @name view
/// @description Draws an icon at the standard icon size in the given color.
///
/// @example
/// icon::view(Icon::History, theme::muted())
pub fn view(icon: Icon, color: Rgba) -> Svg {
    svg()
        .path(icon.path())
        .flex_none()
        .size(unit(size::ICON))
        .text_color(color)
}

/// The icons compiled into the binary, served to GPUI's `svg()` element.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        Ok(Icon::ALL
            .into_iter()
            .find(|icon| icon.path() == path)
            .map(|icon| Cow::Borrowed(icon.source())))
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(Icon::ALL
            .into_iter()
            .map(Icon::path)
            .filter(|candidate| candidate.starts_with(path))
            .map(SharedString::from)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_is_served_as_an_svg() {
        for icon in Icon::ALL {
            let bytes = Assets.load(icon.path()).expect("loads").expect("exists");
            assert!(bytes.starts_with(b"<svg"), "{icon:?}");
        }
        assert!(Assets.load("icons/missing.svg").expect("loads").is_none());
        assert_eq!(Assets.list("icons/").expect("lists").len(), Icon::ALL.len());
    }
}
