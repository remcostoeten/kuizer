use std::path::{Path, PathBuf};

use kuizer::config::Config;
use kuizer::detect::Category;
use kuizer::platform;
use kuizer::units::ByteSize;
use serde::{Deserialize, Serialize};

const RECENT_LIMIT: usize = 8;

pub const SIZE_STEPS: [u64; 4] = [0, 1_000_000, 10_000_000, 100_000_000];
pub const DEPTH_STEPS: [Option<usize>; 4] = [None, Some(3), Some(5), Some(8)];

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
    pub fn from_config(config: &Config) -> Self {
        Self {
            categories: config.detect.categories.clone(),
            min_size: config.scan.min_size.0,
            max_depth: config.scan.max_depth,
            caches: true,
            toolchains: true,
            docker: true,
            repos: true,
            duplicates: true,
            one_file_system: config.scan.one_file_system,
        }
    }

    /// @name ScanOptions::apply
    /// @description Returns a copy of the config with these options laid over it, the same way
    /// `kuizer scan --category` and `--min-size` override the config file for one run.
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

/// @name expand
/// @description Turns what was typed into the folder field into a path: trims it, expands a
/// leading `~` to the home directory and returns `None` for an empty field.
///
/// @example
/// assert_eq!(expand(" /tmp "), Some(PathBuf::from("/tmp")));
pub fn expand(typed: &str) -> Option<PathBuf> {
    let typed = typed.trim();
    if typed.is_empty() {
        return None;
    }
    let home = platform::home_dir();
    match (typed.strip_prefix('~'), home) {
        (Some(""), Some(home)) => Some(home),
        (Some(rest), Some(home)) if rest.starts_with('/') => {
            Some(home.join(rest.trim_start_matches('/')))
        }
        _ => Some(PathBuf::from(typed)),
    }
}

/// @name check_folder
/// @description Confirms a path is an existing folder before a scan starts, so the start screen
/// can say what is wrong instead of failing halfway through.
///
/// @example
/// let root = check_folder(&path)?;
pub fn check_folder(path: &Path) -> Result<PathBuf, String> {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(path.to_path_buf()),
        Ok(_) => Err("This is a file, pick a folder".to_owned()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err("This folder does not exist".to_owned())
        }
        Err(error) => Err(format!("Cannot open this folder: {error}")),
    }
}

/// @name Recent
/// @description The folders scanned most recently, newest first, capped at eight and kept in a
/// plain text file so the start screen can offer them again next time.
///
/// @example
/// let mut recent = Recent::for_user();
/// recent.remember(&roots);
pub struct Recent {
    file: Option<PathBuf>,
    pub paths: Vec<PathBuf>,
}

impl Recent {
    pub fn for_user() -> Self {
        Self::load(
            dirs::state_dir()
                .or_else(dirs::data_local_dir)
                .map(|dir| dir.join("kuizer").join("gui-recent")),
        )
    }

    pub fn load(file: Option<PathBuf>) -> Self {
        let paths = file
            .as_deref()
            .and_then(|file| std::fs::read_to_string(file).ok())
            .map(|text| parse_recent(&text))
            .unwrap_or_default();
        Self { file, paths }
    }

    pub fn remember(&mut self, roots: &[PathBuf]) {
        remember(&mut self.paths, roots);
        self.save();
    }

    fn save(&self) {
        let Some(file) = &self.file else {
            return;
        };
        if let Some(parent) = file.parent()
            && std::fs::create_dir_all(parent).is_err()
        {
            return;
        }
        let text: String = self
            .paths
            .iter()
            .filter_map(|path| path.to_str())
            .map(|path| format!("{path}\n"))
            .collect();
        if let Err(error) = std::fs::write(file, text) {
            eprintln!("kuizer-gui: cannot save recent folders: {error}");
        }
    }
}

fn parse_recent(text: &str) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let path = PathBuf::from(line);
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths.truncate(RECENT_LIMIT);
    paths
}

fn remember(recent: &mut Vec<PathBuf>, roots: &[PathBuf]) {
    recent.retain(|path| !roots.contains(path));
    for root in roots.iter().rev() {
        recent.insert(0, root.clone());
    }
    recent.truncate(RECENT_LIMIT);
}

pub fn size_label(bytes: u64) -> String {
    if bytes == 0 {
        "Any size".to_owned()
    } else {
        format!("{} MB", bytes / 1_000_000)
    }
}

pub fn depth_label(depth: Option<usize>) -> String {
    depth.map_or_else(|| "Unlimited".to_owned(), |depth| format!("{depth} levels"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_trims_and_handles_home() {
        assert_eq!(expand("   "), None);
        assert_eq!(expand(" /tmp/x "), Some(PathBuf::from("/tmp/x")));
        if let Some(home) = platform::home_dir() {
            assert_eq!(expand("~"), Some(home.clone()));
            assert_eq!(expand("~/apps"), Some(home.join("apps")));
        }
        assert_eq!(expand("~other"), Some(PathBuf::from("~other")));
    }

    #[test]
    fn remember_moves_roots_to_the_front() {
        let mut recent = vec![PathBuf::from("/a"), PathBuf::from("/b")];
        remember(&mut recent, &[PathBuf::from("/b")]);
        assert_eq!(recent, vec![PathBuf::from("/b"), PathBuf::from("/a")]);
        for index in 0..20 {
            remember(&mut recent, &[PathBuf::from(format!("/n{index}"))]);
        }
        assert_eq!(recent.len(), RECENT_LIMIT);
        assert_eq!(recent[0], PathBuf::from("/n19"));
    }

    #[test]
    fn parse_recent_skips_blanks_and_duplicates() {
        let parsed = parse_recent("/a\n\n /b \n/a\n");
        assert_eq!(parsed, vec![PathBuf::from("/a"), PathBuf::from("/b")]);
    }

    #[test]
    fn options_override_the_config() {
        let config = Config::default();
        let mut options = ScanOptions::from_config(&config);
        assert_eq!(options.categories.len(), Category::ALL.len());
        options.toggle_category(Category::Temp);
        options.min_size = 10_000_000;
        options.max_depth = Some(3);
        let applied = options.apply(&config);
        assert_eq!(applied.detect.categories.len(), 3);
        assert!(!applied.detect.categories.contains(&Category::Temp));
        assert_eq!(applied.scan.min_size, ByteSize(10_000_000));
        assert_eq!(applied.scan.max_depth, Some(3));
    }

    #[test]
    fn the_last_category_cannot_be_turned_off() {
        let mut options = ScanOptions::from_config(&Config::default());
        for category in Category::ALL {
            options.toggle_category(category);
        }
        assert_eq!(options.categories.len(), 1);
    }

    #[test]
    fn check_folder_rejects_files_and_missing_paths() {
        let dir = std::env::temp_dir();
        assert!(check_folder(&dir).is_ok());
        assert!(check_folder(&dir.join("kuizer-gui-missing-folder")).is_err());
    }
}
