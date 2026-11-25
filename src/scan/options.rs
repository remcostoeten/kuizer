use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::detect::Category;
use crate::units::ByteSize;

/// Options applied to one user initiated project scan and its saved result.
///
/// @name ScanOptions
/// @description Captures which scan sections and artifact categories are enabled.
///
/// @example
/// let options = ScanOptions::from_config(&config);
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanOptions {
    pub categories: Vec<Category>,
    pub min_size: u64,
    pub max_depth: Option<usize>,
    pub caches: bool,
    #[serde(default = "enabled")]
    pub toolchains: bool,
    #[serde(default = "enabled")]
    pub docker: bool,
    #[serde(default = "enabled")]
    pub repos: bool,
    pub duplicates: bool,
    pub one_file_system: bool,
}

fn enabled() -> bool {
    true
}

impl ScanOptions {
    /// @name ScanOptions::from_config
    /// @description Builds the initial scan choices from the shared configuration.
    ///
    /// @example
    /// let options = ScanOptions::from_config(&config);
    pub fn from_config(config: &Config) -> Self {
        Self {
            categories: config.detect.categories.clone(),
            min_size: config.scan.min_size.0,
            max_depth: config.scan.max_depth,
            caches: true,
            toolchains: true,
            docker: true,
            repos: true,
            duplicates: config.duplicates.enabled,
            one_file_system: config.scan.one_file_system,
        }
    }

    /// @name ScanOptions::apply
    /// @description Returns the config with these per-scan overrides applied.
    ///
    /// @example
    /// let config = options.apply(&loaded.config);
    pub fn apply(&self, config: &Config) -> Config {
        let mut config = config.clone();
        config.detect.categories = self.categories.clone();
        config.scan.min_size = ByteSize(self.min_size);
        config.scan.max_depth = self.max_depth;
        config.scan.one_file_system = self.one_file_system;
        config
    }

    /// @name ScanOptions::toggle_category
    /// @description Toggles one category while keeping at least one category enabled.
    ///
    /// @example
    /// options.toggle_category(Category::Cache);
    pub fn toggle_category(&mut self, category: Category) {
        if let Some(index) = self.categories.iter().position(|&item| item == category) {
            if self.categories.len() > 1 {
                self.categories.remove(index);
            }
        } else {
            self.categories.push(category);
            self.categories.sort();
        }
    }
}
