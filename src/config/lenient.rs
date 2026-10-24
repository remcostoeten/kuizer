use std::collections::HashSet;
use std::fmt;

use toml::{Table, Value};

use super::{Config, DeleteConfig, DeleteMode, Excludes};
use crate::detect::{Detector, builtin_rules};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigWarning {
    pub key: String,
    pub message: String,
}

impl ConfigWarning {
    fn new(key: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for ConfigWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.key.is_empty() {
            f.write_str(&self.message)
        } else {
            write!(f, "`{}`: {}", self.key, self.message)
        }
    }
}

fn accepts(root: &Table) -> Result<(), String> {
    root.clone()
        .try_into::<Config>()
        .map(drop)
        .map_err(|error| error.message().to_owned())
}

fn at<'a>(root: &'a mut Table, path: &[String]) -> Option<&'a mut Table> {
    path.iter().try_fold(root, |table, segment| {
        table.get_mut(segment)?.as_table_mut()
    })
}

fn dotted(path: &[String], key: &str) -> String {
    path.iter()
        .map(String::as_str)
        .chain([key])
        .collect::<Vec<_>>()
        .join(".")
}

fn try_set(root: &mut Table, path: &[String], key: &str, value: Value) -> Result<(), String> {
    let Some(table) = at(root, path) else {
        return Err("not a table".to_owned());
    };
    let previous = table.insert(key.to_owned(), value);
    let outcome = accepts(root);
    if outcome.is_err()
        && let Some(table) = at(root, path)
    {
        match previous {
            Some(previous) => {
                table.insert(key.to_owned(), previous);
            }
            None => {
                table.remove(key);
            }
        }
    }
    outcome
}

fn absorb_items(
    root: &mut Table,
    path: &[String],
    key: &str,
    items: Vec<Value>,
    warnings: &mut Vec<ConfigWarning>,
) {
    let offered = items.len();
    for (index, item) in items.into_iter().enumerate() {
        let pushed = at(root, path)
            .and_then(|table| table.get_mut(key)?.as_array_mut())
            .map(|array| array.push(item));
        let outcome = match pushed {
            Some(()) => accepts(root),
            None => Err("not a list".to_owned()),
        };
        if let Err(message) = outcome {
            if let Some(array) = at(root, path)
                .and_then(|table| table.get_mut(key))
                .and_then(Value::as_array_mut)
            {
                array.pop();
            }
            warnings.push(ConfigWarning::new(
                format!("{}[{index}]", dotted(path, key)),
                message,
            ));
        }
    }
    if let Some(table) = at(root, path)
        && offered > 0
        && table
            .get(key)
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty)
    {
        table.remove(key);
    }
}

fn absorb(
    root: &mut Table,
    path: &mut Vec<String>,
    user: Table,
    warnings: &mut Vec<ConfigWarning>,
) {
    for (key, value) in user {
        let whole = match value {
            Value::Table(inner) => {
                if try_set(root, path, &key, Value::Table(Table::new())).is_ok() {
                    path.push(key);
                    absorb(root, path, inner, warnings);
                    path.pop();
                    continue;
                }
                Value::Table(inner)
            }
            other => other,
        };
        let Err(message) = try_set(root, path, &key, whole.clone()) else {
            continue;
        };
        if let Value::Array(items) = whole
            && try_set(root, path, &key, Value::Array(Vec::new())).is_ok()
        {
            absorb_items(root, path, &key, items, warnings);
        } else {
            warnings.push(ConfigWarning::new(dotted(path, &key), message));
        }
    }
}

fn settle(config: &mut Config, warnings: &mut Vec<ConfigWarning>) {
    let mut known: HashSet<String> = if config.detect.builtin_rules {
        builtin_rules().into_iter().map(|rule| rule.name).collect()
    } else {
        HashSet::new()
    };
    config.rules.retain(|rule| {
        let problem = if known.contains(&rule.name) {
            Some(format!("rule `{}` is defined more than once", rule.name))
        } else {
            Detector::new(std::slice::from_ref(rule))
                .err()
                .map(|error| error.to_string())
        };
        match problem {
            Some(message) => {
                warnings.push(ConfigWarning::new(format!("rules.{}", rule.name), message));
                false
            }
            None => {
                known.insert(rule.name.clone());
                true
            }
        }
    });
    config.detect.disabled_rules.retain(|name| {
        let keep = known.contains(name);
        if !keep {
            warnings.push(ConfigWarning::new(
                "detect.disabled_rules",
                format!("unknown rule `{name}`"),
            ));
        }
        keep
    });
    config.scan.exclude.retain(
        |pattern| match Excludes::new(std::slice::from_ref(pattern), &[]) {
            Ok(_) => true,
            Err(error) => {
                warnings.push(ConfigWarning::new("scan.exclude", error.to_string()));
                false
            }
        },
    );
}

fn cautious_delete(config: &mut Config) {
    config.delete = DeleteConfig {
        mode: DeleteMode::Trash,
        confirm: true,
    };
}

