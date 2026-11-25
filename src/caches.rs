use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::config::{Config, Excludes};
use crate::detect::{Category, Kind, RuleInfo, Safety};
use crate::protect::Protected;
use crate::scan::{self, Cancel, Candidate, Usage};

#[derive(Debug, Clone, Copy)]
enum Base {
    Home,
    Cache,
    Data,
    Cargo,
    GoModules,
    Gradle,
    Bun,
    Android,
    AndroidAvd,
    HuggingFace,
    Ollama,
}

struct Spec {
    name: &'static str,
    tool: &'static str,
    base: Base,
    relative: &'static str,
    safety: Safety,
    note: &'static str,
}

const SPECS: &[Spec] = &[
    Spec {
        name: "npm",
        tool: "npm",
        base: Base::Home,
        relative: ".npm/_cacache",
        safety: Safety::Safe,
        note: "",
    },
    Spec {
        name: "npx",
        tool: "npm",
        base: Base::Home,
        relative: ".npm/_npx",
        safety: Safety::Safe,
        note: "",
    },
    Spec {
        name: "pnpm-store",
        tool: "pnpm",
        base: Base::Data,
        relative: "pnpm/store",
        safety: Safety::Review,
        note: "node_modules folders hard-link into the store, so only packages no project uses are freed; `pnpm store prune` removes just those",
    },
    Spec {
        name: "pnpm-metadata",
        tool: "pnpm",
        base: Base::Cache,
        relative: "pnpm",
        safety: Safety::Safe,
        note: "",
    },
    Spec {
        name: "yarn",
        tool: "yarn",
        base: Base::Cache,
        relative: "yarn",
        safety: Safety::Safe,
        note: "",
    },
    Spec {
        name: "yarn-berry",
        tool: "yarn",
        base: Base::Home,
        relative: ".yarn/berry/cache",
        safety: Safety::Safe,
        note: "",
    },
    Spec {
        name: "bun",
        tool: "bun",
        base: Base::Bun,
        relative: "install/cache",
        safety: Safety::Safe,
        note: "",
    },
    Spec {
        name: "deno",
        tool: "deno",
        base: Base::Cache,
        relative: "deno",
        safety: Safety::Safe,
        note: "",
    },
    Spec {
        name: "node-gyp",
        tool: "node",
        base: Base::Cache,
        relative: "node-gyp",
        safety: Safety::Safe,
        note: "",
    },
    Spec {
        name: "typescript",
        tool: "typescript",
        base: Base::Cache,
        relative: "typescript",
        safety: Safety::Safe,
        note: "",
    },
    Spec {
        name: "electron",
        tool: "electron",
        base: Base::Cache,
        relative: "electron",
        safety: Safety::Safe,
        note: "",
    },
    Spec {
        name: "playwright",
        tool: "playwright",
        base: Base::Cache,
        relative: "ms-playwright",
        safety: Safety::Review,
        note: "test browser binaries without user profiles; `npx playwright install` restores them",
    },
    Spec {
        name: "puppeteer",
        tool: "puppeteer",
        base: Base::Cache,
        relative: "puppeteer",
        safety: Safety::Review,
        note: "test browser binaries without user profiles; the next install downloads them again",
    },
    Spec {
        name: "cargo-registry-cache",
        tool: "cargo",
        base: Base::Cargo,
        relative: "registry/cache",
        safety: Safety::Safe,
        note: "",
    },
    Spec {
        name: "cargo-registry-src",
        tool: "cargo",
        base: Base::Cargo,
        relative: "registry/src",
        safety: Safety::Safe,
        note: "",
    },
    Spec {
        name: "cargo-git-checkouts",
        tool: "cargo",
        base: Base::Cargo,
        relative: "git/checkouts",
        safety: Safety::Safe,
        note: "",
    },
    Spec {
        name: "cargo-git-db",
        tool: "cargo",
        base: Base::Cargo,
        relative: "git/db",
        safety: Safety::Safe,
        note: "",
    },
    Spec {
        name: "sccache",
        tool: "sccache",
        base: Base::Cache,
        relative: "sccache",
        safety: Safety::Safe,
        note: "",
    },
    Spec {
        name: "pip",
        tool: "python",
        base: Base::Cache,
        relative: "pip",
        safety: Safety::Safe,
        note: "",
    },
    Spec {
        name: "uv",
        tool: "python",
        base: Base::Cache,
        relative: "uv",
        safety: Safety::Safe,
        note: "",
    },
    Spec {
        name: "poetry",
        tool: "python",
        base: Base::Cache,
        relative: "pypoetry",
        safety: Safety::Review,
        note: "also holds the virtualenvs Poetry created for projects",
    },
    Spec {
        name: "pre-commit",
        tool: "pre-commit",
        base: Base::Cache,
        relative: "pre-commit",
        safety: Safety::Safe,
        note: "",
    },
    Spec {
        name: "go-build",
        tool: "go",
        base: Base::Cache,
        relative: "go-build",
        safety: Safety::Safe,
        note: "",
    },
    Spec {
        name: "go-modules",
        tool: "go",
        base: Base::GoModules,
        relative: "",
        safety: Safety::Review,
        note: "files are read-only, permanent deletion can fail; `go clean -modcache` removes it",
    },
    Spec {
        name: "gradle",
        tool: "gradle",
        base: Base::Gradle,
        relative: "caches",
        safety: Safety::Review,
        note: "",
    },
    Spec {
        name: "maven",
        tool: "maven",
        base: Base::Home,
        relative: ".m2/repository",
        safety: Safety::Review,
        note: "may hold artifacts installed locally with `mvn install`",
    },
    Spec {
        name: "composer",
        tool: "php",
        base: Base::Cache,
        relative: "composer",
        safety: Safety::Safe,
        note: "",
    },
    Spec {
        name: "jetbrains",
        tool: "jetbrains",
        base: Base::Cache,
        relative: "JetBrains",
        safety: Safety::Review,
        note: "IDE indexes and caches; settings and logins live in the config directory",
    },
    Spec {
        name: "huggingface",
        tool: "huggingface",
        base: Base::HuggingFace,
        relative: "",
        safety: Safety::Review,
        note: "downloaded models and datasets",
    },
    Spec {
        name: "ollama",
        tool: "ollama",
        base: Base::Ollama,
        relative: "",
        safety: Safety::Review,
        note: "downloaded Ollama models; `ollama rm <model>` removes individual models",
    },
    Spec {
        name: "bazel",
        tool: "bazel",
        base: Base::Cache,
        relative: "bazel",
        safety: Safety::Review,
        note: "",
    },
    Spec {
        name: "android-avd",
        tool: "android",
        base: Base::AndroidAvd,
        relative: "",
        safety: Safety::Review,
        note: "Android Virtual Device images; deleting removes emulator state and installed apps",
    },
    Spec {
        name: "android-build-cache",
        tool: "android",
        base: Base::Android,
        relative: "build-cache",
        safety: Safety::Safe,
        note: "Android Gradle build cache",
    },
    Spec {
        name: "xcode-derived-data",
        tool: "xcode",
        base: Base::Home,
        relative: "Library/Developer/Xcode/DerivedData",
        safety: Safety::Safe,
        note: "Xcode build intermediates and indexes; rebuilt on next build",
    },
    Spec {
        name: "ios-simulators",
        tool: "xcode",
        base: Base::Home,
        relative: "Library/Developer/CoreSimulator/Devices",
        safety: Safety::Review,
        note: "iOS Simulator devices and app data; `xcrun simctl delete unavailable` removes old runtimes",
    },
    Spec {
        name: "ios-simulator-caches",
        tool: "xcode",
        base: Base::Home,
        relative: "Library/Developer/CoreSimulator/Caches",
        safety: Safety::Safe,
        note: "iOS Simulator runtime and dyld caches",
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Locations {
    pub home: PathBuf,
    pub cache: PathBuf,
    pub data: PathBuf,
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
            data: home.join(".local/share"),
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
        Some(Self {
            cache,
            data: dirs::data_dir().unwrap_or(defaults.data),
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

    fn base(&self, base: Base) -> &Path {
        match base {
            Base::Home => &self.home,
            Base::Cache => &self.cache,
            Base::Data => &self.data,
            Base::Cargo => &self.cargo,
            Base::GoModules => &self.go_modules,
            Base::Gradle => &self.gradle,
            Base::Bun => &self.bun,
            Base::Android => &self.android,
            Base::AndroidAvd => &self.android_avd,
            Base::HuggingFace => &self.huggingface,
            Base::Ollama => &self.ollama,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownCache {
    pub name: &'static str,
    pub tool: &'static str,
    pub path: PathBuf,
    pub safety: Safety,
    pub note: &'static str,
}

/// @name names
/// @description Lists the names of every global tool cache kuizer knows, present or not.
///
/// @example
/// assert!(caches::names().any(|name| name == "npm"));
pub fn names() -> impl Iterator<Item = &'static str> {
    SPECS.iter().map(|spec| spec.name)
}

/// @name safety
/// @description Returns the safety level of a known global cache by name, or `None` if unknown.
/// If multiple specs share a name, `Review` takes precedence over `Safe`.
///
/// @example
/// assert_eq!(caches::safety("npm"), Some(Safety::Safe));
pub fn safety(name: &str) -> Option<Safety> {
    let mut found = None;
    for spec in SPECS.iter().filter(|spec| spec.name == name) {
        if spec.safety == Safety::Review {
            return Some(Safety::Review);
        }
        found = Some(spec.safety);
    }
    found
}

/// @name known
/// @description Rebuilds the description of a known global cache from its name, for a cache
/// that was found at `path` earlier. Returns `None` for a name kuizer does not know.
///
/// @example
/// assert_eq!(caches::known("npm", path).map(|cache| cache.tool), Some("npm"));
pub fn known(name: &str, path: PathBuf) -> Option<KnownCache> {
    SPECS
        .iter()
        .find(|spec| spec.name == name)
        .map(|spec| KnownCache {
            name: spec.name,
            tool: spec.tool,
            path,
            safety: spec.safety,
            note: spec.note,
        })
}

/// @name roots
/// @description Resolves every known global cache location against `locations` without checking
/// that it exists, for callers that want to stay out of tool-managed folders.
///
/// @example
/// let skip = caches::roots(&Locations::for_user()?);
pub fn roots(locations: &Locations) -> Vec<PathBuf> {
    SPECS
        .iter()
        .map(|spec| match spec.relative {
            "" => locations.base(spec.base).to_path_buf(),
            relative => locations.base(spec.base).join(relative),
        })
        .collect()
}

/// @name find
/// @description Resolves the known global tool caches against `locations` and returns the ones
/// that exist as real directories. Symlinks and anything inside protected browser or credential
/// data are left out.
///
/// @example
/// let found = caches::find(&Locations::for_user()?, &Protected::for_user());
pub fn find(locations: &Locations, protected: &Protected) -> Vec<KnownCache> {
    candidates(locations, protected).collect()
}

/// @name find_excluding
/// @description Like [`find`], and leaves out every cache whose path or any folder above it
/// matches `excludes`, so a cache the user excluded from scans is neither listed nor offered for
/// deletion.
///
/// @example
/// let found = caches::find_excluding(&Locations::for_user()?, &Protected::for_user(), &config.excludes()?);
pub fn find_excluding(
    locations: &Locations,
    protected: &Protected,
    excludes: &Excludes,
) -> Vec<KnownCache> {
    candidates(locations, protected)
        .filter(|cache| !cache.path.ancestors().any(|dir| excludes.path(dir)))
        .collect()
}

/// @name measure
/// @description Measures each known global cache and returns nonempty caches, largest first,
/// paired with candidates that frontends can display or clean.
///
/// @example
/// let entries = caches::measure(caches::find(&locations, &Protected::for_user()));
pub fn measure(found: Vec<KnownCache>) -> Vec<(KnownCache, Candidate)> {
    measure_with(found, &Cancel::new(), |_| {})
}

/// @name measure_with
/// @description Like [`measure`], and stops sizing when cancelled while reporting each measured
/// cache as it completes.
///
/// @example
/// let entries = caches::measure_with(found, &cancel, |candidate| rows.push(candidate.clone()));
pub fn measure_with(
    found: Vec<KnownCache>,
    cancel: &Cancel,
    on_measured: impl Fn(&Candidate) + Sync,
) -> Vec<(KnownCache, Candidate)> {
    measure_threaded(
        found,
        cancel,
        scan::size_threads(&Config::default()),
        on_measured,
    )
}

/// @name measure_threaded
/// @description Like [`measure_with`], sizing every cache on one shared pool of `threads`
/// threads, so a huge cache gets help from every thread once the small ones are done.
///
/// @example
/// let entries = caches::measure_threaded(found, &cancel, scan::size_threads(&config), |_| {});
pub fn measure_threaded(
    found: Vec<KnownCache>,
    cancel: &Cancel,
    threads: usize,
    on_measured: impl Fn(&Candidate) + Sync,
) -> Vec<(KnownCache, Candidate)> {
    let items = found
        .into_iter()
        .enumerate()
        .map(|(index, cache)| {
            let candidate = Candidate {
                id: index as u64 + 1,
                path: cache.path.clone(),
                kind: Kind::Dir,
                rule: Arc::new(RuleInfo {
                    name: cache.name.to_owned(),
                    category: Category::Cache,
                    safety: cache.safety,
                    ecosystem: cache.tool.to_owned(),
                }),
                project: None,
                usage: Usage::default(),
                newest: None,
                identity: None,
                project_last_active: None,
            };
            (cache, candidate)
        })
        .collect();
    let mut entries = scan::measure_candidates(items, cancel, threads, on_measured);
    entries.retain(|(_, candidate)| candidate.usage.entries > 0);
    entries
}

/// @name guard_roots
/// @description The guard roots for removing caches or toolchains: the unique parent folder of
/// each candidate, because they live outside the scanned folders.
///
/// @example
/// let guard = Guard::new(&caches::guard_roots(&selected));
pub fn guard_roots(candidates: &[Candidate]) -> Vec<PathBuf> {
    let mut parents: Vec<PathBuf> = candidates
        .iter()
        .filter_map(|candidate| candidate.path.parent().map(Path::to_path_buf))
        .collect();
    parents.sort();
    parents.dedup();
    parents
}

fn candidates(locations: &Locations, protected: &Protected) -> impl Iterator<Item = KnownCache> {
    SPECS
        .iter()
        .map(|spec| KnownCache {
            name: spec.name,
            tool: spec.tool,
            path: match spec.relative {
                "" => locations.base(spec.base).to_path_buf(),
                relative => locations.base(spec.base).join(relative),
            },
            safety: spec.safety,
            note: spec.note,
        })
        .filter(|cache| {
            std::fs::symlink_metadata(&cache.path).is_ok_and(|metadata| metadata.is_dir())
                && !protected.covers(&cache.path)
        })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn finds_existing_caches_only() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path();
        fs::create_dir_all(home.join(".npm/_cacache")).expect("npm");
        fs::create_dir_all(home.join(".cargo/registry/cache")).expect("cargo");
        fs::create_dir_all(home.join(".cache")).expect("cache");
        fs::write(home.join(".cache/pip"), "not a dir").expect("pip file");
        std::os::unix::fs::symlink(home.join(".npm"), home.join(".cache/yarn")).expect("link");

        let found = find(&Locations::under(home), &Protected::new(Some(home)));
        let names: Vec<&str> = found.iter().map(|cache| cache.name).collect();
        assert_eq!(names, ["npm", "cargo-registry-cache"]);

        let pattern = home.join(".npm").display().to_string();
        let excludes = Excludes::new(&[pattern], &[]).expect("excludes");
        let kept = find_excluding(
            &Locations::under(home),
            &Protected::new(Some(home)),
            &excludes,
        );
        let names: Vec<&str> = kept.iter().map(|cache| cache.name).collect();
        assert_eq!(names, ["cargo-registry-cache"]);
    }

    #[test]
    fn known_rebuilds_a_cache_by_name() {
        let cache = known("pnpm-store", PathBuf::from("/x/pnpm/store")).expect("pnpm-store");
        assert_eq!(cache.tool, "pnpm");
        assert_eq!(cache.safety, Safety::Review);
        assert_eq!(cache.path, PathBuf::from("/x/pnpm/store"));
        assert!(!cache.note.is_empty());
        assert!(known("not-a-cache", PathBuf::new()).is_none());
    }

    #[test]
    fn names_are_unique() {
        let mut all: Vec<&str> = names().collect();
        all.sort_unstable();
        let count = all.len();
        all.dedup();
        assert_eq!(all.len(), count);
    }

    #[test]
    fn finds_mobile_and_model_caches_with_expected_safety() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path();
        fs::create_dir_all(home.join(".cache/huggingface")).expect("hf");
        fs::create_dir_all(home.join(".ollama/models")).expect("ollama");
        fs::create_dir_all(home.join(".android/avd")).expect("avd");
        fs::create_dir_all(home.join(".android/build-cache")).expect("android-build-cache");
        fs::create_dir_all(home.join("Library/Developer/Xcode/DerivedData")).expect("derived");
        fs::create_dir_all(home.join("Library/Developer/CoreSimulator/Devices")).expect("sim");
        fs::create_dir_all(home.join("Library/Developer/CoreSimulator/Caches")).expect("sim-cache");

        let found = find(&Locations::under(home), &Protected::new(Some(home)));
        let by_name: std::collections::HashMap<&str, Safety> =
            found.iter().map(|c| (c.name, c.safety)).collect();

        assert_eq!(by_name.get("huggingface"), Some(&Safety::Review));
        assert_eq!(by_name.get("ollama"), Some(&Safety::Review));
        assert_eq!(by_name.get("android-avd"), Some(&Safety::Review));
        assert_eq!(by_name.get("android-build-cache"), Some(&Safety::Safe));
        assert_eq!(by_name.get("xcode-derived-data"), Some(&Safety::Safe));
        assert_eq!(by_name.get("ios-simulators"), Some(&Safety::Review));
        assert_eq!(by_name.get("ios-simulator-caches"), Some(&Safety::Safe));

        // Negative: missing caches are not returned
        let empty = tempfile::tempdir().expect("empty tempdir");
        assert!(
            find(
                &Locations::under(empty.path()),
                &Protected::new(Some(empty.path()))
            )
            .is_empty()
        );
    }
}
