use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::detect::Listing;

const MARKER_SETS: &[&[&str]] = &[
    &["Local State"],
    &["profiles.ini"],
    &["Cookies", "Preferences"],
    &["cookies.sqlite", "prefs.js"],
];

const BROWSERS: &[&str] = &[
    "BraveSoftware",
    "google-chrome",
    "google-chrome-beta",
    "google-chrome-unstable",
    "chromium",
    "microsoft-edge",
    "microsoft-edge-beta",
    "microsoft-edge-dev",
    "vivaldi",
    "opera",
    "opera-beta",
    "thorium",
    "mozilla",
    "librewolf",
    "zen",
    "floorp",
    "waterfox",
];

const HOME_DIRS: &[&str] = &[
    ".mozilla",
    ".librewolf",
    ".waterfox",
    ".floorp",
    ".zen",
    ".thunderbird",
    ".ssh",
    ".gnupg",
    ".password-store",
    ".pki",
    ".local/share/keyrings",
    ".local/share/kwalletd",
    ".var/app/com.brave.Browser",
    ".var/app/com.google.Chrome",
    ".var/app/org.chromium.Chromium",
    ".var/app/com.microsoft.Edge",
    ".var/app/com.vivaldi.Vivaldi",
    ".var/app/com.opera.Opera",
    ".var/app/org.mozilla.firefox",
    ".var/app/io.gitlab.librewolf-community",
    ".var/app/app.zen_browser.zen",
    "snap/firefox",
    "snap/chromium",
    "snap/brave",
    "Library/Application Support/BraveSoftware",
    "Library/Application Support/Google/Chrome",
    "Library/Application Support/Chromium",
    "Library/Application Support/Microsoft Edge",
    "Library/Application Support/Vivaldi",
    "Library/Application Support/Arc",
    "Library/Application Support/Firefox",
    "Library/Caches/BraveSoftware",
    "Library/Caches/Google/Chrome",
    "Library/Caches/Firefox",
    "Library/Keychains",
];

const TOOL_DIRS: &[&str] = &[
    ".vscode/extensions",
    ".vscode-insiders/extensions",
    ".vscode-oss/extensions",
    ".vscode-server",
    ".vscode-server-insiders",
    ".cursor/extensions",
    ".cursor-server",
    ".windsurf/extensions",
    ".windsurf-server",
    ".local/share/pnpm/global",
    ".bun/install/global",
    ".config/yarn/global",
    ".yarn/global",
    ".npm-global",
    ".volta/tools",
    ".local/share/nvim",
    ".config/coc/extensions",
];

/// @name holds_app_data
/// @description Reports whether a directory listing belongs to a browser or Electron profile:
/// a Chromium user data directory (`Local State`), a Firefox profile root (`profiles.ini`), or a
/// single Chromium or Firefox profile with its cookie store.
///
/// @example
/// if protect::holds_app_data(&listing) {
///     return;
/// }
pub fn holds_app_data(listing: &Listing) -> bool {
    MARKER_SETS
        .iter()
        .any(|set| set.iter().all(|name| listing.has(OsStr::new(name))))
}

fn marked_on_disk(dir: &Path) -> bool {
    MARKER_SETS.iter().any(|set| {
        set.iter()
            .all(|name| std::fs::symlink_metadata(dir.join(name)).is_ok())
    })
}

#[derive(Debug, Clone, Default)]
pub struct Protected {
    dirs: HashSet<PathBuf>,
    homes: Vec<PathBuf>,
}

impl Protected {
    pub fn new(home: Option<&Path>) -> Self {
        let Some(home) = home else {
            return Self::default();
        };
        let mut homes = vec![home.to_path_buf()];
        if let Ok(resolved) = std::fs::canonicalize(home)
            && resolved != home
        {
            homes.push(resolved);
        }
        let mut dirs: HashSet<PathBuf> = HashSet::new();
        for home in &homes {
            dirs.extend(HOME_DIRS.iter().chain(TOOL_DIRS).map(|dir| home.join(dir)));
            for parent in [".config", ".cache", ".local/share"] {
                dirs.extend(BROWSERS.iter().map(|name| home.join(parent).join(name)));
            }
        }
        Self { dirs, homes }
    }

    pub fn for_user() -> Self {
        Self::new(dirs::home_dir().as_deref())
    }

    /// @name is_known
    /// @description Checks whether `path` is exactly one of the known browser, keyring or
    /// credential directories. Walkers call it per directory, top-down, so descendants are never
    /// reached.
    ///
    /// @example
    /// if protected.is_known(&child) {
    ///     continue;
    /// }
    pub fn is_known(&self, path: &Path) -> bool {
        self.dirs.contains(path)
    }

