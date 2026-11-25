use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::Path;

use serde::Serialize;
use toml::{Table as TomlTable, Value as TomlValue};
use toml_edit::{DocumentMut, Item, Table, Value};

use super::edit::EditError;
use super::{
    Config, DEFAULT_ZOOM, Excludes, LEGACY_IGNORE_FILE, MAX_SIZE_THREADS, MAX_ZOOM, MIN_ZOOM,
    Policy, expand_home, policy,
};
use crate::detect::{Detector, RuleSpec, builtin_rules};
use crate::keymap::Keymap;
use crate::platform;

/// One change applied by [`fix_text`] or [`fix_file`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConfigChange {
    pub key: String,
    pub action: String,
}

/// The rewritten config text and every change applied to produce it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixReport {
    pub text: String,
    pub changes: Vec<ConfigChange>,
}

fn accepts(root: &TomlTable) -> Result<(), String> {
    root.clone()
        .try_into::<Config>()
        .map(drop)
        .map_err(|error| error.message().to_owned())
}

fn item_to_toml(item: &Item) -> Option<TomlValue> {
    let mut doc = DocumentMut::new();
    doc.insert("v", item.clone());
    doc.to_string().parse::<TomlTable>().ok()?.remove("v")
}

fn value_to_toml(value: &Value) -> Option<TomlValue> {
    item_to_toml(&Item::Value(value.clone()))
}

fn try_set(
    root: &mut TomlTable,
    section: Option<&str>,
    key: &str,
    value: TomlValue,
) -> Result<(), String> {
    let table = match section {
        Some(section) => root
            .get_mut(section)
            .and_then(TomlValue::as_table_mut)
            .ok_or_else(|| "not a table".to_owned())?,
        None => root,
    };
    let previous = table.insert(key.to_owned(), value);
    let outcome = accepts(root);
    if outcome.is_err() {
        let table = match section {
            Some(section) => root.get_mut(section).and_then(TomlValue::as_table_mut),
            None => Some(root),
        };
        if let Some(table) = table {
            match previous {
                Some(previous) => {
                    table.insert(key.to_owned(), previous);
                }
                None => {
                    table.remove(key);
                }
            }
        }
    }
    outcome
}

fn remove_top_level(doc: &mut DocumentMut, key: &str) {
    let prefix = doc
        .get(key)
        .and_then(Item::as_table)
        .and_then(|table| table.decor().prefix())
        .and_then(|raw| raw.as_str())
        .map(str::to_owned);
    doc.remove(key);
    if let Some(prefix) = prefix
        && prefix.contains('#')
    {
        let combined = format!(
            "{}{prefix}",
            doc.decor()
                .prefix()
                .and_then(|raw| raw.as_str())
                .unwrap_or("")
        );
        doc.decor_mut().set_prefix(combined);
    }
}

fn migrate_deprecated(doc: &mut DocumentMut, changes: &mut Vec<ConfigChange>) {
    if let Some(ui_item) = doc.get_mut("ui")
        && let Some(ui) = ui_item.as_table_like_mut()
        && let Some(motion) = ui.remove("motion")
    {
        let ui_empty = ui.is_empty();
        if ui_empty {
            remove_top_level(doc, "ui");
        }
        let already_set = doc
            .get("appearance")
            .and_then(Item::as_table_like)
            .is_some_and(|appearance| appearance.contains_key("motion"));
        if already_set {
            changes.push(ConfigChange {
                key: "ui.motion".to_owned(),
                action: "removed deprecated key (`appearance.motion` is already set)".to_owned(),
            });
        } else {
            let appearance = doc
                .entry("appearance")
                .or_insert_with(|| Item::Table(Table::new()));
            if let Some(table) = appearance.as_table_like_mut() {
                table.insert("motion", motion);
                changes.push(ConfigChange {
                    key: "ui.motion".to_owned(),
                    action: "moved deprecated key to `appearance.motion`".to_owned(),
                });
            }
        }
    }

    if let Some(scan) = doc.get_mut("scan").and_then(Item::as_table_like_mut)
        && scan.get("ignore_file").and_then(Item::as_str) == Some(LEGACY_IGNORE_FILE)
    {
        scan.insert("ignore_file", Item::Value(Value::from(".kuizerignore")));
        changes.push(ConfigChange {
            key: "scan.ignore_file".to_owned(),
            action: "updated deprecated `.sweepignore` to `.kuizerignore`".to_owned(),
        });
    }
}

