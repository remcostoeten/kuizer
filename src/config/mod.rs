mod exclude;

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::detect::{Category, Detector, Listing, RuleError, RuleSpec, builtin_rules};
use crate::platform;
use crate::units::ByteSize;

pub use exclude::{ExcludeError, Excludes};

pub const LEGACY_IGNORE_FILE: &str = ".sweepignore";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub scan: ScanConfig,
    pub detect: DetectConfig,
    pub rules: Vec<RuleSpec>,
    pub duplicates: DuplicateConfig,
    pub delete: DeleteConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScanConfig {
    pub roots: Vec<PathBuf>,
    pub exclude: Vec<String>,
    pub exclude_extensions: Vec<String>,
    pub skip_dirs: Vec<String>,
    pub ignore_file: String,
    pub min_size: ByteSize,
    pub max_depth: Option<usize>,
    pub one_file_system: bool,
    pub excluded_fs_types: Vec<String>,
    pub excluded_mounts: Vec<PathBuf>,
    pub size_threads: usize,
}

impl ScanConfig {
    /// @name ScanConfig::ignores
    /// @description Returns whether a directory listing holds the configured ignore file or the
    /// legacy `.sweepignore` marker, so markers written before the rename keep working.
    ///
    /// @example
    /// if config.scan.ignores(&listing) {
    ///     return;
    /// }
    pub fn ignores(&self, listing: &Listing) -> bool {
        listing.has(self.ignore_file.as_ref()) || listing.has(LEGACY_IGNORE_FILE.as_ref())
    }
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            roots: Vec::new(),
            exclude: Vec::new(),
            exclude_extensions: Vec::new(),
            skip_dirs: [".git", ".hg", ".svn", ".jj"].map(String::from).to_vec(),
            ignore_file: ".kuizerignore".to_owned(),
            min_size: ByteSize(0),
            max_depth: None,
            one_file_system: true,
            excluded_fs_types: [
                "proc",
                "sysfs",
                "devtmpfs",
                "devpts",
                "cgroup",
                "cgroup2",
                "securityfs",
                "debugfs",
                "tracefs",
                "pstore",
                "bpf",
                "configfs",
                "fusectl",
                "mqueue",
                "hugetlbfs",
                "autofs",
                "binfmt_misc",
                "efivarfs",
                "nsfs",
                "nfs",
                "nfs4",
                "cifs",
                "smb3",
                "fuse.sshfs",
            ]
            .map(String::from)
            .to_vec(),
            excluded_mounts: ["/proc", "/sys", "/dev", "/run"]
                .map(PathBuf::from)
                .to_vec(),
            size_threads: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DetectConfig {
    pub categories: Vec<Category>,
    pub builtin_rules: bool,
    pub disabled_rules: Vec<String>,
}

impl Default for DetectConfig {
    fn default() -> Self {
        Self {
            categories: Category::ALL.to_vec(),
            builtin_rules: true,
            disabled_rules: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DuplicateConfig {
    pub min_size: ByteSize,
    pub threads: usize,
    pub verify: bool,
}

impl Default for DuplicateConfig {
    fn default() -> Self {
        Self {
            min_size: ByteSize(4096),
            threads: 0,
            verify: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum DeleteMode {
    #[default]
    Trash,
    Permanent,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct DeleteConfig {
    pub mode: DeleteMode,
    pub confirm: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read config {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid config {path}: {source}")]
    Parse {
        path: PathBuf,
        source: Box<toml::de::Error>,
    },
    #[error(transparent)]
    Rule(#[from] RuleError),
    #[error(transparent)]
    Exclude(#[from] ExcludeError),
    #[error("unknown rule `{0}` in detect.disabled_rules")]
    UnknownDisabled(String),
}

pub struct Loaded {
    pub config: Config,
    pub source: Option<PathBuf>,
}

pub fn default_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("kuizer").join("config.toml"))
}

pub fn expand_home(path: &Path) -> PathBuf {
    match (path.strip_prefix("~"), platform::home_dir()) {
        (Ok(rest), Some(home)) => home.join(rest),
        _ => path.to_path_buf(),
    }
}

impl Config {
    pub fn parse(text: &str, origin: &Path) -> Result<Self, ConfigError> {
        let mut config: Self = toml::from_str(text).map_err(|source| ConfigError::Parse {
            path: origin.to_path_buf(),
            source: Box::new(source),
        })?;
        config.normalize();
        config.validate()?;
        Ok(config)
    }

    pub fn load(explicit: Option<&Path>) -> Result<Loaded, ConfigError> {
        let path = match explicit {
            Some(path) => Some(expand_home(path)),
            None => default_path().filter(|path| path.is_file()),
        };
        let Some(path) = path else {
            return Ok(Loaded {
                config: Self::default(),
                source: None,
            });
        };
        let text = fs::read_to_string(&path).map_err(|source| ConfigError::Read {
            path: path.clone(),
            source,
        })?;
        Ok(Loaded {
            config: Self::parse(&text, &path)?,
            source: Some(path),
        })
    }

    fn normalize(&mut self) {
        for root in &mut self.scan.roots {
            *root = expand_home(root);
        }
        for mount in &mut self.scan.excluded_mounts {
            *mount = expand_home(mount);
        }
        for extension in &mut self.scan.exclude_extensions {
            *extension = extension.trim_start_matches('.').to_ascii_lowercase();
        }
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        self.detector()?;
        Excludes::new(&self.scan.exclude, &self.scan.exclude_extensions)?;
        Ok(())
    }

    pub fn rule_specs(&self) -> Result<Vec<RuleSpec>, ConfigError> {
        Ok(self
            .enabled_rules()?
            .into_iter()
            .filter(|rule| self.detect.categories.contains(&rule.category))
            .collect())
    }

    fn enabled_rules(&self) -> Result<Vec<RuleSpec>, ConfigError> {
        let builtins = if self.detect.builtin_rules {
            builtin_rules()
        } else {
            Vec::new()
        };
        for name in &self.detect.disabled_rules {
            let known = builtins
                .iter()
                .chain(&self.rules)
                .any(|rule| &rule.name == name);
            if !known {
                return Err(ConfigError::UnknownDisabled(name.clone()));
            }
        }
        Ok(self
            .rules
            .iter()
            .cloned()
            .chain(builtins)
            .filter(|rule| !self.detect.disabled_rules.contains(&rule.name))
            .collect())
    }

    pub fn detector(&self) -> Result<Detector, ConfigError> {
        Ok(Detector::new(&self.enabled_rules()?)?.reporting(self.detect.categories.clone()))
    }

    pub fn excludes(&self) -> Result<Excludes, ConfigError> {
        Ok(Excludes::new(
            &self.scan.exclude,
            &self.scan.exclude_extensions,
        )?)
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::Safety;

    fn parse(text: &str) -> Result<Config, ConfigError> {
        Config::parse(text, Path::new("test.toml"))
    }

    #[test]
    fn empty_file_gives_defaults() {
        let config = parse("").expect("parses");
        assert_eq!(config, Config::default());
        assert!(config.scan.one_file_system);
    }

    #[test]
    fn rejects_unknown_fields() {
        assert!(matches!(
            parse("[scan]\nfollow_symlink = true\n"),
            Err(ConfigError::Parse { .. })
        ));
    }

    #[test]
    fn parses_full_example() {
        let config = parse(include_str!("../../docs/config.example.toml")).expect("parses");
        assert_eq!(config.scan.min_size, ByteSize(1_000_000));
        assert_eq!(config.rules.len(), 1);
        assert_eq!(config.rules[0].safety, Safety::Review);
        assert_eq!(config.delete.mode, DeleteMode::Trash);
    }

    #[test]
    fn normalizes_extensions() {
        let config = parse("[scan]\nexclude_extensions = [\".LOG\", \"tmp\"]\n").expect("parses");
        assert_eq!(config.scan.exclude_extensions, ["log", "tmp"]);
    }

    #[test]
    fn disabled_rules_must_exist() {
        assert!(matches!(
            parse("[detect]\ndisabled_rules = [\"nope\"]\n"),
            Err(ConfigError::UnknownDisabled(_))
        ));
        let config = parse("[detect]\ndisabled_rules = [\"js-dist\"]\n").expect("parses");
        let specs = config.rule_specs().expect("specs");
        assert!(specs.iter().all(|rule| rule.name != "js-dist"));
    }

    #[test]
    fn categories_filter_rules() {
        let config = parse("[detect]\ncategories = [\"dependencies\"]\n").expect("parses");
        let specs = config.rule_specs().expect("specs");
        assert!(
            specs
                .iter()
                .all(|rule| rule.category == Category::Dependencies)
        );
        assert!(!specs.is_empty());
    }

    #[test]
    fn invalid_rule_pattern_fails() {
        let text = "[[rules]]\nname = \"x\"\npattern = \"[\"\ncategory = \"build\"\n";
        assert!(matches!(parse(text), Err(ConfigError::Rule(_))));
    }

    #[test]
    fn defaults_round_trip() {
        let text = Config::default().to_toml();
        assert_eq!(parse(&text).expect("parses"), Config::default());
    }
}
