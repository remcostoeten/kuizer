use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::ConfigWarning;
use crate::detect::{Detector, RuleSpec};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RulePack {
    #[serde(default)]
    rules: Vec<RuleSpec>,
}

/// @name default_rules_dir
/// @description Returns the default directory for drop-in rule packs (`~/.config/kuizer/rules.d`).
///
/// @example
/// let dir = kuizer::config::default_rules_dir();
pub fn default_rules_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("kuizer").join("rules.d"))
}

pub(crate) fn load_rules_dir(
    dir: &Path,
    known: &mut HashSet<String>,
    warnings: &mut Vec<ConfigWarning>,
) -> Vec<RuleSpec> {
    if !dir.is_dir() {
        return Vec::new();
    }
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            warnings.push(ConfigWarning::note(
                "rules.d",
                format!("cannot read {}: {error}", dir.display()),
            ));
            return Vec::new();
        }
    };

    let mut files: Vec<PathBuf> = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("toml") && path.is_file() {
                    files.push(path);
                }
            }
            Err(error) => {
                warnings.push(ConfigWarning::note(
                    "rules.d",
                    format!("cannot read entry in {}: {error}", dir.display()),
                ));
            }
        }
    }
    files.sort();

    let mut loaded = Vec::new();
    'files: for path in files {
        let filename = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let key = format!("rules.d/{filename}");
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) => {
                warnings.push(ConfigWarning::note(
                    &key,
                    format!("cannot read rule pack: {error}"),
                ));
                continue;
            }
        };
        let pack: RulePack = match toml::from_str(&text) {
            Ok(pack) => pack,
            Err(error) => {
                warnings.push(ConfigWarning::note(
                    &key,
                    format!("invalid rule pack: {}", error.message()),
                ));
                continue;
            }
        };
        let mut seen_in_file = HashSet::new();
        for rule in &pack.rules {
            if known.contains(&rule.name) || !seen_in_file.insert(rule.name.as_str()) {
                warnings.push(ConfigWarning::note(
                    &key,
                    format!(
                        "rule `{}` is defined more than once, file skipped",
                        rule.name
                    ),
                ));
                continue 'files;
            }
            if let Err(error) = Detector::new(std::slice::from_ref(rule)) {
                warnings.push(ConfigWarning::note(&key, format!("{error}, file skipped")));
                continue 'files;
            }
        }
        for rule in pack.rules {
            known.insert(rule.name.clone());
            loaded.push(rule);
        }
    }
    loaded
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn loads_valid_rule_packs_and_skips_invalid_files_with_warnings() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config_path = tmp.path().join("config.toml");
        let rules_dir = tmp.path().join("rules.d");
        fs::create_dir_all(&rules_dir).expect("rules.d");

        fs::write(
            &config_path,
            r#"
[detect]
disabled_rules = ["from-pack-disabled"]
"#,
        )
        .expect("config.toml");

        fs::write(
            rules_dir.join("10-good.toml"),
            r#"
[[rules]]
name = "from-pack-enabled"
pattern = ".custom-out"
markers = ["custom.toml"]
category = "build"
safety = "safe"
ecosystem = "custom"

[[rules]]
name = "from-pack-disabled"
pattern = ".custom-disabled"
markers = ["custom.toml"]
category = "cache"
"#,
        )
        .expect("10-good.toml");

        // Broken TOML syntax -> warned and skipped.
        fs::write(rules_dir.join("20-broken.toml"), "[[rules]\nname = ").expect("20-broken.toml");

        // Unknown top-level key -> warned and skipped.
        fs::write(
            rules_dir.join("30-unknown-key.toml"),
            "[scan]\nmin_size = 1\n",
        )
        .expect("30-unknown-key.toml");

        // Invalid rule pattern (contains slash) -> whole file skipped with warning.
        fs::write(
            rules_dir.join("40-bad-rule.toml"),
            r#"
[[rules]]
name = "valid-sibling-skipped"
pattern = ".valid"
category = "cache"

[[rules]]
name = "bad-pattern"
pattern = "a/b"
category = "cache"
"#,
        )
        .expect("40-bad-rule.toml");

        // Duplicate built-in rule name -> warned and skipped.
        fs::write(
            rules_dir.join("50-duplicate.toml"),
            r#"
[[rules]]
name = "node-modules"
pattern = "node_modules"
category = "dependencies"
"#,
        )
        .expect("50-duplicate.toml");

        // Non-toml file is ignored silently.
        fs::write(rules_dir.join("README.md"), "# notes\n").expect("README.md");

        let loaded = Config::load(Some(&config_path)).expect("load succeeds");
        let custom_names: Vec<&str> = loaded
            .config
            .rules
            .iter()
            .map(|r| r.name.as_str())
            .collect();
        assert_eq!(custom_names, ["from-pack-enabled", "from-pack-disabled"]);

        // Disabled pack rule stays disabled and produces no unknown-rule warning.
        let enabled_names: Vec<String> = loaded
            .config
            .rule_specs()
            .expect("rule specs")
            .into_iter()
            .map(|r| r.name)
            .collect();
        assert!(enabled_names.iter().any(|n| n == "from-pack-enabled"));
        assert!(!enabled_names.iter().any(|n| n == "from-pack-disabled"));

        // Invalid packs produced warnings and did not widen scope.
        let warning_keys: Vec<&str> = loaded.warnings.iter().map(|w| w.key.as_str()).collect();
        assert_eq!(
            warning_keys,
            [
                "rules.d/20-broken.toml",
                "rules.d/30-unknown-key.toml",
                "rules.d/40-bad-rule.toml",
                "rules.d/50-duplicate.toml",
            ]
        );
        assert!(loaded.warnings.iter().all(|w| !w.widens_scope));
    }
}
