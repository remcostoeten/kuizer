mod edit;
mod exclude;
mod fix;
mod lenient;
mod policy;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::detect::{Category, Detector, Listing, RuleError, RuleSpec, builtin_rules};
use crate::keymap::{KeyError, Keymap, Overrides};
use crate::platform;
use crate::toolchains::Tool;
use crate::units::ByteSize;

pub use edit::{
    EditError, SettingValue, add_exclude, check_setting, exclude_pattern, has_setting,
    home_relative, set_keys, set_protected_paths, set_setting,
};
pub use exclude::{ExcludeError, Excludes};
pub use fix::{ConfigChange, FixReport, fix_file, fix_text};
pub use lenient::{ConfigWarning, rejected_scope};
pub use policy::{EXAMPLE_POLICY, Policy, PolicySelect, SourceKind, known_rules};

pub const LEGACY_IGNORE_FILE: &str = ".sweepignore";
pub const MAX_SIZE_THREADS: usize = 64;
pub const MIN_ZOOM: u16 = 50;
pub const MAX_ZOOM: u16 = 200;
pub const DEFAULT_ZOOM: u16 = 100;

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
    pub route_shortcuts: RouteShortcutsConfig,
    pub keys: Overrides,
    pub toolchains: ToolchainConfig,
    pub docker: DockerConfig,
    pub policy: BTreeMap<String, Policy>,
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
    Permanent,
    Trash,
}

impl DeleteMode {
    pub const fn key(self) -> &'static str {
        match self {
            Self::Permanent => "permanent",
            Self::Trash => "trash",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Permanent => "Permanent",
            Self::Trash => "Trash",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DeleteConfig {
    pub mode: DeleteMode,
    pub confirm: bool,
    pub protected: Vec<PathBuf>,
    pub cross_device: bool,
}

impl DeleteConfig {
    /// @name DeleteConfig::cautious
    /// @description The deletion settings used when the config file cannot be trusted: move to
    /// the trash and always ask, because the file may have asked for exactly that.
    ///
    /// @example
    /// config.delete = DeleteConfig::cautious();
    pub fn cautious() -> Self {
        Self {
            mode: DeleteMode::Trash,
            confirm: true,
            protected: Vec::new(),
            cross_device: false,
        }
    }
}

impl Default for DeleteConfig {
    fn default() -> Self {
        Self {
            mode: DeleteMode::default(),
            confirm: true,
            protected: Vec::new(),
            cross_device: false,
        }
    }
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
    pub theme: Theme,
    pub accent: Accent,
    pub density: Density,
    pub zoom: u16,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            motion: true,
            sidebar: Sidebar::default(),
            theme: Theme::default(),
            accent: Accent::default(),
            density: Density::default(),
            zoom: DEFAULT_ZOOM,
        }
    }
}

/// The GUI keys that open a sidebar section: whether they work, and whether the sidebar shows
/// them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RouteShortcutsConfig {
    pub enabled: bool,
    pub hints: bool,
}

impl Default for RouteShortcutsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            hints: true,
        }
    }
}

/// The colors the GUI draws with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

impl Theme {
    pub const ALL: [Self; 2] = [Self::Dark, Self::Light];

    pub const fn key(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Light => "Light",
        }
    }
}

/// The color the GUI marks the selection, the focus and its bars with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Accent {
    #[default]
    Purple,
    Blue,
    Teal,
    Pink,
}

impl Accent {
    pub const ALL: [Self; 4] = [Self::Purple, Self::Blue, Self::Teal, Self::Pink];

    /// @name Accent::key
    /// @description The value of `appearance.accent` that selects this color.
    ///
    /// @example
    /// assert_eq!(Accent::Teal.key(), "teal");
    pub const fn key(self) -> &'static str {
        match self {
            Self::Purple => "purple",
            Self::Blue => "blue",
            Self::Teal => "teal",
            Self::Pink => "pink",
        }
    }

    /// Human-readable label shown in the settings UI.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Purple => "Purple",
            Self::Blue => "Blue",
            Self::Teal => "Teal",
            Self::Pink => "Pink",
        }
    }
}

/// How tightly the GUI packs rows and padding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Density {
    #[default]
    Comfortable,
    Compact,
}

impl Density {
    pub const ALL: [Self; 2] = [Self::Comfortable, Self::Compact];

