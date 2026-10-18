use std::path::{Path, PathBuf};

use crate::detect::Safety;
use crate::protect::Protected;

#[derive(Debug, Clone, Copy)]
enum Base {
    Home,
    Cache,
    Data,
    Cargo,
    GoModules,
    Gradle,
    Bun,
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
        base: Base::Cache,
        relative: "huggingface",
        safety: Safety::Review,
        note: "downloaded models and datasets",
    },
    Spec {
        name: "bazel",
        tool: "bazel",
        base: Base::Cache,
        relative: "bazel",
        safety: Safety::Review,
        note: "",
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
}

fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

impl Locations {
    pub fn under(home: &Path) -> Self {
        Self {
            home: home.to_path_buf(),
            cache: home.join(".cache"),
            data: home.join(".local/share"),
            cargo: home.join(".cargo"),
            go_modules: home.join("go/pkg/mod"),
            gradle: home.join(".gradle"),
            bun: home.join(".bun"),
        }
    }

    pub fn for_user() -> Option<Self> {
        let home = dirs::home_dir()?;
        let defaults = Self::under(&home);
        Some(Self {
            cache: dirs::cache_dir().unwrap_or(defaults.cache),
            data: dirs::data_dir().unwrap_or(defaults.data),
            cargo: env_dir("CARGO_HOME").unwrap_or(defaults.cargo),
            go_modules: env_dir("GOMODCACHE")
                .or_else(|| env_dir("GOPATH").map(|path| path.join("pkg/mod")))
                .unwrap_or(defaults.go_modules),
            gradle: env_dir("GRADLE_USER_HOME").unwrap_or(defaults.gradle),
            bun: env_dir("BUN_INSTALL").unwrap_or(defaults.bun),
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

/// @name find
/// @description Resolves the known global tool caches against `locations` and returns the ones
/// that exist as real directories. Symlinks and anything inside protected browser or credential
/// data are left out.
///
/// @example
/// let found = caches::find(&Locations::for_user()?, &Protected::for_user());
pub fn find(locations: &Locations, protected: &Protected) -> Vec<KnownCache> {
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
        .collect()
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
        #[cfg(unix)]
        std::os::unix::fs::symlink(home.join(".npm"), home.join(".cache/yarn")).expect("link");

        let found = find(&Locations::under(home), &Protected::new(Some(home)));
        let names: Vec<&str> = found.iter().map(|cache| cache.name).collect();
        assert_eq!(names, ["npm", "cargo-registry-cache"]);
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
}
