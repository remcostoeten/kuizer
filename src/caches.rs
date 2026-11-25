use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::catalog::{self, CatalogConfig, Group, Platform};
use crate::config::{Config, Excludes};
use crate::detect::{Category, Kind, RuleInfo, Safety};
use crate::protect::Protected;
use crate::scan::{self, Cancel, Candidate, Usage};

pub use crate::catalog::Locations;

const UNKNOWN_CACHE_NOTE: &str = "No catalog entry. Programs keep data they can rebuild in this folder, but check which program owns it first.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownCache {
    pub name: String,
    pub tool: String,
    pub group: Group,
    pub category: Category,
    pub kind: Kind,
    pub path: PathBuf,
    pub safety: Safety,
    pub note: String,
    pub processes: Vec<String>,
}

/// @name names
/// @description Lists every active catalog entry name plus every found cache name, so `--select`
/// accepts both known catalog names and discovered `other:<dir>` caches.
///
/// @example
/// let known = caches::names(&found, &config.catalog);
pub fn names(found: &[KnownCache], catalog_config: &CatalogConfig) -> Vec<String> {
    let mut all: Vec<String> = catalog::entries(catalog_config)
        .into_iter()
        .map(|entry| entry.name)
        .chain(found.iter().map(|cache| cache.name.clone()))
        .collect();
    all.sort_unstable();
    all.dedup();
    all
}

/// @name safety
/// @description Returns the safety level of a catalog entry by name, or `None` if unknown.
/// If multiple entries share a name, `Review` takes precedence over `Safe`.
///
/// @example
/// assert_eq!(caches::safety("npm", &CatalogConfig::default()), Some(Safety::Safe));
pub fn safety(name: &str, catalog_config: &CatalogConfig) -> Option<Safety> {
    let mut found = None;
    for entry in catalog::entries(catalog_config)
        .into_iter()
        .filter(|entry| entry.name == name)
    {
        if entry.safety == Safety::Review {
            return Some(Safety::Review);
        }
        found = Some(entry.safety);
    }
    found
}

/// @name roots
/// @description Resolves every active catalog location without wildcards against `locations`
/// without checking that it exists, for callers that want to stay out of tool-managed folders.
///
/// @example
/// let skip = caches::roots(&Locations::for_user()?);
pub fn roots(locations: &Locations) -> Vec<PathBuf> {
    let platform = catalog::current_platform();
    catalog::embedded()
        .iter()
        .filter(|entry| entry.platforms.is_empty() || entry.platforms.contains(&platform))
        .filter(|entry| !entry.path.split('/').any(|part| part == "*"))
        .filter_map(|entry| {
            let base = locations.base(entry.base)?;
            Some(if entry.path.is_empty() {
                base.to_path_buf()
            } else {
                base.join(&entry.path)
            })
        })
        .collect()
}

/// @name find
/// @description Resolves the active location catalog against `locations` and returns every
/// existing entry that is not refused by `protected`, plus unclaimed direct child directories of
/// `locations.cache` marked `review` when `catalog.unknown_caches` is true.
///
/// @example
/// let found = caches::find(&Locations::for_user()?, &Protected::for_user(), &config.catalog);
pub fn find(
    locations: &Locations,
    protected: &Protected,
    catalog_config: &CatalogConfig,
) -> Vec<KnownCache> {
    find_for_platform(
        locations,
        protected,
        catalog_config,
        catalog::current_platform(),
    )
}

fn find_for_platform(
    locations: &Locations,
    protected: &Protected,
    catalog_config: &CatalogConfig,
    platform: Platform,
) -> Vec<KnownCache> {
    let active = catalog::entries(catalog_config);
    let resolved = catalog::resolve(&active, locations, platform, protected);
    let mut found: Vec<KnownCache> = resolved
        .into_iter()
        .map(|item| KnownCache {
            name: item.entry.name,
            tool: item.entry.app,
            group: item.entry.group,
            category: item.entry.category,
            kind: item.entry.kind,
            path: item.path,
            safety: item.entry.safety,
            note: item.entry.note,
            processes: item.entry.processes,
        })
        .collect();
    if catalog_config.unknown_caches {
        append_unknown_caches(&mut found, locations, protected);
    }
    found
}

fn append_unknown_caches(
    found: &mut Vec<KnownCache>,
    locations: &Locations,
    protected: &Protected,
) {
    let Ok(read) = fs::read_dir(&locations.cache) else {
        return;
    };
    let claimed: Vec<PathBuf> = found.iter().map(|cache| cache.path.clone()).collect();
    let mut seen: HashSet<PathBuf> = claimed.iter().cloned().collect();
    let mut children = Vec::new();
    for entry in read.flatten() {
        let path = entry.path();
        if fs::symlink_metadata(&path)
            .is_ok_and(|metadata| !metadata.file_type().is_symlink() && metadata.is_dir())
        {
            children.push(path);
        }
    }
    children.sort();
    for path in children {
        if protected.refuses(&path) {
            continue;
        }
        let overlaps = claimed
            .iter()
            .any(|other| other == &path || other.starts_with(&path) || path.starts_with(other));
        if overlaps || !seen.insert(path.clone()) {
            continue;
        }
        let Some(child) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        found.push(KnownCache {
            name: format!("other:{child}"),
            tool: child.to_owned(),
            group: Group::Apps,
            category: Category::Cache,
            kind: Kind::Dir,
            path,
            safety: Safety::Review,
            note: UNKNOWN_CACHE_NOTE.to_owned(),
            processes: Vec::new(),
        });
    }
}