fn fix_array_schema(
    root: &mut TomlTable,
    section: &str,
    key: &str,
    array: &mut toml_edit::Array,
    changes: &mut Vec<ConfigChange>,
) -> bool {
    let offered = array.len();
    let mut bad_indices = Vec::new();
    for (index, value) in array.iter().enumerate() {
        let Some(toml_val) = value_to_toml(value) else {
            bad_indices.push(index);
            changes.push(ConfigChange {
                key: format!("{section}.{key}[{index}]"),
                action: "removed invalid list entry".to_owned(),
            });
            continue;
        };
        let pushed = root
            .get_mut(section)
            .and_then(TomlValue::as_table_mut)
            .and_then(|table| table.get_mut(key))
            .and_then(TomlValue::as_array_mut)
            .map(|list| list.push(toml_val));
        let outcome = match pushed {
            Some(()) => accepts(root),
            None => Err("not a list".to_owned()),
        };
        if let Err(message) = outcome {
            if let Some(list) = root
                .get_mut(section)
                .and_then(TomlValue::as_table_mut)
                .and_then(|table| table.get_mut(key))
                .and_then(TomlValue::as_array_mut)
            {
                list.pop();
            }
            bad_indices.push(index);
            changes.push(ConfigChange {
                key: format!("{section}.{key}[{index}]"),
                action: format!("removed invalid list entry ({message})"),
            });
        }
    }
    for index in bad_indices.into_iter().rev() {
        array.remove(index);
    }
    if offered > 0 && array.is_empty() {
        if let Some(table) = root.get_mut(section).and_then(TomlValue::as_table_mut) {
            table.remove(key);
        }
        return true;
    }
    false
}

fn fix_section_schema(
    doc: &mut DocumentMut,
    root: &mut TomlTable,
    section: &str,
    changes: &mut Vec<ConfigChange>,
) {
    let keys: Vec<String> = match doc.get(section).and_then(Item::as_table_like) {
        Some(table) => table.iter().map(|(k, _)| k.to_owned()).collect(),
        None => return,
    };
    for key in keys {
        let Some(item) = doc
            .get(section)
            .and_then(Item::as_table_like)
            .and_then(|table| table.get(&key))
            .cloned()
        else {
            continue;
        };
        let Some(toml_val) = item_to_toml(&item) else {
            if let Some(table) = doc.get_mut(section).and_then(Item::as_table_like_mut) {
                table.remove(&key);
            }
            changes.push(ConfigChange {
                key: format!("{section}.{key}"),
                action: "removed invalid setting".to_owned(),
            });
            continue;
        };
        let Err(message) = try_set(root, Some(section), &key, toml_val) else {
            continue;
        };
        if section == "delete" && key == "mode" {
            let _ = try_set(
                root,
                Some("delete"),
                "mode",
                TomlValue::String("trash".to_owned()),
            );
            if let Some(table) = doc.get_mut("delete").and_then(Item::as_table_like_mut) {
                table.insert("mode", Item::Value(Value::from("trash")));
            }
            changes.push(ConfigChange {
                key: "delete.mode".to_owned(),
                action: format!("replaced invalid value with `\"trash\"` ({message})"),
            });
            continue;
        }
        let is_array = item.as_array().is_some();
        if section != "keys"
            && is_array
            && try_set(root, Some(section), &key, TomlValue::Array(Vec::new())).is_ok()
        {
            let mut drop_key = false;
            if let Some(array) = doc
                .get_mut(section)
                .and_then(Item::as_table_like_mut)
                .and_then(|table| table.get_mut(&key))
                .and_then(Item::as_array_mut)
            {
                drop_key = fix_array_schema(root, section, &key, array, changes);
            }
            if drop_key && let Some(table) = doc.get_mut(section).and_then(Item::as_table_like_mut)
            {
                table.remove(&key);
            }
        } else {
            if let Some(table) = doc.get_mut(section).and_then(Item::as_table_like_mut) {
                table.remove(&key);
            }
            changes.push(ConfigChange {
                key: format!("{section}.{key}"),
                action: format!("removed invalid setting ({message})"),
            });
        }
    }
}

