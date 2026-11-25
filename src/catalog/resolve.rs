use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{Base, Entry, Platform};
use crate::detect::Kind;
use crate::protect::Protected;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Locations {
    pub home: PathBuf,
    pub cache: PathBuf,
    pub config: PathBuf,
    pub data: PathBuf,
    pub state: Option<PathBuf>,
    pub library: Option<PathBuf>,
    pub cargo: PathBuf,
    pub go_modules: PathBuf,
    pub gradle: PathBuf,
    pub bun: PathBuf,
    pub android: PathBuf,
    pub android_avd: PathBuf,
    pub huggingface: PathBuf,
    pub ollama: PathBuf,
}

fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

impl Locations {
    pub fn under(home: &Path) -> Self {
        let cache = home.join(".cache");
        let android = home.join(".android");
        Self {
            home: home.to_path_buf(),
            huggingface: cache.join("huggingface"),
            cache,
            config: home.join(".config"),
            data: home.join(".local/share"),
            state: Some(home.join(".local/state")),
            library: Some(home.join("Library")),
            cargo: home.join(".cargo"),
            go_modules: home.join("go/pkg/mod"),
            gradle: home.join(".gradle"),
            bun: home.join(".bun"),
            android_avd: android.join("avd"),
            android,
            ollama: home.join(".ollama/models"),
        }
    }

    pub fn for_user() -> Option<Self> {
        let home = dirs::home_dir()?;
        let defaults = Self::under(&home);
        let cache = dirs::cache_dir().unwrap_or(defaults.cache);
        let android = env_dir("ANDROID_USER_HOME").unwrap_or(defaults.android);
        let android_avd = env_dir("ANDROID_AVD_HOME").unwrap_or_else(|| android.join("avd"));
        let huggingface = env_dir("HF_HOME").unwrap_or_else(|| cache.join("huggingface"));
        let ollama = env_dir("OLLAMA_MODELS").unwrap_or(defaults.ollama);
        let library = cfg!(target_os = "macos").then(|| home.join("Library"));
        Some(Self {
            cache,
            config: dirs::config_dir().unwrap_or(defaults.config),
            data: dirs::data_dir().unwrap_or(defaults.data),
            state: dirs::state_dir(),
            library,
            cargo: env_dir("CARGO_HOME").unwrap_or(defaults.cargo),
            go_modules: env_dir("GOMODCACHE")
                .or_else(|| env_dir("GOPATH").map(|path| path.join("pkg/mod")))
                .unwrap_or(defaults.go_modules),
            gradle: env_dir("GRADLE_USER_HOME").unwrap_or(defaults.gradle),
            bun: env_dir("BUN_INSTALL").unwrap_or(defaults.bun),
            android,
            android_avd,
            huggingface,
            ollama,
            home,
        })
    }

