use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;

use super::Excludes;
use crate::detect::{Category, Detector, Match, RuleInfo, RuleSpec, Safety};
use crate::units::Idle;

pub const PROJECT_CONFIG_FILE: &str = ".kuizer.toml";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    #[serde(default)]
    pub rules: Vec<RuleSpec>,
    #[serde(default)]
    pub keep: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub idle: Option<Idle>,
}

/// Compiled `.kuizer.toml` scope active for a project root and its descendants.
pub struct ProjectScope {
    pub root: PathBuf,
    detector: Option<Detector>,
    keep_globs: Excludes,
    keep_paths: Vec<PathBuf>,
    pub idle: Option<Idle>,
    parent: Option<Arc<ProjectScope>>,
}

fn normalize_relative(raw: &str) -> Option<PathBuf> {
    let path = Path::new(raw);
    if path.is_absolute() {
        return None;
    }
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => out.push(part),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

fn has_glob_meta(pattern: &str) -> bool {
    pattern.contains(['*', '?', '[', ']', '{', '}'])
}

impl ProjectConfig {
    /// @name ProjectConfig::load
    /// @description Loads and validates a `.kuizer.toml` file at `project_root`, returning
    /// `Ok(None)` if the file does not exist.
    ///
    /// @example
    /// if let Some(cfg) = ProjectConfig::load(project_root)? { ... }
    pub fn load(project_root: &Path) -> Result<Option<Self>, String> {
        let path = project_root.join(PROJECT_CONFIG_FILE);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
        };
        let parsed: Self = toml::from_str(&text)
            .map_err(|error| format!("invalid {}: {}", path.display(), error.message()))?;
        if !parsed.rules.is_empty() {
            Detector::new(&parsed.rules)
                .map_err(|error| format!("invalid {}: {error}", path.display()))?;
        }
        for raw in parsed.keep.iter().chain(&parsed.exclude) {
            if has_glob_meta(raw) {
                Excludes::new(std::slice::from_ref(raw), &[])
                    .map_err(|error| format!("invalid {}: {error}", path.display()))?;
            } else if normalize_relative(raw).is_none() {
                return Err(format!(
                    "invalid {}: keep/exclude path `{raw}` must be relative to the project root",
                    path.display()
                ));
            }
        }
        Ok(Some(parsed))
    }
}

impl ProjectScope {
    pub fn load(
        root: PathBuf,
        reported: &[Category],
        parent: Option<Arc<ProjectScope>>,
    ) -> Result<Option<Arc<Self>>, String> {
        let Some(cfg) = ProjectConfig::load(&root)? else {
            return Ok(None);
        };
        let detector = if cfg.rules.is_empty() {
            None
        } else {
            Some(
                Detector::new(&cfg.rules)
                    .map_err(|error| {
                        format!("{}: {error}", root.join(PROJECT_CONFIG_FILE).display())
                    })?
                    .reporting(reported.to_vec()),
            )
        };
        let mut glob_patterns = Vec::new();
        let mut keep_paths = Vec::new();
        for raw in cfg.keep.iter().chain(&cfg.exclude) {
            if has_glob_meta(raw) {
                glob_patterns.push(raw.clone());
            } else if let Some(rel) = normalize_relative(raw) {
                keep_paths.push(rel);
            }
        }
        let keep_globs = Excludes::new(&glob_patterns, &[])
            .map_err(|error| format!("{}: {error}", root.join(PROJECT_CONFIG_FILE).display()))?;
        Ok(Some(Arc::new(Self {
            root,
            detector,
            keep_globs,
            keep_paths,
            idle: cfg.idle,
            parent,
        })))
    }

    /// Returns whether `path` is marked never-delete by this scope or any enclosing project scope.
    pub fn keeps(&self, path: &Path) -> bool {
        if let Ok(rel) = path.strip_prefix(&self.root) {
            if self
                .keep_paths
                .iter()
                .any(|kept| rel == kept || rel.starts_with(kept))
            {
                return true;
            }
            if !rel.as_os_str().is_empty()
                && (self.keep_globs.full_path(rel)
                    || rel
                        .file_name()
                        .is_some_and(|name| self.keep_globs.name(name)))
            {
                return true;
            }
        }
        self.parent.as_ref().is_some_and(|p| p.keeps(path))
    }