fn fix_rules(
    doc: &mut DocumentMut,
    builtin_enabled: bool,
    changes: &mut Vec<ConfigChange>,
) -> HashSet<String> {
    let mut known: HashSet<String> = if builtin_enabled {
        builtin_rules().into_iter().map(|rule| rule.name).collect()
    } else {
        HashSet::new()
    };
    let Some(item) = doc.get_mut("rules") else {
        return known;
    };
    if let Some(tables) = item.as_array_of_tables_mut() {
        let mut bad = Vec::new();
        for (index, table) in tables.iter().enumerate() {
            let parsed = item_to_toml(&Item::Table(table.clone()))
                .ok_or_else(|| "not a table".to_owned())
                .and_then(|value| {
                    value
                        .try_into::<RuleSpec>()
                        .map_err(|error| error.message().to_owned())
                });
            match parsed {
                Ok(rule) => {
                    let problem = if known.contains(&rule.name) {
                        Some(format!("rule `{}` is defined more than once", rule.name))
                    } else {
                        Detector::new(std::slice::from_ref(&rule))
                            .err()
                            .map(|error| error.to_string())
                    };
                    if let Some(message) = problem {
                        bad.push(index);
                        changes.push(ConfigChange {
                            key: format!("rules.{}", rule.name),
                            action: format!("removed invalid rule ({message})"),
                        });
                    } else {
                        known.insert(rule.name);
                    }
                }
                Err(message) => {
                    let label = table
                        .get("name")
                        .and_then(Item::as_str)
                        .map(|name| format!("rules.{name}"))
                        .unwrap_or_else(|| format!("rules[{index}]"));
                    bad.push(index);
                    changes.push(ConfigChange {
                        key: label,
                        action: format!("removed invalid rule ({message})"),
                    });
                }
            }
        }
        for index in bad.into_iter().rev() {
            tables.remove(index);
        }
        if tables.is_empty() {
            doc.remove("rules");
        }
        return known;
    }
    if let Some(array) = item.as_array_mut() {
        let mut bad = Vec::new();
        for (index, value) in array.iter().enumerate() {
            let parsed = value_to_toml(value)
                .ok_or_else(|| "not a rule".to_owned())
                .and_then(|value| {
                    value
                        .try_into::<RuleSpec>()
                        .map_err(|error| error.message().to_owned())
                });
            match parsed {
                Ok(rule) => {
                    let problem = if known.contains(&rule.name) {
                        Some(format!("rule `{}` is defined more than once", rule.name))
                    } else {
                        Detector::new(std::slice::from_ref(&rule))
                            .err()
                            .map(|error| error.to_string())
                    };
                    if let Some(message) = problem {
                        bad.push(index);
                        changes.push(ConfigChange {
                            key: format!("rules.{}", rule.name),
                            action: format!("removed invalid rule ({message})"),
                        });
                    } else {
                        known.insert(rule.name);
                    }
                }
                Err(message) => {
                    bad.push(index);
                    changes.push(ConfigChange {
                        key: format!("rules[{index}]"),
                        action: format!("removed invalid rule ({message})"),
                    });
                }
            }
        }
        for index in bad.into_iter().rev() {
            array.remove(index);
        }
        if array.is_empty() {
            doc.remove("rules");
        }
        return known;
    }
    doc.remove("rules");
    changes.push(ConfigChange {
        key: "rules".to_owned(),
        action: "removed invalid setting (expected a list of rule tables)".to_owned(),
    });
    known
}