fn syntax_warning(text: &str, error: &toml::de::Error) -> ConfigWarning {
    let line = error.span().map(|span| {
        text.as_bytes()[..span.start.min(text.len())]
            .iter()
            .filter(|&&byte| byte == b'\n')
            .count()
            + 1
    });
    let message = match line {
        Some(line) => format!("line {line}: {}", error.message()),
        None => error.message().to_owned(),
    };
    ConfigWarning::new("", format!("cannot parse the file: {message}"))
}

impl Config {
    /// @name Config::parse_lenient
    /// @description Parses config text without ever failing. Every setting that does not parse,
    /// is unknown, or does not validate is dropped on its own and reported as a warning, and the
    /// default stays in force for it; list entries are dropped one by one. When a `[delete]`
    /// setting is rejected, or the file is not valid TOML at all, deletion falls back to trash
    /// mode with confirmation so a typo can never make deletion less careful.
    ///
    /// @example
    /// let (config, warnings) = Config::parse_lenient("[scan]\nmin_size = 3.5\n");
    /// assert_eq!(config.scan.min_size, Config::default().scan.min_size);
    /// assert_eq!(warnings.len(), 1);
    pub fn parse_lenient(text: &str) -> (Self, Vec<ConfigWarning>) {
        let user = match text.parse::<Table>() {
            Ok(user) => user,
            Err(error) => {
                let mut config = Self::default();
                cautious_delete(&mut config);
                return (config, vec![syntax_warning(text, &error)]);
            }
        };
        let mut warnings = Vec::new();
        let mut root = Table::new();
        absorb(&mut root, &mut Vec::new(), user, &mut warnings);
        let mut config = root.try_into::<Self>().unwrap_or_default();
        let rejected_delete = warnings
            .iter()
            .any(|warning| warning.key == "delete" || warning.key.starts_with("delete."));
        config.normalize();
        settle(&mut config, &mut warnings);
        if rejected_delete {
            cautious_delete(&mut config);
            warnings.push(ConfigWarning::new(
                "delete",
                "a delete setting was rejected, so deletion uses the trash with confirmation until it is fixed",
            ));
        }
        if let Err(error) = config.validate() {
            let mut fallback = Self::default();
            cautious_delete(&mut fallback);
            warnings.push(ConfigWarning::new(
                "",
                format!("the settings do not work together ({error}), using the defaults"),
            ));
            return (fallback, warnings);
        }
        (config, warnings)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn keys(warnings: &[ConfigWarning]) -> Vec<&str> {
        warnings
            .iter()
            .map(|warning| warning.key.as_str())
            .collect()
    }

    #[test]
    fn valid_text_loads_exactly_like_the_strict_parser() {
        let text = include_str!("../../docs/config.example.toml");
        let strict = Config::parse(text, Path::new("example.toml")).expect("parses");
        assert_eq!(Config::parse_lenient(text), (strict, Vec::new()));
    }

    #[test]
    fn each_bad_setting_falls_back_on_its_own() {
        let text = r#"
            [apperance]
            motion = false

            [appearance]
            motion = false

            [scan]
            min_size = true
            max_depth = 4
            exclude = ["ok", "["]

            [detect]
            disabled_rules = ["js-dist", "nope"]
            categories = ["typo"]

            [toolchains]
            tools = ["nvm", "asdf"]

            [[rules]]
            name = "bad"
            pattern = "["
            category = "build"

            [[rules]]
            name = "good"
            pattern = "out"
            category = "build"
        "#;
        let (config, warnings) = Config::parse_lenient(text);
        let defaults = Config::default();
        assert!(!config.appearance.motion);
        assert_eq!(config.scan.max_depth, Some(4));
        assert_eq!(config.scan.min_size, defaults.scan.min_size);
        assert_eq!(config.scan.exclude, ["ok"]);
        assert_eq!(config.detect.disabled_rules, ["js-dist"]);
        assert_eq!(config.detect.categories, defaults.detect.categories);
        assert_eq!(config.toolchains.tools, [crate::toolchains::Tool::Nvm]);
        assert_eq!(config.rules.len(), 1);
        assert_eq!(config.rules[0].name, "good");
        assert_eq!(config.delete, defaults.delete);
        let mut keys = keys(&warnings);
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "apperance",
                "detect.categories[0]",
                "detect.disabled_rules",
                "rules.bad",
                "scan.exclude",
                "scan.min_size",
                "toolchains.tools[1]",
            ]
        );
    }

    #[test]
    fn a_rejected_delete_setting_makes_deletion_careful() {
        let text = "[delete]\nmode = \"permanent\"\nconfirm = \"yes\"\n";
        let (config, warnings) = Config::parse_lenient(text);
        assert_eq!(config.delete.mode, DeleteMode::Trash);
        assert!(config.delete.confirm);
        assert!(keys(&warnings).contains(&"delete.confirm"));

        let (config, warnings) = Config::parse_lenient("[delete\nmode = 1\n");
        assert_eq!(config.delete.mode, DeleteMode::Trash);
        assert!(config.delete.confirm);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("line 1"), "{warnings:?}");
    }
}
