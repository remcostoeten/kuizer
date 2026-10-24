mod edit;
mod exclude;
mod lenient;

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::detect::{Category, Detector, Listing, RuleError, RuleSpec, builtin_rules};
use crate::platform;
use crate::toolchains::Tool;
use crate::units::ByteSize;

pub use edit::{EditError, add_exclude, exclude_pattern};
pub use exclude::{ExcludeError, Excludes};
pub use lenient::ConfigWarning;

pub const LEGACY_IGNORE_FILE: &str = ".sweepignore";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub scan: ScanConfig,
    pub detect: DetectConfig,
    pub rules: Vec<RuleSpec>,
    pub duplicates: DuplicateConfig,
    pub delete: DeleteConfig,
    pub git: GitConfig,
    pub appearance: AppearanceConfig,
    pub toolchains: ToolchainConfig,
    pub docker: DockerConfig,
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
    /// if config.scan.ignores(&detector, &listing) {
    ///     return;
    /// }
    pub fn ignores(&self, detector: &Detector, listing: &Listing) -> bool {
        detector.lists(listing, &self.ignore_file) || detector.lists(listing, LEGACY_IGNORE_FILE)
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
    pub enabled: bool,
    pub min_size: ByteSize,
    pub threads: usize,
    pub verify: bool,
    pub include_hidden: bool,
}

impl Default for DuplicateConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            min_size: ByteSize(4096),
            threads: 0,
            verify: false,
            include_hidden: false,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GitConfig {
    pub min_gain: ByteSize,
    pub aggressive: bool,
    pub keep_branches: Vec<String>,
    pub threads: usize,
}

impl Default for GitConfig {
    fn default() -> Self {
        Self {
            min_gain: ByteSize(1024 * 1024),
            aggressive: false,
            keep_branches: ["main", "master", "develop", "trunk"]
                .map(String::from)
                .to_vec(),
            threads: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppearanceConfig {
    pub motion: bool,
    pub sidebar: Sidebar,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            motion: true,
            sidebar: Sidebar::default(),
        }
    }
}

/// How wide the GUI sidebar opens: with labels, or as a narrow strip of icons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Sidebar {
    #[default]
    Expanded,
    Collapsed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ToolchainConfig {
    pub tools: Vec<Tool>,
    pub pins: bool,
}

impl Default for ToolchainConfig {
    fn default() -> Self {
        Self {
            tools: Tool::ALL.to_vec(),
            pins: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DockerConfig {
    pub engines: Vec<String>,
    pub timeout_secs: u64,
    pub recent_hours: u64,
}

impl Default for DockerConfig {
    fn default() -> Self {
        Self {
            engines: ["docker", "podman"].map(String::from).to_vec(),
            timeout_secs: 10,
            recent_hours: 24,
        }
    }
}

impl DockerConfig {
    pub fn timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.timeout_secs.max(1))
    }
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

#[derive(Default)]
pub struct Loaded {
    pub config: Config,
    pub source: Option<PathBuf>,
    pub warnings: Vec<ConfigWarning>,
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

    /// @name Config::load
    /// @description Loads the config file, or `explicit` when given. A file that cannot be
    /// parsed or holds invalid settings never fails the load: the bad settings fall back to their
    /// defaults and come back as `warnings`. Only an explicitly requested file that cannot be
    /// read is an error.
    ///
    /// @example
    /// let loaded = Config::load(None)?;
    /// for warning in &loaded.warnings {
    ///     eprintln!("config: {warning}");
    /// }
    pub fn load(explicit: Option<&Path>) -> Result<Loaded, ConfigError> {
        let path = match explicit {
            Some(path) => Some(expand_home(path)),
            None => default_path().filter(|path| path.is_file()),
        };
        let Some(path) = path else {
            return Ok(Loaded::default());
        };
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(source) if explicit.is_some() => {
                return Err(ConfigError::Read { path, source });
            }
            Err(source) => {
                let mut config = Self::default();
                config.delete.confirm = true;
                return Ok(Loaded {
                    config,
                    warnings: vec![ConfigWarning {
                        key: String::new(),
                        message: format!("cannot read {}: {source}", path.display()),
                    }],
                    source: Some(path),
                });
            }
        };
        let (config, warnings) = Self::parse_lenient(&text);
        Ok(Loaded {
            config,
            source: Some(path),
            warnings,
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
        Ok(Detector::new(&self.enabled_rules()?)?
            .watching([self.scan.ignore_file.as_str(), LEGACY_IGNORE_FILE])
            .reporting(self.detect.categories.clone()))
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
        assert_eq!(config.git.min_gain, ByteSize(1024 * 1024));
        assert!(!config.git.aggressive);
        assert!(config.appearance.motion);
        assert!(!config.duplicates.enabled);
    }

    #[test]
    fn toolchain_tools_and_pins_are_configurable() {
        let defaults = Config::default();
        assert_eq!(defaults.toolchains.tools, Tool::ALL);
        assert!(defaults.toolchains.pins);
        let config =
            parse("[toolchains]\ntools = [\"nvm\", \"go\"]\npins = false\n").expect("parses");
        assert_eq!(config.toolchains.tools, [Tool::Nvm, Tool::Go]);
        assert!(!config.toolchains.pins);
        assert!(matches!(
            parse("[toolchains]\ntools = [\"asdf\"]\n"),
            Err(ConfigError::Parse { .. })
        ));
    }

    #[test]
    fn docker_engines_and_timeouts_are_configurable() {
        let defaults = Config::default().docker;
        assert_eq!(defaults.engines, ["docker", "podman"]);
        assert_eq!(defaults.timeout(), std::time::Duration::from_secs(10));
        let config =
            parse("[docker]\nengines = [\"podman\"]\ntimeout_secs = 0\nrecent_hours = 2\n")
                .expect("parses");
        assert_eq!(config.docker.engines, ["podman"]);
        assert_eq!(config.docker.timeout(), std::time::Duration::from_secs(1));
        assert_eq!(config.docker.recent_hours, 2);
    }

    #[test]
    fn motion_can_be_turned_off() {
        let config = parse("[appearance]\nmotion = false\n").expect("parses");
        assert!(!config.appearance.motion);
        assert!(Config::default().appearance.motion);
    }

    #[test]
    fn sidebar_opens_expanded_unless_collapsed() {
        let config = parse("[appearance]\nsidebar = \"collapsed\"\n").expect("parses");
        assert_eq!(config.appearance.sidebar, Sidebar::Collapsed);
        assert_eq!(Config::default().appearance.sidebar, Sidebar::Expanded);
        assert!(parse("[appearance]\nsidebar = \"wide\"\n").is_err());
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