    /// Returns whether `dir` contains a never-delete relative path strictly underneath it, so `dir`
    /// itself must not be emitted as a candidate.
    pub fn protects_descendant(&self, dir: &Path) -> bool {
        if let Ok(rel) = dir.strip_prefix(&self.root)
            && !rel.as_os_str().is_empty()
            && self
                .keep_paths
                .iter()
                .any(|kept| kept != rel && kept.starts_with(rel))
        {
            return true;
        }
        self.parent
            .as_ref()
            .is_some_and(|p| p.protects_descendant(dir))
    }

    pub fn classify_dir(
        &self,
        path: &Path,
        parent_names: &[&OsStr],
        own_names: &[&OsStr],
    ) -> Option<Match> {
        if let Some(detector) = &self.detector {
            let parent_listing = detector.listing(parent_names.iter().copied());
            let own_listing = detector.listing(own_names.iter().copied());
            if let Some(rule) = detector.classify_dir(path, &parent_listing, &own_listing)
                && detector.reports(&rule)
            {
                return Some(rule);
            }
        }
        self.parent
            .as_ref()
            .and_then(|p| p.classify_dir(path, parent_names, own_names))
    }

    pub fn classify_file(&self, name: &OsStr, parent_names: &[&OsStr]) -> Option<Match> {
        if let Some(detector) = &self.detector {
            let parent_listing = detector.listing(parent_names.iter().copied());
            if let Some(rule) = detector.classify_file(name, &parent_listing)
                && detector.reports(&rule)
            {
                return Some(rule);
            }
        }
        self.parent
            .as_ref()
            .and_then(|p| p.classify_file(name, parent_names))
    }

    pub fn classify_symlink(&self, path: &Path, parent_names: &[&OsStr]) -> Option<Match> {
        if let Some(detector) = &self.detector {
            let parent_listing = detector.listing(parent_names.iter().copied());
            if let Some(rule) = detector.classify_symlink(path, &parent_listing)
                && detector.reports(&rule)
            {
                return Some(rule);
            }
        }
        self.parent
            .as_ref()
            .and_then(|p| p.classify_symlink(path, parent_names))
    }

    /// Combines a global match with an optional project match. A project rule may only narrow
    /// safety (`Safe` -> `Review`), never widen a `Review` match to `Safe`.
    pub fn narrow_safety(global: Match, project: Option<Match>) -> Match {
        match project {
            Some(local) if global.safety == Safety::Safe && local.safety == Safety::Review => {
                Arc::new(RuleInfo {
                    safety: Safety::Review,
                    ..RuleInfo::clone(&global)
                })
            }
            _ => global,
        }
    }
}

/// @name project_idle_override
/// @description Returns the strictest project-level idle override found in `.kuizer.toml` at
/// `project_root` (if present and valid), or `None` when none is configured.
///
/// @example
/// let override_idle = project_idle_override(project_root);
pub fn project_idle_override(project_root: &Path) -> Option<Idle> {
    ProjectConfig::load(project_root).ok().flatten()?.idle
}