fn filter_string_array<F>(
    doc: &mut DocumentMut,
    section: &str,
    key: &str,
    changes: &mut Vec<ConfigChange>,
    mut check: F,
) where
    F: FnMut(&str) -> Result<(), String>,
{
    let Some(array) = doc
        .get_mut(section)
        .and_then(Item::as_table_like_mut)
        .and_then(|table| table.get_mut(key))
        .and_then(Item::as_array_mut)
    else {
        return;
    };
    let offered = array.len();
    let mut bad = Vec::new();
    for (index, value) in array.iter().enumerate() {
        if let Some(text) = value.as_str()
            && let Err(reason) = check(text)
        {
            bad.push(index);
            changes.push(ConfigChange {
                key: format!("{section}.{key}"),
                action: format!("removed `{text}` ({reason})"),
            });
        }
    }
    for index in bad.into_iter().rev() {
        array.remove(index);
    }
    if offered > 0
        && array.is_empty()
        && let Some(table) = doc.get_mut(section).and_then(Item::as_table_like_mut)
    {
        table.remove(key);
    }
}

fn fix_policies(doc: &mut DocumentMut, base: &Config, changes: &mut Vec<ConfigChange>) {
    let names: Vec<String> = match doc.get("policy").and_then(Item::as_table_like) {
        Some(table) => table.iter().map(|(k, _)| k.to_owned()).collect(),
        None => return,
    };
    for name in names {
        let Some(item) = doc
            .get("policy")
            .and_then(Item::as_table_like)
            .and_then(|table| table.get(&name))
            .cloned()
        else {
            continue;
        };
        let result = item_to_toml(&item)
            .ok_or_else(|| "not a policy table".to_owned())
            .and_then(|value| {
                value
                    .try_into::<Policy>()
                    .map_err(|error| error.message().to_owned())
            })
            .and_then(|mut pol| {
                pol.normalize();
                policy::validate_policy(base, &name, &pol).map_err(|error| error.to_string())
            });
        if let Err(reason) = result {
            if let Some(table) = doc.get_mut("policy").and_then(Item::as_table_like_mut) {
                table.remove(&name);
            }
            changes.push(ConfigChange {
                key: format!("policy.{name}"),
                action: format!("removed invalid policy ({reason})"),
            });
        }
    }
}

