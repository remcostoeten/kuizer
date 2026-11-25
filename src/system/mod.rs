mod list;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::ConfigError;
use crate::detect::{Category, Kind, RuleInfo, Safety};
use crate::scan::{Candidate, Usage};

pub use list::{Listing, Tools, for_user, list};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Manager {
    Pacman,
    Apt,
    Dnf,
    Zypper,
    Flatpak,
    Snap,
    Homebrew,
    Journal,
}

impl Manager {
    pub const ALL: [Self; 8] = [
        Self::Pacman,
        Self::Apt,
        Self::Dnf,
        Self::Zypper,
        Self::Flatpak,
        Self::Snap,
        Self::Homebrew,
        Self::Journal,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Pacman => "pacman",
            Self::Apt => "apt",
            Self::Dnf => "dnf",
            Self::Zypper => "zypper",
            Self::Flatpak => "flatpak",
            Self::Snap => "snap",
            Self::Homebrew => "homebrew",
            Self::Journal => "journal",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "pacman" => Some(Self::Pacman),
            "apt" => Some(Self::Apt),
            "dnf" => Some(Self::Dnf),
            "zypper" => Some(Self::Zypper),
            "flatpak" => Some(Self::Flatpak),
            "snap" => Some(Self::Snap),
            "homebrew" => Some(Self::Homebrew),
            "journal" => Some(Self::Journal),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub manager: Manager,
    pub id: String,
    pub label: String,
    pub bytes: u64,
    pub size_known: bool,
    pub safety: Safety,
    pub needs_root: bool,
    pub note: String,
}

impl Item {
    /// @name Item::path
    /// @description Returns the stable pseudo-path identifying this system item.
    ///
    /// @example
    /// assert_eq!(item.path(), PathBuf::from("system:journal/journal"));
    pub fn path(&self) -> PathBuf {
        PathBuf::from(format!("system:{}/{}", self.manager.name(), self.id))
    }

    /// @name Item::candidate
    /// @description Wraps this system item as a report candidate with `Kind::External`.
    ///
    /// @example
    /// let candidate = item.candidate(1);
    pub fn candidate(&self, id: u64) -> Candidate {
        let category = if self.manager == Manager::Journal {
            Category::Logs
        } else {
            Category::Cache
        };
        Candidate {
            id,
            path: self.path(),
            kind: Kind::External,
            rule: Arc::new(RuleInfo {
                name: self.manager.name().to_owned(),
                category,
                safety: self.safety,
                ecosystem: "system".to_owned(),
            }),
            project: None,
            usage: Usage {
                allocated: self.bytes,
                apparent: self.bytes,
                entries: 1,
                partial: !self.size_known,
            },
            newest: None,
            identity: None,
            project_last_active: None,
        }
    }
}

/// @name entries
/// @description Pairs each listed system item with a candidate numbered from 1.
///
/// @example
/// let rows = system::entries(listing.items);
pub fn entries(items: Vec<Item>) -> Vec<(Item, Candidate)> {
    items
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            let candidate = item.candidate((index + 1) as u64);
            (item, candidate)
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SystemConfig {
    pub managers: Vec<Manager>,
    pub timeout_secs: u64,
}

impl Default for SystemConfig {
    fn default() -> Self {
        Self {
            managers: Manager::ALL.to_vec(),
            timeout_secs: 10,
        }
    }
}

impl SystemConfig {
    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_secs.max(1))
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.timeout_secs == 0 {
            return Err(ConfigError::Invalid {
                key: "system.timeout_secs".to_owned(),
                message: "must be at least 1".to_owned(),
            });
        }
        Ok(())
    }
}
