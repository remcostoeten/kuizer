mod builtin;
mod detector;

use serde::{Deserialize, Serialize};

pub use builtin::builtin_rules;
pub use detector::{Detector, Listing, Match, RuleError, RuleInfo};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    Dependencies,
    Build,
    Cache,
    Temp,
}

impl Category {
    pub const ALL: [Category; 4] = [Self::Dependencies, Self::Build, Self::Cache, Self::Temp];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dependencies => "dependencies",
            Self::Build => "build",
            Self::Cache => "cache",
            Self::Temp => "temp",
        }
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