/// @name fix_text
/// @description Rewrites deprecated keys and removes or corrects invalid settings in `text` while
/// keeping surrounding comments and layout intact. Returns an error only when `text` is not valid
/// TOML syntax at all, so broken files are never overwritten blindly.
///
/// @example
/// let report = fix_text("[ui]\nmotion = false\n")?;
/// assert!(report.text.contains("[appearance]"));
/// assert_eq!(report.changes.len(), 1);
pub fn fix_text(text: &str) -> Result<FixReport, String> {
    let mut doc: DocumentMut = text
        .parse()
        .map_err(|error: toml_edit::TomlError| error.to_string())?;
    let mut changes = Vec::new();

    migrate_deprecated(&mut doc, &mut changes);

    let mut root = TomlTable::new();
    let top_keys: Vec<String> = doc.iter().map(|(k, _)| k.to_owned()).collect();
    for key in top_keys {
        if key == "rules" || key == "policy" {
            continue;
        }
        let Some(item) = doc.get(&key).cloned() else {
            continue;
        };
        if item.is_table_like() {
            if try_set(&mut root, None, &key, TomlValue::Table(TomlTable::new())).is_ok() {
                fix_section_schema(&mut doc, &mut root, &key, &mut changes);
            } else {
                remove_top_level(&mut doc, &key);
                changes.push(ConfigChange {
                    key,
                    action: "removed unknown section".to_owned(),
                });
            }
        } else {
            let outcome = item_to_toml(&item)
                .ok_or_else(|| "invalid value".to_owned())
                .and_then(|value| try_set(&mut root, None, &key, value));
            if let Err(message) = outcome {
                remove_top_level(&mut doc, &key);
                changes.push(ConfigChange {
                    key,
                    action: format!("removed invalid setting ({message})"),
                });
            }
        }
    }

    if let Some(item) = doc.get("policy")
        && !item.is_table_like()
    {
        doc.remove("policy");
        changes.push(ConfigChange {
            key: "policy".to_owned(),
            action: "removed invalid setting (expected a table)".to_owned(),
        });
    }

    let builtin_enabled = doc
        .get("detect")
        .and_then(Item::as_table_like)
        .and_then(|detect| detect.get("builtin_rules"))
        .and_then(Item::as_bool)
        .unwrap_or(true);
    let known_rules = fix_rules(&mut doc, builtin_enabled, &mut changes);

    filter_string_array(&mut doc, "detect", "disabled_rules", &mut changes, |name| {
        if known_rules.contains(name) {
            Ok(())
        } else {
            Err(format!("unknown rule `{name}`"))
        }
    });

    filter_string_array(&mut doc, "scan", "exclude", &mut changes, |pattern| {
        Excludes::new(&[pattern.to_owned()], &[])
            .map(drop)
            .map_err(|error| error.to_string())
    });

    filter_string_array(&mut doc, "delete", "protected", &mut changes, |raw| {
        let expanded = expand_home(Path::new(raw));
        if expanded.is_absolute() && expanded.is_dir() {
            Ok(())
        } else {
            Err("not an existing absolute directory".to_owned())
        }
    });

    if let Some(scan) = doc.get_mut("scan").and_then(Item::as_table_like_mut)
        && let Some(threads) = scan.get("size_threads").and_then(Item::as_integer)
        && threads > MAX_SIZE_THREADS as i64
    {
        scan.insert(
            "size_threads",
            Item::Value(Value::from(MAX_SIZE_THREADS as i64)),
        );
        changes.push(ConfigChange {
            key: "scan.size_threads".to_owned(),
            action: format!("clamped {threads} to {MAX_SIZE_THREADS}"),
        });
    }

    if let Some(appearance) = doc.get_mut("appearance").and_then(Item::as_table_like_mut)
        && let Some(zoom) = appearance.get("zoom").and_then(Item::as_integer)
        && !(i64::from(MIN_ZOOM)..=i64::from(MAX_ZOOM)).contains(&zoom)
    {
        appearance.insert("zoom", Item::Value(Value::from(i64::from(DEFAULT_ZOOM))));
        changes.push(ConfigChange {
            key: "appearance.zoom".to_owned(),
            action: format!(
                "reset {zoom} to {DEFAULT_ZOOM} (valid range {MIN_ZOOM} to {MAX_ZOOM})"
            ),
        });
    }

    // Validate [keys] entries against Keymap::resolve directly from the document.
    if let Some(raw_keys) = doc
        .get("keys")
        .and_then(item_to_toml)
        .and_then(|value| value.try_into::<crate::keymap::Overrides>().ok())
    {
        let (_, key_problems) = Keymap::resolve(&raw_keys);
        for problem in key_problems {
            if let Some(keys) = doc.get_mut("keys").and_then(Item::as_table_like_mut)
                && keys.remove(&problem.name).is_some()
            {
                changes.push(ConfigChange {
                    key: format!("keys.{}", problem.name),
                    action: format!("removed invalid keybinding ({})", problem.message),
                });
            }
        }
    }

    // Validate [policy.*] against the cleaned rules and categories.
    let (base, _) = Config::parse_lenient(&doc.to_string());
    fix_policies(&mut doc, &base, &mut changes);

    Ok(FixReport {
        text: doc.to_string(),
        changes,
    })
}

