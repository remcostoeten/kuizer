mod builtin;
mod detector;
mod explain;

use serde::{Deserialize, Serialize};

pub use builtin::builtin_rules;
pub use detector::{Detector, Listing, Match, RuleError, RuleInfo};
pub use explain::{Explanation, MatchedRule, NearMiss, explain_path};

pub(crate) const APP_DATA_MARKERS: &[&[&str]] = &[
    &["Local State"],
    &["profiles.ini"],
    &["Cookies", "Preferences"],
    &["cookies.sqlite", "prefs.js"],
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    Dependencies,
    Build,
    Cache,
    Temp,
    Logs,
    Trash,
    Files,
}

impl Category {
    pub const ALL: [Category; 4] = [Self::Dependencies, Self::Build, Self::Cache, Self::Temp];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dependencies => "dependencies",
            Self::Build => "build",
            Self::Cache => "cache",
            Self::Temp => "temp",
            Self::Logs => "logs",
            Self::Trash => "trash",
            Self::Files => "files",
        }
    }

    /// @name Category::is_rule_category
    /// @description Whether a project detection rule may use this category. Logs, trash and
    /// files only come from the location catalog and the trash and big-file sources.
    ///
    /// @example
    /// assert!(!Category::Trash.is_rule_category());
    pub fn is_rule_category(self) -> bool {
        Self::ALL.contains(&self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Safety {
    Safe,
    Review,
}

impl Safety {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Safe => "safe",
            Self::Review => "review",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Dir,
    File,
    Symlink,
    External,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleSpec {
    pub name: String,
    #[serde(default = "default_kind")]
    pub kind: Kind,
    pub pattern: String,
    #[serde(default)]
    pub markers: Vec<String>,
    #[serde(default)]
    pub contains: Vec<String>,
    pub category: Category,
    #[serde(default = "default_safety")]
    pub safety: Safety,
    #[serde(default = "default_ecosystem")]
    pub ecosystem: String,
}

fn default_kind() -> Kind {
    Kind::Dir
}

fn default_safety() -> Safety {
    Safety::Review
}

fn default_ecosystem() -> String {
    "custom".to_owned()
}

pub const CACHEDIR_TAG: &str = "CACHEDIR.TAG";
pub const CACHEDIR_SIGNATURE: &[u8] = b"Signature: 8a477f597d28d172789f06886806bc55";