    pub const fn key(self) -> &'static str {
        match self {
            Self::Comfortable => "comfortable",
            Self::Compact => "compact",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Comfortable => "Comfortable",
            Self::Compact => "Compact",
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

impl Sidebar {
    pub const fn key(self) -> &'static str {
        match self {
            Self::Expanded => "expanded",
            Self::Collapsed => "collapsed",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Expanded => "Expanded",
            Self::Collapsed => "Collapsed",
        }
    }
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
    #[error(transparent)]
    Key(#[from] KeyError),
    #[error("unknown rule `{0}` in detect.disabled_rules")]
    UnknownDisabled(String),
    #[error("appearance.zoom must be between {MIN_ZOOM} and {MAX_ZOOM}, not {0}")]
    Zoom(u16),
    #[error("policy.{policy}.{field}: {message}")]
    Policy {
        policy: String,
        field: String,
        message: String,
    },
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
                let config = Self {
                    delete: DeleteConfig::cautious(),
                    ..Self::default()
                };
                return Ok(Loaded {
                    config,
                    warnings: vec![ConfigWarning::new(
                        "",
                        format!("cannot read {}: {source}", path.display()),
                    )],
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
        for path in &mut self.delete.protected {
            *path = expand_home(path);
        }
        for extension in &mut self.scan.exclude_extensions {
            *extension = extension.trim_start_matches('.').to_ascii_lowercase();
        }
        for policy in self.policy.values_mut() {
            policy.normalize();
        }
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        self.detector()?;
        Excludes::new(&self.scan.exclude, &self.scan.exclude_extensions)?;
        if let Some(error) = Keymap::resolve(&self.keys).1.into_iter().next() {
            return Err(error.into());
        }
        if !(MIN_ZOOM..=MAX_ZOOM).contains(&self.appearance.zoom) {
            return Err(ConfigError::Zoom(self.appearance.zoom));
        }
        for (name, policy) in &self.policy {
            policy::validate_policy(self, name, policy)?;
        }
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

    /// @name usable_excludes
    /// @description Returns the excludes built from every valid pattern. Loading the config
    /// reports the invalid ones.
    ///
    /// @example
    /// let excludes = config.usable_excludes();
    pub fn usable_excludes(&self) -> Excludes {
        Excludes::usable(&self.scan.exclude, &self.scan.exclude_extensions)
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }

    /// @name init_toml
    /// @description Renders the starter config written by `kuizer config init`, including a
    /// commented example policy.
    ///
    /// @example
    /// let text = Config::default().init_toml();
    pub fn init_toml(&self) -> String {
        format!("{}{}", self.to_toml(), EXAMPLE_POLICY)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::detect::{Kind, Safety};

    fn parse(text: &str) -> Result<Config, ConfigError> {
        Config::parse(text, Path::new("test.toml"))
    }

    fn collect_json_keys(value: &serde_json::Value, prefix: &str, out: &mut BTreeSet<String>) {
        match value {
            serde_json::Value::Object(map) => {
                for (key, child) in map {
                    let path = if prefix.is_empty() {
                        key.clone()
                    } else {
                        format!("{prefix}.{key}")
                    };
                    collect_json_keys(child, &path, out);
                }
            }
            serde_json::Value::Array(items) if items.iter().any(serde_json::Value::is_object) => {
                for item in items {
                    collect_json_keys(item, prefix, out);
                }
            }
            _ => {
                if !prefix.is_empty() {
                    out.insert(prefix.to_owned());
                }
            }
        }
    }

    fn collect_toml_keys(value: &toml::Value, prefix: &str, out: &mut BTreeSet<String>) {
        match value {
            toml::Value::Table(table) => {
                for (key, child) in table {
                    let path = if prefix.is_empty() {
                        key.clone()
                    } else {
                        format!("{prefix}.{key}")
                    };
                    collect_toml_keys(child, &path, out);
                }
            }
            toml::Value::Array(items) if items.iter().any(toml::Value::is_table) => {
                for item in items {
                    collect_toml_keys(item, prefix, out);
                }
            }
            _ => {
                if !prefix.is_empty() {
                    out.insert(prefix.to_owned());
                }
            }
        }
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
        assert_eq!(config.scan.max_depth, Some(16));
        assert_eq!(
            config.scan.excluded_fs_types,
            ScanConfig::default().excluded_fs_types
        );
        assert_eq!(config.rules.len(), 1);
        assert_eq!(config.rules[0].safety, Safety::Review);
        assert!(config.delete.confirm);
        assert_eq!(config.delete.mode, DeleteMode::Permanent);
        assert_eq!(config.git.min_gain, ByteSize(1024 * 1024));
        assert!(!config.git.aggressive);
        assert!(config.appearance.motion);
        assert!(!config.duplicates.enabled);
    }

    #[test]
    fn example_config_covers_every_schema_key() {
        let schema = Config {
            scan: ScanConfig {
                max_depth: Some(16),
                ..ScanConfig::default()
            },
            rules: vec![RuleSpec {
                name: "example".to_owned(),
                kind: Kind::Dir,
                pattern: "_site".to_owned(),
                markers: Vec::new(),
                contains: Vec::new(),
                category: Category::Build,
                safety: Safety::Review,
                ecosystem: "custom".to_owned(),
            }],
            ..Config::default()
        };
        let schema_value = serde_json::to_value(&schema).expect("serializes");
        let mut schema_keys = BTreeSet::new();
        collect_json_keys(&schema_value, "", &mut schema_keys);

        let example_table: toml::Table = include_str!("../../docs/config.example.toml")
            .parse()
            .expect("parses");
        let mut example_keys = BTreeSet::new();
        collect_toml_keys(&toml::Value::Table(example_table), "", &mut example_keys);

        let missing: Vec<&String> = schema_keys.difference(&example_keys).collect();
        let extra: Vec<&String> = example_keys.difference(&schema_keys).collect();
        assert!(
            missing.is_empty() && extra.is_empty(),
            "docs/config.example.toml does not match Config schema\nmissing: {missing:?}\nextra: {extra:?}"
        );
    }

    #[test]
    fn docker_timeout_has_one_second_floor() {
        let config = parse("[docker]\ntimeout_secs = 0\n").expect("parses");
        assert_eq!(config.docker.timeout(), std::time::Duration::from_secs(1));
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
