use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use crate::detect::{APP_DATA_MARKERS, Detector, Listing};

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
];

const CREDENTIAL_DIRS: &[&str] = &[
    ".ssh",
    ".gnupg",
    ".password-store",
    ".pki",
    ".local/share/keyrings",
    ".local/share/kwalletd",
    "Library/Keychains",
];

const PROFILE_CACHES: &[&str] = &[
    "Cache",
    "Code Cache",
    "GPUCache",
    "DawnCache",
    "DawnGraphiteCache",
    "DawnWebGPUCache",
    "GraphiteDawnCache",
    "GrShaderCache",
    "ShaderCache",
    "cache2",
    "startupCache",
    "CachedData",
    "CachedExtensionVSIXs",
];

const PROFILE_GUARD_FILES: &[&str] = &[
    "Local State",
    "Cookies",
    "Login Data",
    "cookies.sqlite",
    "logins.json",
    "key4.db",
];

const VOLTA_TOOLS: &str = ".volta/tools";

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
    VOLTA_TOOLS,
    ".local/share/nvim",
    ".config/coc/extensions",
];

pub const SYSTEM_PATHS: &[&str] = &[
    "/bin",
    "/boot",
    "/dev",
    "/efi",
    "/etc",
    "/lib",
    "/lib32",
    "/lib64",
    "/nix",
    "/opt",
    "/proc",
    "/root",
    "/run",
    "/sbin",
    "/snap",
    "/srv",
    "/sys",
    "/usr",
    "/var/lib",
    "/var/log",
    "/Applications",
    "/Library",
    "/System",
    "/private/etc",
    "/private/var/db",
];

/// @name holds_app_data
/// @description Reports whether a directory listing belongs to a browser or Electron profile:
/// a Chromium user data directory (`Local State`), a Firefox profile root (`profiles.ini`), or a
/// single Chromium or Firefox profile with its cookie store.
///
/// @example
/// if protect::holds_app_data(&detector, &listing) {
///     return;
/// }
pub fn holds_app_data(detector: &Detector, listing: &Listing) -> bool {
    APP_DATA_MARKERS
        .iter()
        .any(|set| set.iter().all(|name| detector.lists(listing, name)))
}

