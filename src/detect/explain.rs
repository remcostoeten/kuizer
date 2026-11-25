use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Component, Path, PathBuf};

use globset::Glob;
use serde::Serialize;

use super::{CACHEDIR_SIGNATURE, CACHEDIR_TAG, Category, Kind, RuleSpec, Safety, builtin_rules};
use crate::config::{Config, ConfigError, LEGACY_IGNORE_FILE, PROJECT_CONFIG_FILE, ProjectScope};
use crate::delete::{Guard, Target};
use crate::platform::{self, MountTable};
use crate::protect::Protected;

#[derive(Debug, Clone, Serialize)]
pub struct NearMiss {
    pub rule: String,
    pub missing_markers: Vec<String>,
    pub missing_contents: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MatchedRule {
    pub name: String,
    pub kind: Kind,
    pub category: Category,
    pub safety: Safety,
    pub ecosystem: String,
    pub markers_found: Vec<String>,
    pub contents_found: Vec<String>,
    pub source: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Explanation {
    pub path: PathBuf,
    pub exists: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<Kind>,
    pub is_candidate: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched: Option<MatchedRule>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub near_misses: Vec<NearMiss>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blockers: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delete_guard: Option<String>,
}

fn normalize_path(raw: &Path) -> PathBuf {
    let abs = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(raw))
            .unwrap_or_else(|_| raw.to_path_buf())
    };
    // Canonicalize the parent directory if possible without dereferencing the target itself,
    // so symlinks such as Nix `result` remain symlinks.
    if let (Some(parent), Some(name)) = (abs.parent(), abs.file_name())
        && let Ok(canon_parent) = fs::canonicalize(parent)
    {
        return canon_parent.join(name);
    }
    let mut out = PathBuf::new();
    for component in abs.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn read_names(dir: &Path) -> Vec<OsString> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<OsString> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.file_name())
        .collect();
    names.sort();
    names
}

fn pattern_matches(pattern: &str, name: &OsStr) -> bool {
    Glob::new(pattern)
        .ok()
        .map(|g| g.compile_matcher().is_match(name))
        .unwrap_or_else(|| name == OsStr::new(pattern))
}

fn matching_evidence(patterns: &[String], names: &[OsString]) -> Vec<String> {
    let mut found = Vec::new();
    for pat in patterns {
        if let Some(hit) = names
            .iter()
            .find(|name| pattern_matches(pat, name.as_os_str()))
        {
            found.push(hit.to_string_lossy().into_owned());
        }
    }
    found
}

fn valid_cachedir_tag(dir: &Path) -> bool {
    use std::io::Read;
    let mut header = [0u8; CACHEDIR_SIGNATURE.len()];
    fs::File::open(dir.join(CACHEDIR_TAG))
        .and_then(|mut file| file.read_exact(&mut header))
        .is_ok_and(|()| header == CACHEDIR_SIGNATURE)
}

fn load_ancestor_projects(
    path: &Path,
    categories: &[Category],
) -> Option<std::sync::Arc<ProjectScope>> {
    let mut ancestors: Vec<&Path> = path.ancestors().skip(1).collect();
    ancestors.reverse();
    let mut current = None;
    for dir in ancestors {
        if dir.join(PROJECT_CONFIG_FILE).is_file()
            && let Ok(Some(scope)) =
                ProjectScope::load(dir.to_path_buf(), categories, current.clone())
        {
            current = Some(scope);
        }
    }
    current
}

