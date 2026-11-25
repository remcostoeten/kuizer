mod resolve;

use std::collections::HashSet;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::config::ConfigError;
use crate::detect::{Category, Kind, Safety};

pub use resolve::{Locations, Resolved, current_platform, resolve};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "cli", derive(clap::ValueEnum))]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(feature = "cli", value(rename_all = "kebab-case"))]
pub enum Group {
    Developer,
    Apps,
    Browsers,
    System,
}

impl Group {
    pub const ALL: [Self; 4] = [Self::Developer, Self::Apps, Self::Browsers, Self::System];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Developer => "developer",
            Self::Apps => "apps",
            Self::Browsers => "browsers",
            Self::System => "system",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Base {
    Home,
    Cache,
    Config,
    Data,
    State,
    Library,
    Cargo,
    GoModules,
    Gradle,
    Bun,
    Android,
    AndroidAvd,
    HuggingFace,
    Ollama,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Platform {
    Linux,
    Macos,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub name: String,
    pub app: String,
    pub group: Group,
    #[serde(default = "default_category")]
    pub category: Category,
    #[serde(default = "default_kind")]
    pub kind: Kind,
    pub base: Base,
    pub path: String,
    #[serde(default = "default_safety")]
    pub safety: Safety,
    #[serde(default)]
    pub platforms: Vec<Platform>,
    #[serde(default)]
    pub processes: Vec<String>,
    #[serde(default)]
    pub note: String,
}

fn default_category() -> Category {
    Category::Cache
}

fn default_kind() -> Kind {
    Kind::Dir
}

fn default_safety() -> Safety {
    Safety::Review
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogFile {
    #[serde(default)]
    entry: Vec<Entry>,
}

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("invalid catalog {origin}: {source}")]
    Parse {
        origin: String,
        source: Box<toml::de::Error>,
    },
    #[error("duplicate catalog entry `{0}`")]
    Duplicate(String),
    #[error("invalid catalog entry name `{0}`: use lowercase letters, digits and hyphens")]
    InvalidName(String),
    #[error("catalog entry `{0}` must have a non-empty app name")]
    EmptyApp(String),
    #[error("invalid path `{path}` in catalog entry `{name}`: {reason}")]
    InvalidPath {
        name: String,
        path: String,
        reason: &'static str,
    },
    #[error("catalog entry `{name}` uses category `{category}`; use cache, logs or temp")]
    InvalidCategory {
        name: String,
        category: &'static str,
    },
    #[error("catalog entry `{name}` uses unsupported kind; use dir or file")]
    InvalidKind { name: String },
}

pub(crate) fn check_entry(entry: &Entry) -> Result<(), CatalogError> {
    if !valid_name(&entry.name) {
        return Err(CatalogError::InvalidName(entry.name.clone()));
    }
    if entry.app.trim().is_empty() {
        return Err(CatalogError::EmptyApp(entry.name.clone()));
    }
    check_path(&entry.name, &entry.path)?;
    if !matches!(
        entry.category,
        Category::Cache | Category::Logs | Category::Temp
    ) {
        return Err(CatalogError::InvalidCategory {
            name: entry.name.clone(),
            category: entry.category.as_str(),
        });
    }
    if !matches!(entry.kind, Kind::Dir | Kind::File) {
        return Err(CatalogError::InvalidKind {
            name: entry.name.clone(),
        });
    }
    Ok(())
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && !name.ends_with('-')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn check_path(name: &str, path: &str) -> Result<(), CatalogError> {
    if path.is_empty() {
        return Ok(());
    }
    if path.starts_with('/') {
        return Err(CatalogError::InvalidPath {
            name: name.to_owned(),
            path: path.to_owned(),
            reason: "must be relative to base",
        });
    }
    if path.contains('\\') {
        return Err(CatalogError::InvalidPath {
            name: name.to_owned(),
            path: path.to_owned(),
            reason: "backslashes are not allowed",
        });
    }
    for component in path.split('/') {
        if component.is_empty() {
            return Err(CatalogError::InvalidPath {
                name: name.to_owned(),
                path: path.to_owned(),
                reason: "contains an empty path component",
            });
        }
        if component == "." || component == ".." {
            return Err(CatalogError::InvalidPath {
                name: name.to_owned(),
                path: path.to_owned(),
                reason: "`.` and `..` components are not allowed",
            });
        }
        if component != "*" && component.contains(['*', '?', '[', ']', '{', '}']) {
            return Err(CatalogError::InvalidPath {
                name: name.to_owned(),
                path: path.to_owned(),
                reason: "only standalone `*` wildcard components are allowed",
            });
        }
    }
    Ok(())
}

/// @name parse
/// @description Parses a TOML location catalog file and validates every `[[entry]]`.
/// Supported bases are `home` (home folder), `cache` (`dirs::cache_dir()`), `config`
/// (`dirs::config_dir()`), `data` (`dirs::data_dir()`), `state` (`dirs::state_dir()`, Linux
/// only), `library` (`~/Library`, macOS only), and tool bases `cargo`, `go-modules`, `gradle`,
/// `bun`, `android`, `android-avd`, `hugging-face`, `ollama`.
///
/// @example
/// let entries = kuizer::catalog::parse("", "inline")?;
/// assert!(entries.is_empty());
pub fn parse(text: &str, origin: &str) -> Result<Vec<Entry>, CatalogError> {
    let parsed: CatalogFile = toml::from_str(text).map_err(|source| CatalogError::Parse {
        origin: origin.to_owned(),
        source: Box::new(source),
    })?;
    for entry in &parsed.entry {
        check_entry(entry)?;
    }
    check_unique(&parsed.entry)?;
    Ok(parsed.entry)
}

/// @name check_unique
/// @description Verifies that no two catalog entries share a `name`.
///
/// @example
/// kuizer::catalog::check_unique(kuizer::catalog::embedded())?;
pub fn check_unique<'a>(entries: impl IntoIterator<Item = &'a Entry>) -> Result<(), CatalogError> {
    let mut seen = HashSet::new();
    for entry in entries {
        if !seen.insert(entry.name.as_str()) {
            return Err(CatalogError::Duplicate(entry.name.clone()));
        }
    }
    Ok(())
}

const EMBEDDED_FILES: [(&str, &str); 4] = [
    ("developer.toml", include_str!("data/developer.toml")),
    ("linux.toml", include_str!("data/linux.toml")),
    ("macos.toml", include_str!("data/macos.toml")),
    ("browsers.toml", include_str!("data/browsers.toml")),
];

/// @name embedded
/// @description Returns the built-in location catalog entries in order: developer, linux, macos,
/// browsers.
///
/// @example
/// let _entries = kuizer::catalog::embedded();
pub fn embedded() -> &'static [Entry] {
    static ENTRIES: OnceLock<Vec<Entry>> = OnceLock::new();
    ENTRIES.get_or_init(|| {
        let mut all = Vec::new();
        for (origin, text) in EMBEDDED_FILES {
            if let Ok(entries) = parse(text, origin) {
                all.extend(entries);
            }
        }
        all
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CatalogConfig {
    pub disabled: Vec<String>,
    pub locations: Vec<Entry>,
    pub unknown_caches: bool,
}

impl Default for CatalogConfig {
    fn default() -> Self {
        Self {
            disabled: Vec::new(),
            locations: Vec::new(),
            unknown_caches: true,
        }
    }
}

impl CatalogConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        let embedded_names: HashSet<&str> =
            embedded().iter().map(|entry| entry.name.as_str()).collect();
        let mut user_names: HashSet<&str> = HashSet::new();
        for (index, entry) in self.locations.iter().enumerate() {
            if let Err(error) = check_entry(entry) {
                return Err(ConfigError::Invalid {
                    key: format!("catalog.locations[{index}]"),
                    message: error.to_string(),
                });
            }
            if embedded_names.contains(entry.name.as_str()) {
                return Err(ConfigError::Invalid {
                    key: format!("catalog.locations[{index}].name"),
                    message: format!("`{}` collides with a built-in catalog entry", entry.name),
                });
            }
            if !user_names.insert(entry.name.as_str()) {
                return Err(ConfigError::Invalid {
                    key: format!("catalog.locations[{index}].name"),
                    message: format!("duplicate catalog entry `{}`", entry.name),
                });
            }
        }
        for name in &self.disabled {
            if !embedded_names.contains(name.as_str()) && !user_names.contains(name.as_str()) {
                return Err(ConfigError::Invalid {
                    key: "catalog.disabled".to_owned(),
                    message: format!("unknown catalog entry `{name}`"),
                });
            }
        }
        Ok(())
    }
}

/// @name entries
/// @description Returns the active catalog entries for `config`: user locations first, then
/// built-in entries, minus any names in `config.disabled`.
///
/// @example
/// let active = kuizer::catalog::entries(&config.catalog);
pub fn entries(config: &CatalogConfig) -> Vec<Entry> {
    let disabled: HashSet<&str> = config.disabled.iter().map(String::as_str).collect();
    config
        .locations
        .iter()
        .chain(embedded())
        .filter(|entry| !disabled.contains(entry.name.as_str()))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalog_is_valid() {
        let mut all = Vec::new();
        for (origin, text) in EMBEDDED_FILES {
            let entries = parse(text, origin).unwrap_or_else(|error| panic!("{origin}: {error}"));
            all.extend(entries);
        }
        check_unique(&all).expect("embedded names must be unique");
    }

    #[test]
    fn rejects_bad_entries() {
        let cases = [
            (
                "dotdot path",
                r#"
[[entry]]
name = "bad-dotdot"
app = "App"
group = "apps"
base = "cache"
path = "../escape"
"#,
            ),
            (
                "partial wildcard",
                r#"
[[entry]]
name = "bad-glob"
app = "App"
group = "apps"
base = "cache"
path = "a*b"
"#,
            ),
            (
                "uppercase name",
                r#"
[[entry]]
name = "BadName"
app = "App"
group = "apps"
base = "cache"
path = "ok"
"#,
            ),
            (
                "external kind",
                r#"
[[entry]]
name = "bad-kind"
app = "App"
group = "apps"
kind = "external"
base = "cache"
path = "ok"
"#,
            ),
            (
                "build category",
                r#"
[[entry]]
name = "bad-category"
app = "App"
group = "apps"
category = "build"
base = "cache"
path = "ok"
"#,
            ),
            (
                "duplicate name",
                r#"
[[entry]]
name = "dup"
app = "App"
group = "apps"
base = "cache"
path = "one"

[[entry]]
name = "dup"
app = "App"
group = "apps"
base = "cache"
path = "two"
"#,
            ),
        ];

        for (label, text) in cases {
            assert!(parse(text, label).is_err(), "{label} should be rejected");
        }
    }

    #[test]
    fn accepts_wildcard_and_empty_path() {
        let text = r#"
[[entry]]
name = "chrome-cache"
app = "Chrome"
group = "browsers"
base = "cache"
path = "google-chrome/*/Cache"

[[entry]]
name = "go-modules"
app = "Go"
group = "developer"
base = "go-modules"
path = ""
"#;
        let entries = parse(text, "inline").expect("parses");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, "google-chrome/*/Cache");
        assert_eq!(entries[1].path, "");
        assert_eq!(entries[0].category, Category::Cache);
        assert_eq!(entries[0].kind, Kind::Dir);
        assert_eq!(entries[0].safety, Safety::Review);
    }
}