/// @name fix_file
/// @description Runs [`fix_text`] on the config file at `path` and atomically writes it back when
/// at least one change was made. Returns the list of changes applied.
///
/// @example
/// let changes = fix_file(&config_path)?;
/// for change in &changes {
///     println!("{}: {}", change.key, change.action);
/// }
pub fn fix_file(path: &Path) -> Result<Vec<ConfigChange>, EditError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(EditError::Read {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    let report = fix_text(&text).map_err(|message| EditError::Parse {
        path: path.to_path_buf(),
        message,
    })?;
    if !report.changes.is_empty() {
        platform::write_atomic(path, report.text.as_bytes()).map_err(|source| {
            EditError::Write {
                path: path.to_path_buf(),
                source,
            }
        })?;
    }
    Ok(report.changes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_deprecated_ui_motion_and_legacy_ignore_file_keeping_comments() {
        let input =
            "# keep me\n[ui]\nmotion = false # smooth\n\n[scan]\nignore_file = \".sweepignore\"\n";
        let report = fix_text(input).expect("fixes");
        assert!(report.text.starts_with("# keep me\n"));
        assert!(!report.text.contains("[ui]"));
        assert_eq!(report.changes.len(), 2, "{:?}", report.changes);
        assert_eq!(report.changes[0].key, "ui.motion");
        assert_eq!(report.changes[1].key, "scan.ignore_file");

        let (config, warnings) = Config::parse_lenient(&report.text);
        assert!(warnings.is_empty(), "{warnings:?}\n{}", report.text);
        assert!(!config.appearance.motion);
        assert_eq!(config.scan.ignore_file, ".kuizerignore");
    }

    #[test]
    fn keeps_appearance_motion_when_both_ui_and_appearance_are_set() {
        let input = "[ui]\nmotion = false\n[appearance]\nmotion = true\n";
        let report = fix_text(input).expect("fixes");
        let (config, warnings) = Config::parse_lenient(&report.text);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(config.appearance.motion);
        assert_eq!(report.changes.len(), 1);
        assert_eq!(report.changes[0].key, "ui.motion");
    }

    #[test]
    fn fixes_invalid_keys_lists_clamps_rules_keys_and_policies_to_zero_warnings() {
        let temp = tempfile::tempdir().expect("tempdir");
        let existing = temp.path().join("keep");
        fs::create_dir(&existing).expect("dir");
        let missing = temp.path().join("gone");
        let input = format!(
            r#"# user header
[apperance]
motion = false

[appearance]
zoom = 400
motion = false

[scan]
min_size = true
max_depth = 4
size_threads = 128
exclude = ["ok", "["]

[detect]
disabled_rules = ["js-dist", "nope"]
categories = ["typo"]

[delete]
mode = "trsh"
protected = ["{}", "{}"]

[toolchains]
tools = ["nvm", "asdf"]

[keys]
"nav.down" = "n"
"nope.action" = "x"

[[rules]]
name = "bad"
pattern = "["
category = "build"

[[rules]]
name = "good"
pattern = "out"
category = "build"

[policy.broken]
source = "cache"
paths = ["/tmp"]

[policy.good]
source = "projects"
select = "safe"
"#,
            existing.display(),
            missing.display()
        );
        let report = fix_text(&input).expect("fixes");
        assert!(report.text.starts_with("# user header\n"));
        assert!(!report.changes.is_empty());

        let (config, warnings) = Config::parse_lenient(&report.text);
        assert!(warnings.is_empty(), "{warnings:?}\n{}", report.text);
        config.validate().expect("valid");
        assert!(!config.appearance.motion);
        assert_eq!(config.appearance.zoom, DEFAULT_ZOOM);
        assert_eq!(config.scan.max_depth, Some(4));
        assert_eq!(config.scan.size_threads, MAX_SIZE_THREADS);
        assert_eq!(config.scan.exclude, ["ok"]);
        assert_eq!(config.detect.disabled_rules, ["js-dist"]);
        assert_eq!(
            config.detect.categories,
            Config::default().detect.categories
        );
        assert_eq!(config.delete.mode, crate::config::DeleteMode::Trash);
        assert_eq!(config.delete.protected, [existing]);
        assert_eq!(config.toolchains.tools, [crate::toolchains::Tool::Nvm]);
        assert_eq!(config.rules.len(), 1);
        assert_eq!(config.rules[0].name, "good");
        assert!(config.policy.contains_key("good"));
        assert!(!config.policy.contains_key("broken"));

        // Running fix_text a second time is a no-op.
        let second = fix_text(&report.text).expect("idempotent");
        assert!(second.changes.is_empty(), "{:?}", second.changes);
    }

    #[test]
    fn refuses_unparseable_toml_without_modifying_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("config.toml");
        fs::write(&file, "[scan\nmin_size = 1\n").expect("write");
        let err = fix_file(&file).expect_err("broken toml");
        assert!(matches!(err, EditError::Parse { .. }));
        assert_eq!(
            fs::read_to_string(&file).expect("read"),
            "[scan\nmin_size = 1\n"
        );
    }
}