/// @name effective_idle
/// @description Applies a project-level idle override on top of `global`. A project override can
/// only narrow (require a longer idle period), never shorten the global `--idle` threshold.
///
/// @example
/// let effective = effective_idle(global_idle, project_idle_override(project));
pub fn effective_idle(global: Idle, project: Option<Idle>) -> Idle {
    match project {
        Some(local) => Idle(global.0.max(local.0)),
        None => global,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::config::Config;
    use crate::scan::{self, Cancel};

    #[test]
    fn per_project_kuizer_toml_adds_rules_keeps_paths_and_narrows_safety() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = fs::canonicalize(tmp.path()).expect("canonical root");
        let project = root.join("my-app");
        fs::create_dir_all(project.join("node_modules/pkg")).expect("node_modules");
        fs::create_dir_all(project.join("dist/public")).expect("dist/public");
        fs::create_dir_all(project.join(".custom-gen/out")).expect("custom-gen");
        fs::create_dir_all(project.join(".kept-dir/data")).expect("kept-dir");
        fs::write(project.join("package.json"), "{}\n").expect("package.json");
        fs::write(project.join("node_modules/pkg/a.js"), "x").expect("a.js");
        fs::write(project.join("dist/public/app.js"), "x").expect("app.js");
        fs::write(project.join(".custom-gen/out/a.bin"), "x").expect("a.bin");
        fs::write(project.join(".kept-dir/data/a.txt"), "x").expect("a.txt");
        fs::write(
            project.join(PROJECT_CONFIG_FILE),
            r#"
idle = "90d"
keep = ["dist/public", ".kept-dir"]

[[rules]]
name = "custom-gen"
pattern = ".custom-gen"
category = "build"
safety = "safe"

[[rules]]
name = "cautious-node-modules"
pattern = "node_modules"
category = "dependencies"
safety = "review"

[[rules]]
name = "try-upgrade-dist"
pattern = "dist"
category = "build"
safety = "safe"
"#,
        )
        .expect(".kuizer.toml");

        let config = Config::default();
        let handle =
            scan::start(&config, std::slice::from_ref(&root), Cancel::new()).expect("scan starts");
        let collected = scan::collect(&handle, |_| {});

        let by_name: std::collections::HashMap<String, Safety> = collected
            .candidates
            .iter()
            .map(|c| {
                (
                    c.path.file_name().unwrap().to_string_lossy().into_owned(),
                    c.rule.safety,
                )
            })
            .collect();

        // Extra rule `.custom-gen` was detected.
        assert_eq!(by_name.get(".custom-gen"), Some(&Safety::Safe));
        // Built-in `node_modules` was narrowed from Safe -> Review by project rule.
        assert_eq!(by_name.get("node_modules"), Some(&Safety::Review));
        // `dist` was NOT emitted because `dist/public` is in `keep`.
        assert!(!by_name.contains_key("dist"));
        // `.kept-dir` was NOT emitted.
        assert!(!by_name.contains_key(".kept-dir"));
    }

    #[test]
    fn project_rule_cannot_upgrade_builtin_review_to_safe() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = fs::canonicalize(tmp.path()).expect("canonical root");
        let project = root.join("app");
        fs::create_dir_all(project.join("dist")).expect("dist");
        fs::write(project.join("package.json"), "{}\n").expect("package.json");
        fs::write(project.join("dist/bundle.js"), "x").expect("bundle.js");
        fs::write(
            project.join(PROJECT_CONFIG_FILE),
            r#"
[[rules]]
name = "force-safe-dist"
pattern = "dist"
markers = ["package.json"]
category = "build"
safety = "safe"
"#,
        )
        .expect(".kuizer.toml");

        let config = Config::default();
        let handle =
            scan::start(&config, std::slice::from_ref(&root), Cancel::new()).expect("scan starts");
        let collected = scan::collect(&handle, |_| {});
        assert_eq!(collected.candidates.len(), 1);
        // Built-in `js-dist` is `Review`; `.kuizer.toml` cannot make it `Safe`.
        assert_eq!(collected.candidates[0].rule.safety, Safety::Review);
    }

    #[test]
    fn effective_idle_only_narrows_global_threshold() {
        let global = Idle(Duration::from_secs(30 * 86_400));
        let longer = Idle(Duration::from_secs(90 * 86_400));
        let shorter = Idle(Duration::from_secs(7 * 86_400));

        assert_eq!(effective_idle(global, Some(longer)), longer);
        assert_eq!(effective_idle(global, Some(shorter)), global);
        assert_eq!(effective_idle(global, None), global);
    }

    #[test]
    fn invalid_project_config_reports_issue_and_does_not_abort_scan() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = fs::canonicalize(tmp.path()).expect("canonical root");
        let project = root.join("broken");
        fs::create_dir_all(project.join("node_modules/pkg")).expect("node_modules");
        fs::write(project.join("package.json"), "{}\n").expect("package.json");
        fs::write(project.join("node_modules/pkg/a.js"), "x").expect("a.js");
        fs::write(project.join(PROJECT_CONFIG_FILE), "not valid toml = [").expect("broken");

        let config = Config::default();
        let handle =
            scan::start(&config, std::slice::from_ref(&root), Cancel::new()).expect("scan starts");
        let collected = scan::collect(&handle, |_| {});
        assert_eq!(collected.candidates.len(), 1);
        assert!(
            collected
                .issues
                .iter()
                .any(|issue| issue.path.ends_with(PROJECT_CONFIG_FILE))
        );
    }
}