    pub(crate) fn base(&self, base: Base) -> Option<&Path> {
        match base {
            Base::Home => Some(&self.home),
            Base::Cache => Some(&self.cache),
            Base::Config => Some(&self.config),
            Base::Data => Some(&self.data),
            Base::State => self.state.as_deref(),
            Base::Library => self.library.as_deref(),
            Base::Cargo => Some(&self.cargo),
            Base::GoModules => Some(&self.go_modules),
            Base::Gradle => Some(&self.gradle),
            Base::Bun => Some(&self.bun),
            Base::Android => Some(&self.android),
            Base::AndroidAvd => Some(&self.android_avd),
            Base::HuggingFace => Some(&self.huggingface),
            Base::Ollama => Some(&self.ollama),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolved {
    pub entry: Entry,
    pub path: PathBuf,
}

/// @name current_platform
/// @description Returns the catalog platform for the current operating system.
///
/// @example
/// let _platform = kuizer::catalog::current_platform();
pub fn current_platform() -> Platform {
    if cfg!(target_os = "macos") {
        Platform::Macos
    } else {
        Platform::Linux
    }
}

/// @name resolve
/// @description Resolves catalog `entries` against `locations` for `platform`, expanding `*`
/// directory wildcards, dropping missing paths, symlinks, wrong kinds, refused paths, duplicates
/// and outer directories that contain a more specific resolved entry.
///
/// @example
/// let found = kuizer::catalog::resolve(
///     kuizer::catalog::embedded(),
///     &locations,
///     kuizer::catalog::current_platform(),
///     &protected,
/// );
pub fn resolve(
    entries: &[Entry],
    locations: &Locations,
    platform: Platform,
    protected: &Protected,
) -> Vec<Resolved> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for entry in entries {
        if !entry.platforms.is_empty() && !entry.platforms.contains(&platform) {
            continue;
        }
        let Some(root) = locations.base(entry.base) else {
            continue;
        };
        let mut current = vec![root.to_path_buf()];
        if !entry.path.is_empty() {
            for component in entry.path.split('/') {
                if component == "*" {
                    current = expand_wildcard(&current);
                } else {
                    for path in &mut current {
                        path.push(component);
                    }
                }
                if current.is_empty() {
                    break;
                }
            }
        }
        for path in current {
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            let matches_kind = match entry.kind {
                Kind::Dir => metadata.is_dir(),
                Kind::File => metadata.is_file(),
                Kind::Symlink | Kind::External => false,
            };
            if !matches_kind || protected.refuses(&path) {
                continue;
            }
            if seen.insert(path.clone()) {
                out.push(Resolved {
                    entry: entry.clone(),
                    path,
                });
            }
        }
    }
    drop_outer_nested(out)
}

fn expand_wildcard(parents: &[PathBuf]) -> Vec<PathBuf> {
    let mut next = Vec::new();
    for parent in parents {
        let Ok(read) = fs::read_dir(parent) else {
            continue;
        };
        let mut children = Vec::new();
        for item in read.flatten() {
            let path = item.path();
            if fs::symlink_metadata(&path)
                .is_ok_and(|metadata| !metadata.file_type().is_symlink() && metadata.is_dir())
            {
                children.push(path);
            }
        }
        children.sort();
        next.extend(children);
    }
    next
}

fn drop_outer_nested(resolved: Vec<Resolved>) -> Vec<Resolved> {
    let paths: Vec<PathBuf> = resolved.iter().map(|item| item.path.clone()).collect();
    resolved
        .into_iter()
        .filter(|item| {
            !paths
                .iter()
                .any(|other| other != &item.path && other.starts_with(&item.path))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::parse;

    #[test]
    fn expands_wildcards_to_real_directories_only() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path();
        fs::create_dir_all(home.join(".cache/browser/profile-b/Cache")).expect("b");
        fs::create_dir_all(home.join(".cache/browser/profile-a/Cache")).expect("a");
        fs::create_dir_all(home.join(".cache/browser/profile-empty")).expect("empty");
        let real = home.join("external");
        fs::create_dir_all(real.join("Cache")).expect("real");
        std::os::unix::fs::symlink(&real, home.join(".cache/browser/profile-link")).expect("link");

        let entries = parse(
            r#"
[[entry]]
name = "browser-cache"
app = "Browser"
group = "apps"
base = "cache"
path = "browser/*/Cache"
"#,
            "inline",
        )
        .expect("parses");

        let found = resolve(
            &entries,
            &Locations::under(home),
            Platform::Linux,
            &Protected::new(Some(home)),
        );
        let paths: Vec<PathBuf> = found.into_iter().map(|item| item.path).collect();
        assert_eq!(
            paths,
            [
                home.join(".cache/browser/profile-a/Cache"),
                home.join(".cache/browser/profile-b/Cache"),
            ]
        );
    }

    #[test]
    fn skips_other_platforms_missing_bases_and_wrong_kinds() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path();
        fs::create_dir_all(home.join(".cache/mac-only")).expect("mac");
        fs::create_dir_all(home.join(".cache/dir-not-file")).expect("dir");
        fs::create_dir_all(home.join(".local/state/app")).expect("state");

        let entries = parse(
            r#"
[[entry]]
name = "mac-only"
app = "App"
group = "apps"
base = "cache"
path = "mac-only"
platforms = ["macos"]

[[entry]]
name = "wrong-kind"
app = "App"
group = "apps"
kind = "file"
base = "cache"
path = "dir-not-file"

[[entry]]
name = "missing-path"
app = "App"
group = "apps"
base = "cache"
path = "does-not-exist"

[[entry]]
name = "needs-state"
app = "App"
group = "apps"
base = "state"
path = "app"
"#,
            "inline",
        )
        .expect("parses");

        let mut locations = Locations::under(home);
        locations.state = None;
        let found = resolve(
            &entries,
            &locations,
            Platform::Linux,
            &Protected::new(Some(home)),
        );
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn keeps_the_inner_of_nested_results() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path();
        fs::create_dir_all(home.join(".cache/logs/crash")).expect("nested");

        let entries = parse(
            r#"
[[entry]]
name = "logs-outer"
app = "App"
group = "apps"
base = "cache"
path = "logs"

[[entry]]
name = "logs-inner"
app = "App"
group = "apps"
base = "cache"
path = "logs/crash"
"#,
            "inline",
        )
        .expect("parses");

        let found = resolve(
            &entries,
            &Locations::under(home),
            Platform::Linux,
            &Protected::new(Some(home)),
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].entry.name, "logs-inner");
        assert_eq!(found[0].path, home.join(".cache/logs/crash"));
    }

    #[test]
    fn skips_refused_paths() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path();
        fs::create_dir_all(home.join(".ssh/keys")).expect("ssh");

        let entries = parse(
            r#"
[[entry]]
name = "ssh-keys"
app = "SSH"
group = "system"
base = "home"
path = ".ssh/keys"
"#,
            "inline",
        )
        .expect("parses");

        let found = resolve(
            &entries,
            &Locations::under(home),
            Platform::Linux,
            &Protected::new(Some(home)),
        );
        assert!(found.is_empty());
    }
}