/// @name explain_path
/// @description Inspects a single path under `config` and explains why it is or is not a cleanup
/// candidate: which rule matched (or nearly matched), which markers and contents were found, its
/// safety level, and which scan filter or deletion guard would skip or refuse it.
///
/// @example
/// let info = kuizer::detect::explain_path(&config, Path::new("./node_modules"))?;
pub fn explain_path(config: &Config, raw_path: &Path) -> Result<Explanation, ConfigError> {
    let path = normalize_path(raw_path);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Explanation {
                path,
                exists: false,
                kind: None,
                is_candidate: false,
                matched: None,
                near_misses: Vec::new(),
                blockers: vec!["path does not exist".to_owned()],
                delete_guard: Some("path no longer exists".to_owned()),
            });
        }
        Err(error) => {
            return Ok(Explanation {
                path,
                exists: false,
                kind: None,
                is_candidate: false,
                matched: None,
                near_misses: Vec::new(),
                blockers: vec![format!("cannot inspect path: {error}")],
                delete_guard: Some(format!("cannot inspect path: {error}")),
            });
        }
    };

    let kind = if metadata.file_type().is_symlink() {
        Kind::Symlink
    } else if metadata.is_dir() {
        Kind::Dir
    } else {
        Kind::File
    };

    let mut blockers = Vec::new();
    let protected = Protected::for_user().with_paths(&config.delete.protected);
    let mounts = MountTable::load();
    let excludes = config.usable_excludes();

    if Protected::protects_system_path(&path) {
        blockers.push("inside a protected system directory".to_owned());
    } else if protected.covers(&path) {
        blockers.push(
            "protected (browser data, credentials, installed tools, or delete.protected)"
                .to_owned(),
        );
    }

    if config
        .scan
        .excluded_mounts
        .iter()
        .any(|mount| path.starts_with(mount))
        || mounts.fs_type(&path).is_some_and(|fs_type| {
            config
                .scan
                .excluded_fs_types
                .iter()
                .any(|excluded| excluded == fs_type)
        })
    {
        blockers.push("on an excluded mount or filesystem type".to_owned());
    }

    for ancestor in path.ancestors() {
        if let Some(name) = ancestor.file_name() {
            if let Some(skip) = config
                .scan
                .skip_dirs
                .iter()
                .find(|skip| name == OsStr::new(skip.as_str()))
                && (ancestor != path || kind == Kind::Dir)
            {
                blockers.push(format!("skipped by scan.skip_dirs (`{skip}`)"));
                break;
            }
            if excludes.name(name) {
                blockers.push(format!(
                    "excluded by scan.exclude pattern matching `{}`",
                    name.to_string_lossy()
                ));
                break;
            }
        }
        if excludes.full_path(ancestor) {
            blockers.push(format!(
                "excluded by scan.exclude path pattern matching `{}`",
                ancestor.display()
            ));
            break;
        }
    }

    if kind == Kind::File
        && path
            .file_name()
            .is_some_and(|name| excludes.extension(name))
    {
        blockers.push("excluded by scan.exclude_extensions".to_owned());
    }

    // Check ignore files in target (if dir) or ancestors.
    for dir in path.ancestors() {
        if dir == path && kind != Kind::Dir {
            continue;
        }
        if !dir.is_dir() {
            continue;
        }
        for ignore in [config.scan.ignore_file.as_str(), LEGACY_IGNORE_FILE] {
            if !ignore.is_empty() && dir.join(ignore).exists() {
                blockers.push(format!("ignored by `{ignore}` in {}", dir.display()));
                break;
            }
        }
    }

    let project = load_ancestor_projects(&path, &config.detect.categories);
    if let Some(scope) = &project {
        if scope.keeps(&path) {
            blockers.push(format!(
                "kept (never-delete) by `{PROJECT_CONFIG_FILE}` in {}",
                scope.root.display()
            ));
        } else if kind == Kind::Dir && scope.protects_descendant(&path) {
            blockers.push(format!(
                "contains a never-delete path configured in `{PROJECT_CONFIG_FILE}` in {}",
                scope.root.display()
            ));
        }
    }

    let parent_dir = path.parent();
    let parent_names = parent_dir.map_or_else(Vec::new, read_names);
    let own_names = if kind == Kind::Dir {
        read_names(&path)
    } else {
        Vec::new()
    };

    let detector = config.detector()?;
    let parent_listing = detector.listing(parent_names.iter());
    let own_listing = detector.listing(own_names.iter());

    let global_match = match kind {
        Kind::Dir => detector.classify_dir(&path, &parent_listing, &own_listing),
        Kind::File => path
            .file_name()
            .and_then(|name| detector.classify_file(name, &parent_listing)),
        Kind::Symlink => detector.classify_symlink(&path, &parent_listing),
        Kind::External => None,
    };

    let parent_refs: Vec<&OsStr> = parent_names.iter().map(OsString::as_os_str).collect();
    let own_refs: Vec<&OsStr> = own_names.iter().map(OsString::as_os_str).collect();
    let local_match = project.as_ref().and_then(|scope| match kind {
        Kind::Dir => scope.classify_dir(&path, &parent_refs, &own_refs),
        Kind::File => path
            .file_name()
            .and_then(|name| scope.classify_file(name, &parent_refs)),
        Kind::Symlink => scope.classify_symlink(&path, &parent_refs),
        Kind::External => None,
    });

    let all_specs: Vec<RuleSpec> = config
        .rules
        .iter()
        .cloned()
        .chain(if config.detect.builtin_rules {
            builtin_rules()
        } else {
            Vec::new()
        })
        .filter(|rule| !config.detect.disabled_rules.contains(&rule.name))
        .collect();
    let builtin_names: std::collections::HashSet<String> =
        builtin_rules().into_iter().map(|r| r.name).collect();

    let mut matched = None;
    let mut near_misses = Vec::new();

    if let Some(global_rule) = global_match {
        let mut notes = Vec::new();
        if !detector.reports(&global_rule) {
            blockers.push(format!(
                "category `{}` is disabled in detect.categories",
                global_rule.category.as_str()
            ));
        }
        let mut final_rule = ProjectScope::narrow_safety(global_rule.clone(), local_match);
        if final_rule.safety != global_rule.safety {
            notes.push(format!(
                "safety narrowed from safe to review by `{PROJECT_CONFIG_FILE}`"
            ));
        }
        if final_rule.safety == Safety::Safe && protected.in_hidden_home_dir(&path) {
            final_rule = std::sync::Arc::new(super::RuleInfo {
                safety: Safety::Review,
                ..super::RuleInfo::clone(&final_rule)
            });
            notes.push("safety narrowed from safe to review because path is inside a hidden directory in home".to_owned());
        }
        let spec = all_specs.iter().find(|s| s.name == final_rule.name);
        let markers_found = spec
            .map(|s| matching_evidence(&s.markers, &parent_names))
            .unwrap_or_default();
        let contents_found = spec
            .map(|s| matching_evidence(&s.contains, &own_names))
            .unwrap_or_default();
        let source = if builtin_names.contains(&final_rule.name) {
            "built-in".to_owned()
        } else {
            "config / rules.d".to_owned()
        };
        matched = Some(MatchedRule {
            name: final_rule.name.clone(),
            kind,
            category: final_rule.category,
            safety: final_rule.safety,
            ecosystem: final_rule.ecosystem.clone(),
            markers_found,
            contents_found,
            source,
            notes,
        });
    } else if let Some(local_rule) = local_match {
        let mut notes = Vec::new();
        let mut safety = local_rule.safety;
        if safety == Safety::Safe && protected.in_hidden_home_dir(&path) {
            safety = Safety::Review;
            notes.push("safety narrowed from safe to review because path is inside a hidden directory in home".to_owned());
        }
        matched = Some(MatchedRule {
            name: local_rule.name.clone(),
            kind,
            category: local_rule.category,
            safety,
            ecosystem: local_rule.ecosystem.clone(),
            markers_found: Vec::new(),
            contents_found: Vec::new(),
            source: PROJECT_CONFIG_FILE.to_owned(),
            notes,
        });
    } else if let Some(file_name) = path.file_name() {
        for spec in &all_specs {
            if spec.kind != kind || spec.pattern == "*" {
                continue;
            }
            if !pattern_matches(&spec.pattern, file_name) {
                continue;
            }
            let found_markers = matching_evidence(&spec.markers, &parent_names);
            let missing_markers = if spec.markers.is_empty() || !found_markers.is_empty() {
                Vec::new()
            } else {
                spec.markers.clone()
            };
            let found_contents = matching_evidence(&spec.contains, &own_names);
            let missing_contents = if spec.contains.is_empty() || !found_contents.is_empty() {
                Vec::new()
            } else {
                spec.contains.clone()
            };
            let mut note = None;
            if spec.name == "nix-result"
                && fs::read_link(&path).is_ok_and(|target| !target.starts_with("/nix/store"))
            {
                note = Some("symlink does not point into /nix/store".to_owned());
            } else if spec.name == "python-venv-stale" && missing_contents.is_empty() {
                note = Some("virtualenv interpreter still exists (not stale)".to_owned());
            } else if spec.contains.iter().any(|c| c == CACHEDIR_TAG)
                && own_names.iter().any(|n| n == CACHEDIR_TAG)
                && !valid_cachedir_tag(&path)
            {
                note = Some("CACHEDIR.TAG does not start with the standard signature".to_owned());
            }
            near_misses.push(NearMiss {
                rule: spec.name.clone(),
                missing_markers,
                missing_contents,
                note,
            });
        }
    }

    // Evaluate the deletion guard using the parent directory as the simulated scan root.
    let guard_roots: Vec<PathBuf> = parent_dir
        .map(|parent| vec![parent.to_path_buf()])
        .unwrap_or_default();
    let guard = Guard::new(&guard_roots)
        .with_protected(&config.delete.protected)
        .with_cross_device(config.delete.cross_device);
    let target = if kind == Kind::Symlink {
        Target::from(&crate::scan::Candidate {
            id: 0,
            path: path.clone(),
            kind: Kind::Symlink,
            rule: std::sync::Arc::new(super::RuleInfo {
                name: String::new(),
                category: Category::Temp,
                safety: Safety::Review,
                ecosystem: String::new(),
            }),
            project: None,
            usage: Default::default(),
            newest: None,
            identity: platform::identity(&metadata),
            project_last_active: None,
        })
    } else {
        Target::new(path.clone(), platform::identity(&metadata))
    };
    let delete_guard = guard.check_target(&target).err().map(|r| r.to_string());

    let is_candidate = matched.is_some() && blockers.is_empty();

    Ok(Explanation {
        path,
        exists: true,
        kind: Some(kind),
        is_candidate,
        matched,
        near_misses,
        blockers,
        delete_guard,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explains_matching_candidate_with_markers_and_safety() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let project = tmp.path().join("web");
        let nm = project.join("node_modules");
        fs::create_dir_all(&nm).expect("node_modules");
        fs::write(project.join("package.json"), "{}\n").expect("package.json");

        let info = explain_path(&Config::default(), &nm).expect("explain");
        assert!(info.exists);
        assert!(info.is_candidate);
        let matched = info.matched.expect("matched");
        assert_eq!(matched.name, "node-modules");
        assert_eq!(matched.safety, Safety::Safe);
        assert_eq!(matched.markers_found, vec!["package.json"]);
        assert!(info.blockers.is_empty());
        assert!(info.delete_guard.is_none());
    }

    #[test]
    fn explains_near_miss_when_marker_is_missing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let nm = tmp.path().join("orphan/node_modules");
        fs::create_dir_all(&nm).expect("node_modules");

        let info = explain_path(&Config::default(), &nm).expect("explain");
        assert!(info.exists);
        assert!(!info.is_candidate);
        assert!(info.matched.is_none());
        assert!(info.near_misses.iter().any(|miss| {
            miss.rule == "node-modules" && miss.missing_markers == vec!["package.json"]
        }));
    }

    #[test]
    fn explains_protected_system_path_and_guard_refusal() {
        let info = explain_path(&Config::default(), Path::new("/etc")).expect("explain");
        assert!(!info.is_candidate);
        assert!(!info.blockers.is_empty());
        assert!(info.delete_guard.is_some());
    }

    #[test]
    fn explains_kuizer_toml_keep_blocker() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let project = tmp.path().join("app");
        let nm = project.join("node_modules");
        fs::create_dir_all(&nm).expect("node_modules");
        fs::write(project.join("package.json"), "{}\n").expect("package.json");
        fs::write(project.join(".kuizer.toml"), "keep = [\"node_modules\"]\n").expect("toml");

        let info = explain_path(&Config::default(), &nm).expect("explain");
        assert!(!info.is_candidate);
        assert!(info.blockers.iter().any(|b| b.contains(".kuizer.toml")));
    }
}