    /// @name covers
    /// @description Checks whether `path` lies inside protected application data: inside a known
    /// browser, credential or tool install directory, or under a directory that holds browser or Electron
    /// profile markers on disk.
    ///
    /// @example
    /// if protected.covers(&target.path) {
    ///     return Err(Refusal::AppData);
    /// }
    pub fn covers(&self, path: &Path) -> bool {
        path.ancestors()
            .any(|dir| self.dirs.contains(dir) || marked_on_disk(dir))
    }

    /// @name in_hidden_home_dir
    /// @description Checks whether `path` lies inside a hidden folder directly under the home
    /// directory, such as `~/.config` or `~/.local`. Tools and editors install their own
    /// dependencies there, so matches in those places should never be offered as safe.
    ///
    /// @example
    /// if protected.in_hidden_home_dir(&found.path) {
    ///     safety = Safety::Review;
    /// }
    pub fn in_hidden_home_dir(&self, path: &Path) -> bool {
        self.homes
            .iter()
            .filter_map(|home| path.strip_prefix(home).ok())
            .filter_map(|rest| rest.components().next())
            .any(|first| first.as_os_str().to_string_lossy().starts_with('.'))
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;

    use super::*;

    fn listing(names: &[&str]) -> Listing {
        Listing::new(names.iter().map(OsString::from).collect())
    }

    #[test]
    fn recognizes_profile_listings() {
        assert!(holds_app_data(&listing(&["Default", "Local State"])));
        assert!(holds_app_data(&listing(&["profiles.ini", "abc.default"])));
        assert!(holds_app_data(&listing(&[
            "Cookies",
            "Preferences",
            "History"
        ])));
        assert!(holds_app_data(&listing(&["cookies.sqlite", "prefs.js"])));
        assert!(!holds_app_data(&listing(&[
            "Cookies",
            "src",
            "package.json"
        ])));
        assert!(!holds_app_data(&listing(&["node_modules", "package.json"])));
    }

    #[test]
    fn covers_known_dirs_and_marked_ancestors() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path();
        let protected = Protected::new(Some(home));
        assert!(protected.is_known(&home.join(".config/BraveSoftware")));
        assert!(protected.covers(&home.join(".config/BraveSoftware/Brave-Browser/Default")));
        assert!(protected.covers(&home.join(".cache/mozilla/firefox/x/cache2")));
        assert!(!protected.covers(&home.join("apps/chromium/out")));

        let app = home.join(".config/discord");
        fs::create_dir_all(app.join("modules/node_modules")).expect("dirs");
        fs::write(app.join("Local State"), "{}").expect("marker");
        assert!(protected.covers(&app.join("modules/node_modules")));
        assert!(!protected.covers(&home.join(".config/other/node_modules")));
    }

    #[test]
    fn covers_global_tool_installs() {
        let home = Path::new("/home/me");
        let protected = Protected::new(Some(home));
        for path in [
            ".vscode/extensions/esbenp.prettier-vscode-11.0.0/node_modules",
            ".vscode-server/extensions/x/node_modules",
            ".cursor/extensions/x/node_modules",
            ".local/share/pnpm/global/5/node_modules",
            ".bun/install/global/node_modules",
            ".config/yarn/global/node_modules",
        ] {
            assert!(protected.covers(&home.join(path)), "{path}");
        }
        assert!(protected.is_known(&home.join(".vscode/extensions")));
        assert!(!protected.covers(&home.join(".local/share/pnpm/store")));
        assert!(!protected.covers(&home.join(".bun/install/cache")));
        assert!(!protected.covers(&home.join("code/web/node_modules")));
    }

    #[cfg(unix)]
    #[test]
    fn protects_resolved_paths_of_a_symlinked_home() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let real = fs::canonicalize(tmp.path())
            .expect("canonical")
            .join("real");
        fs::create_dir_all(&real).expect("real home");
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&real, &link).expect("symlink");

        let protected = Protected::new(Some(&link));
        assert!(protected.is_known(&real.join(".ssh")));
        assert!(protected.is_known(&link.join(".ssh")));
        assert!(protected.covers(&real.join(".gnupg/private-keys-v1.d")));
        assert!(protected.in_hidden_home_dir(&real.join(".config/x/node_modules")));
    }

    #[test]
    fn detects_hidden_home_dirs() {
        let protected = Protected::new(Some(Path::new("/home/me")));
        assert!(protected.in_hidden_home_dir(Path::new("/home/me/.config/tool/node_modules")));
        assert!(!protected.in_hidden_home_dir(Path::new("/home/me/code/.hidden/node_modules")));
        assert!(!protected.in_hidden_home_dir(Path::new("/srv/.cache/node_modules")));
        assert!(!Protected::default().in_hidden_home_dir(Path::new("/home/me/.config/x")));
    }
}