/// @name find_excluding
/// @description Like [`find`], and leaves out every cache whose path or any folder above it
/// matches `excludes`, so a cache the user excluded from scans is neither listed nor offered for
/// deletion.
///
/// @example
/// let found = caches::find_excluding(
///     &Locations::for_user()?,
///     &Protected::for_user(),
///     &config.catalog,
///     &config.excludes()?,
/// );
pub fn find_excluding(
    locations: &Locations,
    protected: &Protected,
    catalog_config: &CatalogConfig,
    excludes: &Excludes,
) -> Vec<KnownCache> {
    find(locations, protected, catalog_config)
        .into_iter()
        .filter(|cache| !cache.path.ancestors().any(|dir| excludes.path(dir)))
        .collect()
}

/// @name measure
/// @description Measures each known global cache and returns nonempty caches, largest first,
/// paired with candidates that frontends can display or clean.
///
/// @example
/// let entries = caches::measure(caches::find(&locations, &Protected::for_user(), &config.catalog));
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
                kind: cache.kind,
                rule: Arc::new(RuleInfo {
                    name: cache.name.clone(),
                    category: cache.category,
                    safety: cache.safety,
                    ecosystem: cache.tool.clone(),
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

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::catalog::{Base, Entry};

    fn known_only() -> CatalogConfig {
        CatalogConfig {
            unknown_caches: false,
            ..CatalogConfig::default()
        }
    }

    #[test]
    fn finds_existing_caches_only() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path();
        fs::create_dir_all(home.join(".npm/_cacache")).expect("npm");
        fs::create_dir_all(home.join(".cargo/registry/cache")).expect("cargo");
        fs::create_dir_all(home.join(".cache")).expect("cache");
        fs::write(home.join(".cache/pip"), "not a dir").expect("pip file");
        std::os::unix::fs::symlink(home.join(".npm"), home.join(".cache/yarn")).expect("link");

        let found = find(
            &Locations::under(home),
            &Protected::new(Some(home)),
            &known_only(),
        );
        let found_names: Vec<&str> = found.iter().map(|cache| cache.name.as_str()).collect();
        assert_eq!(found_names, ["npm", "cargo-registry-cache"]);

        let pattern = home.join(".npm").display().to_string();
        let excludes = Excludes::new(&[pattern], &[]).expect("excludes");
        let kept = find_excluding(
            &Locations::under(home),
            &Protected::new(Some(home)),
            &known_only(),
            &excludes,
        );
        let kept_names: Vec<&str> = kept.iter().map(|cache| cache.name.as_str()).collect();
        assert_eq!(kept_names, ["cargo-registry-cache"]);
    }

    #[test]
    fn lists_unknown_cache_folders_for_review() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path();
        fs::create_dir_all(home.join(".cache/pip")).expect("pip");
        fs::create_dir_all(home.join(".cache/someapp")).expect("someapp");
        fs::create_dir_all(home.join(".cache/google-chrome")).expect("chrome");

        let found = find(
            &Locations::under(home),
            &Protected::new(Some(home)),
            &CatalogConfig::default(),
        );
        let names: Vec<(&str, Safety)> = found
            .iter()
            .map(|cache| (cache.name.as_str(), cache.safety))
            .collect();
        assert_eq!(
            names,
            [("pip", Safety::Safe), ("other:someapp", Safety::Review)]
        );
    }

    #[test]
    fn user_locations_and_disabled_entries_apply() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path();
        fs::create_dir_all(home.join(".cache/pip")).expect("pip");
        fs::create_dir_all(home.join(".cache/renderer")).expect("renderer");

        let catalog_config = CatalogConfig {
            disabled: vec!["pip".to_owned()],
            locations: vec![Entry {
                name: "my-renderer-cache".to_owned(),
                app: "Renderer".to_owned(),
                group: Group::Apps,
                category: Category::Cache,
                kind: Kind::Dir,
                base: Base::Cache,
                path: "renderer".to_owned(),
                safety: Safety::Safe,
                platforms: Vec::new(),
                processes: Vec::new(),
                note: String::new(),
            }],
            unknown_caches: false,
        };
        let found = find(
            &Locations::under(home),
            &Protected::new(Some(home)),
            &catalog_config,
        );
        let names: Vec<&str> = found.iter().map(|cache| cache.name.as_str()).collect();
        assert_eq!(names, ["my-renderer-cache"]);
    }

    #[test]
    fn names_are_unique() {
        let all = names(&[], &CatalogConfig::default());
        let mut deduped = all.clone();
        deduped.sort_unstable();
        deduped.dedup();
        assert_eq!(all.len(), deduped.len());
        assert!(all.iter().any(|name| name == "npm"));
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

        let found = find(
            &Locations::under(home),
            &Protected::new(Some(home)),
            &known_only(),
        );
        let by_name: std::collections::HashMap<&str, Safety> =
            found.iter().map(|c| (c.name.as_str(), c.safety)).collect();

        assert_eq!(by_name.get("huggingface"), Some(&Safety::Review));
        assert_eq!(by_name.get("ollama"), Some(&Safety::Review));
        assert_eq!(by_name.get("android-avd"), Some(&Safety::Review));
        assert_eq!(by_name.get("android-build-cache"), Some(&Safety::Safe));
        assert_eq!(by_name.get("xcode-derived-data"), Some(&Safety::Safe));
        assert_eq!(by_name.get("ios-simulators"), Some(&Safety::Review));
        assert_eq!(by_name.get("ios-simulator-caches"), Some(&Safety::Safe));

        let empty = tempfile::tempdir().expect("empty tempdir");
        assert!(
            find(
                &Locations::under(empty.path()),
                &Protected::new(Some(empty.path())),
                &known_only(),
            )
            .is_empty()
        );
    }
}
