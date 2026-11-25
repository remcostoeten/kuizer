use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{Config, ConfigError, Excludes, expand_home};
use crate::caches;
use crate::detect::Category;
use crate::docker::Object;
use crate::toolchains::Tool;
use crate::units::{ByteSize, Idle};

/// Commented example policy appended to `config init` output.
pub const EXAMPLE_POLICY: &str = r#"
# Named cleanup policies, run with `kuizer clean --policy NAME`.
# Inspect them with `kuizer policy list` and `kuizer policy show NAME`.
# [policy.stale-deps]
# source = "projects"
# paths = ["~/apps"]
# category = ["dependencies", "build"]
# rule = []
# idle = "60d"
# min_size = "50MB"
# exclude = []
# select = "safe"
# permanent = false
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceKind {
    Projects,
    Cache,
    Toolchain,
    Docker,
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Projects => "projects",
            Self::Cache => "cache",
            Self::Toolchain => "toolchain",
            Self::Docker => "docker",
        }
    }
}

impl fmt::Display for SourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum PolicySelect {
    #[default]
    Safe,
    All,
    Rules(Vec<String>),
}

impl fmt::Display for PolicySelect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Safe => f.write_str("safe"),
            Self::All => f.write_str("all"),
            Self::Rules(rules) => f.write_str(&rules.join(", ")),
        }
    }
}

impl Serialize for PolicySelect {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Safe => serializer.serialize_str("safe"),
            Self::All => serializer.serialize_str("all"),
            Self::Rules(rules) => rules.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for PolicySelect {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Text(String),
            List(Vec<String>),
        }
        match Raw::deserialize(deserializer)? {
            Raw::Text(text) => match text.as_str() {
                "safe" => Ok(Self::Safe),
                "all" => Ok(Self::All),
                other => Err(serde::de::Error::custom(format!(
                    "unknown select value `{other}`, expected \"safe\", \"all\", or a list of rule names"
                ))),
            },
            Raw::List(rules) => Ok(Self::Rules(rules)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub source: SourceKind,
    #[serde(default)]
    pub paths: Vec<PathBuf>,
    #[serde(default)]
    pub category: Vec<Category>,
    #[serde(default)]
    pub rule: Vec<String>,
    #[serde(default)]
    pub project: Vec<String>,
    #[serde(default)]
    pub idle: Option<Idle>,
    #[serde(default)]
    pub min_size: Option<ByteSize>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub select: PolicySelect,
    #[serde(default)]
    pub permanent: bool,
}

impl Policy {
    pub(super) fn normalize(&mut self) {
        for path in &mut self.paths {
            *path = expand_home(path);
        }
    }
}

/// @name known_rules
/// @description Returns the valid rule names for `source` under `config`.
///
/// @example
/// let names = known_rules(&config, SourceKind::Cache)?;
pub fn known_rules(config: &Config, source: SourceKind) -> Result<Vec<String>, ConfigError> {
    let mut names: Vec<String> = match source {
        SourceKind::Projects => config
            .rule_specs()?
            .into_iter()
            .map(|spec| spec.name)
            .collect(),
        SourceKind::Cache => caches::names(&[], &config.catalog),
        SourceKind::Toolchain => Tool::ALL
            .iter()
            .map(|tool| tool.name().to_owned())
            .collect(),
        SourceKind::Docker => Object::ALL
            .iter()
            .map(|object| object.name().to_owned())
            .collect(),
    };
    names.sort_unstable();
    names.dedup();
    Ok(names)
}

/// @name validate_policy
/// @description Checks a single named policy against `config` and returns a `ConfigError::Policy`
/// identifying the policy name and field if anything is invalid.
///
/// @example
/// validate_policy(&config, "stale-deps", policy)?;
pub(super) fn validate_policy(
    config: &Config,
    name: &str,
    policy: &Policy,
) -> Result<(), ConfigError> {
    let fail = |field: &str, message: String| ConfigError::Policy {
        policy: name.to_owned(),
        field: field.to_owned(),
        message,
    };
    if policy.source != SourceKind::Projects && !policy.paths.is_empty() {
        return Err(fail(
            "paths",
            format!(
                "paths is only allowed when source is \"projects\", not \"{}\"",
                policy.source
            ),
        ));
    }
    if policy.source != SourceKind::Projects && policy.idle.is_some() {
        return Err(fail(
            "idle",
            format!(
                "idle is only supported when source is \"projects\", not \"{}\"",
                policy.source
            ),
        ));
    }
    let known = known_rules(config, policy.source)?;
    for rule in &policy.rule {
        if !known.iter().any(|candidate| candidate == rule) {
            return Err(fail("rule", format!("unknown rule \"{rule}\"")));
        }
    }
    if let PolicySelect::Rules(rules) = &policy.select {
        if rules.is_empty() {
            return Err(fail(
                "select",
                "select list must contain at least one rule name".to_owned(),
            ));
        }
        for rule in rules {
            if !known.iter().any(|candidate| candidate == rule) {
                return Err(fail("select", format!("unknown rule \"{rule}\"")));
            }
        }
    }
    if let Err(error) = Excludes::new(&policy.exclude, &[]) {
        return Err(fail("exclude", error.to_string()));
    }
    if let Err(error) = Excludes::new(&policy.project, &[]) {
        return Err(fail("project", error.to_string()));
    }
    Ok(())
}