fn marked_on_disk(dir: &Path) -> bool {
    APP_DATA_MARKERS.iter().any(|set| {
        set.iter()
            .all(|name| std::fs::symlink_metadata(dir.join(name)).is_ok())
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MarkerStamp {
    device: u64,
    inode: u64,
    modified: SystemTime,
    changed: (i64, i64),
}

impl MarkerStamp {
    fn from_metadata(metadata: &std::fs::Metadata) -> Option<Self> {
        use std::os::unix::fs::MetadataExt;

        Some(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            modified: metadata.modified().ok()?,
            changed: (metadata.ctime(), metadata.ctime_nsec()),
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct Protected {
    dirs: HashSet<PathBuf>,
    names: HashSet<OsString>,
    homes: Vec<PathBuf>,
    never: Vec<PathBuf>,
    exempt: Vec<PathBuf>,
    extra: Vec<PathBuf>,
    marker_cache: Arc<Mutex<HashMap<PathBuf, (MarkerStamp, bool)>>>,
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
        let mut never: Vec<PathBuf> = Vec::new();
        for home in &homes {
            dirs.extend(
                HOME_DIRS
                    .iter()
                    .chain(CREDENTIAL_DIRS)
                    .chain(TOOL_DIRS)
                    .map(|dir| home.join(dir)),
            );
            never.extend(
                CREDENTIAL_DIRS
                    .iter()
                    .chain(TOOL_DIRS)
                    .map(|dir| home.join(dir)),
            );
            for parent in [".config", ".cache", ".local/share"] {
                dirs.extend(BROWSERS.iter().map(|name| home.join(parent).join(name)));
            }
        }
        let names = dirs
            .iter()
            .filter_map(|dir| dir.file_name().map(OsString::from))
            .collect();
        Self {
            dirs,
            names,
            homes,
            never,
            exempt: Vec::new(),
            extra: Vec::new(),
            marker_cache: Arc::default(),
        }
    }

    pub fn for_user() -> Self {
        Self::new(dirs::home_dir().as_deref())
    }

    /// @name with_paths
    /// @description Adds `extra` absolute directories to the protected ones, together with their
    /// symlink-resolved form, so walkers never enter them and deletion refuses them and
    /// everything below. Relative paths are ignored. The built-in directories stay protected
    /// whatever is passed.
    ///
    /// @example
    /// let protected = Protected::for_user().with_paths(&[PathBuf::from("/home/me/thesis")]);
    pub fn with_paths(mut self, extra: &[PathBuf]) -> Self {
        for path in extra.iter().filter(|path| path.is_absolute()) {
            let resolved = std::fs::canonicalize(path).ok();
            for dir in std::iter::once(path.clone()).chain(resolved) {
                if let Some(name) = dir.file_name() {
                    self.names.insert(name.to_owned());
                }
                self.dirs.insert(dir);
            }
            self.extra.push(path.clone());
        }
        self
    }

    /// @name Protected::protects_system_path
    /// @description Returns whether `path` lies under a built-in operating-system path that
    /// deletion refuses.
    ///
    /// @example
    /// if Protected::protects_system_path(path) { return Err(Refusal::Protected); }
    pub fn protects_system_path(path: &Path) -> bool {
        SYSTEM_PATHS.iter().any(|prefix| path.starts_with(prefix))
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
        path.file_name()
            .is_some_and(|name| self.names.contains(name))
            && self.dirs.contains(path)
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
        let target = std::fs::symlink_metadata(path).ok();
        let target_dir = target
            .as_ref()
            .filter(|metadata| metadata.is_dir())
            .cloned()
            .or_else(|| {
                target
                    .as_ref()
                    .filter(|metadata| metadata.file_type().is_symlink())
                    .and_then(|_| std::fs::metadata(path).ok())
            });
        if self
            .extra
            .iter()
            .any(|protected| path.starts_with(protected))
        {
            return true;
        }
        for dir in path.ancestors() {
            if self.dirs.contains(dir) && !self.exempts(dir, path) {
                return true;
            }
            let metadata = if dir == path {
                target_dir.clone()
            } else {
                std::fs::metadata(dir).ok()
            };
            if let Some(metadata) = metadata
                && metadata.is_dir()
                && self.marked_on_disk(dir, &metadata)
            {
                return true;
            }
        }
        false
    }

    /// @name refuses
    /// @description Checks whether kuizer must leave `path` alone because it lies inside protected
    /// application data. A known disk cache folder directly inside a browser or Electron profile,
    /// such as `GPUCache` or Firefox's `cache2`, is the one exception; nothing inside credential
    /// or tool folders is ever allowed.
    ///
    /// @example
    /// if protected.refuses(&found) {
    ///     continue;
    /// }
    pub fn refuses(&self, path: &Path) -> bool {
        self.covers(path) && !self.is_profile_cache(path)
    }

    fn is_profile_cache(&self, path: &Path) -> bool {
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            return false;
        };
        if !PROFILE_CACHES.contains(&name) {
            return false;
        }
        if self.never.iter().any(|dir| path.starts_with(dir))
            || self.extra.iter().any(|dir| path.starts_with(dir))
        {
            return false;
        }
        if self.dirs.contains(path) {
            return false;
        }
        let Ok(metadata) = std::fs::symlink_metadata(path) else {
            return false;
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return false;
        }
        let Some(parent) = path.parent() else {
            return false;
        };
        let Ok(parent_meta) = std::fs::symlink_metadata(parent) else {
            return false;
        };
        if parent_meta.file_type().is_symlink() || !parent_meta.is_dir() {
            return false;
        }
        if self.marked_on_disk(path, &metadata) {
            return false;
        }
        !PROFILE_GUARD_FILES
            .iter()
            .any(|guard_file| std::fs::symlink_metadata(path.join(guard_file)).is_ok())
    }

    fn marked_on_disk(&self, dir: &Path, metadata: &std::fs::Metadata) -> bool {
        let Some(stamp) = MarkerStamp::from_metadata(metadata) else {
            return marked_on_disk(dir);
        };
        if let Ok(cache) = self.marker_cache.lock()
            && let Some((cached_stamp, marked)) = cache.get(dir)
            && *cached_stamp == stamp
        {
            return *marked;
        }
        let marked = marked_on_disk(dir);
        if let Ok(mut cache) = self.marker_cache.lock() {
            cache.insert(dir.to_path_buf(), (stamp, marked));
        }
        marked
    }

    fn exempts(&self, dir: &Path, path: &Path) -> bool {
        self.exempt
            .iter()
            .any(|exempt| exempt == dir && path.starts_with(exempt))
    }

    pub(crate) fn allowing_volta_tools(mut self) -> Self {
        self.exempt = self
            .homes
            .iter()
            .map(|home| home.join(VOLTA_TOOLS))
            .collect();
        self
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
    use std::fs;

    use super::*;
    use crate::detect::builtin_rules;

    #[test]
    fn the_volta_exemption_still_refuses_browser_data() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path();
        let node = home.join(".volta/tools/image/node/20.0.0");
        let profile = home.join(".config/google-chrome/Default");
        fs::create_dir_all(&node).expect("node");
        fs::create_dir_all(&profile).expect("profile");
        let protected = Protected::new(Some(home));
        assert!(protected.covers(&node));
        let allowed = protected.allowing_volta_tools();
        assert!(!allowed.covers(&node));
        assert!(allowed.covers(&profile));
    }

    #[test]
    fn recognizes_profile_listings() {
        let detector = Detector::new(&builtin_rules()).expect("builtin rules");
        let holds = |names: &[&str]| holds_app_data(&detector, &detector.listing(names));
        assert!(holds(&["Default", "Local State"]));
        assert!(holds(&["profiles.ini", "abc.default"]));
        assert!(holds(&["Cookies", "Preferences", "History"]));
        assert!(holds(&["cookies.sqlite", "prefs.js"]));
        assert!(!holds(&["Cookies", "src", "package.json"]));
        assert!(!holds(&["node_modules", "package.json"]));
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
    fn marker_cache_tracks_directory_changes() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let app = tmp.path().join("app");
        let child = app.join("data");
        fs::create_dir_all(&child).expect("app");
        let protected = Protected::default();

        assert!(!protected.covers(&child));
        fs::write(app.join("Local State"), "{}").expect("marker");
        assert!(protected.covers(&child));
        fs::remove_file(app.join("Local State")).expect("remove marker");
        assert!(!protected.covers(&child));
    }

    #[test]
    fn extra_paths_add_to_the_built_in_ones() {
        let home = Path::new("/home/me");
        let protected = Protected::new(Some(home))
            .with_paths(&[home.join("thesis"), PathBuf::from("relative/dir")]);
        assert!(protected.is_known(&home.join("thesis")));
        assert!(protected.covers(&home.join("thesis/figures/node_modules")));
        assert!(!protected.covers(&home.join("thesis-old/node_modules")));
        assert!(!protected.covers(Path::new("relative/dir/x")));
        assert!(protected.is_known(&home.join(".ssh")));
    }

    #[test]
    fn configured_paths_are_protected_and_relative_paths_are_ignored() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let protected_path = tmp.path().join("important");
        fs::create_dir(&protected_path).expect("protected directory");
        let future_path = tmp.path().join("future");
        let protected = Protected::default().with_paths(&[
            protected_path.clone(),
            future_path.clone(),
            PathBuf::from("relative/path"),
        ]);
        assert!(protected.covers(&protected_path.join("file")));
        assert!(protected.covers(&future_path.join("file")));
        assert!(!protected.covers(Path::new("relative/path/file")));
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

    #[test]
    fn allows_only_cache_folders_inside_chromium_profiles() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path();
        let chrome = home.join(".config/google-chrome");
        let profile = chrome.join("Default");
        let network = profile.join("Network");
        let gpu = profile.join("GPUCache");
        let code_cache = profile.join("Code Cache");
        let http_cache = home.join(".cache/google-chrome/Default/Cache");
        let cache_data = http_cache.join("Cache_Data");
        fs::create_dir_all(&network).expect("network");
        fs::create_dir_all(&gpu).expect("gpu");
        fs::create_dir_all(code_cache.join("js")).expect("code cache");
        fs::create_dir_all(&cache_data).expect("cache data");
        fs::write(chrome.join("Local State"), "{}").expect("local state");
        fs::write(profile.join("Preferences"), "{}").expect("prefs");
        fs::write(network.join("Cookies"), "cookies").expect("cookies");
        fs::write(gpu.join("data_0"), "shader").expect("shader");
        fs::write(code_cache.join("js/index"), "idx").expect("idx");
        fs::write(cache_data.join("f_000001"), "body").expect("body");

        let protected = Protected::new(Some(home));
        assert!(!protected.refuses(&gpu));
        assert!(!protected.refuses(&code_cache));
        assert!(!protected.refuses(&http_cache));

        assert!(protected.refuses(&chrome));
        assert!(protected.refuses(&profile));
        assert!(protected.refuses(&network));
        assert!(protected.refuses(&network.join("Cookies")));
        assert!(protected.refuses(&gpu.join("data_0")));
        assert!(protected.refuses(&cache_data));
    }

    #[test]
    fn allows_electron_and_firefox_caches() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path();
        let discord = home.join(".config/discord");
        let discord_cache = discord.join("Cache");
        fs::create_dir_all(&discord_cache).expect("discord");
        fs::write(discord.join("Local State"), "{}").expect("local state");
        fs::write(discord.join("Cookies"), "c").expect("cookies");
        fs::write(discord.join("Preferences"), "{}").expect("prefs");
        fs::write(discord_cache.join("x"), "x").expect("x");

        let ff_profile = home.join(".mozilla/firefox/ab.default");
        fs::create_dir_all(&ff_profile).expect("ff profile");
        fs::write(ff_profile.join("prefs.js"), "").expect("prefs");
        fs::write(ff_profile.join("cookies.sqlite"), "").expect("cookies");

        let ff_cache = home.join(".cache/mozilla/firefox/ab.default/cache2");
        fs::create_dir_all(ff_cache.join("entries")).expect("ff cache");
        fs::write(ff_cache.join("entries/x"), "x").expect("entry");

        let protected = Protected::new(Some(home));
        assert!(!protected.refuses(&discord_cache));
        assert!(protected.refuses(&ff_profile));
        assert!(!protected.refuses(&ff_cache));
    }

    #[test]
    fn never_allows_credentials_tools_or_profile_lookalikes() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path();
        let ssh_cache = home.join(".ssh/Cache");
        let keyring_cache = home.join(".local/share/keyrings/Cache");
        let ext_cache = home.join(".vscode/extensions/x/Cache");
        let fake_cache = home.join(".config/discord/Cache");
        let real_target = home.join("outside");
        let chrome_profile = home.join(".config/google-chrome/Default");
        let link_gpu = chrome_profile.join("GPUCache");

        fs::create_dir_all(&ssh_cache).expect("ssh");
        fs::create_dir_all(&keyring_cache).expect("keyring");
        fs::create_dir_all(&ext_cache).expect("ext");
        fs::create_dir_all(&fake_cache).expect("fake");
        fs::write(home.join(".config/discord/Local State"), "{}").expect("discord state");
        fs::write(fake_cache.join("Local State"), "{}").expect("nested state");
        fs::create_dir_all(&real_target).expect("outside");
        fs::create_dir_all(&chrome_profile).expect("chrome");
        std::os::unix::fs::symlink(&real_target, &link_gpu).expect("link");

        let protected = Protected::new(Some(home));
        assert!(protected.refuses(&ssh_cache));
        assert!(protected.refuses(&keyring_cache));
        assert!(protected.refuses(&ext_cache));
        assert!(protected.refuses(&fake_cache));
        assert!(protected.refuses(&link_gpu));
    }
}
