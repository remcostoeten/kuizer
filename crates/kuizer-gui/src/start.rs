use std::collections::HashSet;
use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use kuizer::config::Config;
use kuizer::output::display_path;
use kuizer::platform;
pub use kuizer::scan::ScanOptions;

use crate::persist;

const RECENT_LIMIT: usize = 8;
const NUL_HEX_PREFIX: char = '\0';

pub const SIZE_STEPS: [u64; 4] = [0, 1_000_000, 10_000_000, 100_000_000];
pub const DEPTH_STEPS: [Option<usize>; 4] = [None, Some(3), Some(5), Some(8)];

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
    gone: HashSet<PathBuf>,
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
        Self {
            file,
            paths,
            gone: HashSet::new(),
        }
    }

    pub fn remember(&mut self, roots: &[PathBuf]) {
        remember(&mut self.paths, roots);
        self.save();
    }

    fn save(&self) {
        let Some(file) = &self.file else {
            return;
        };
        let text: String = self.paths.iter().map(|path| recent_line(path)).collect();
        persist::save(file.clone(), text);
    }

    /// @name Recent::probe
    /// @description Returns the folders among `paths` that no longer exist. It touches the disk,
    /// so run it off the interface thread: a stale network mount can block for a long time.
    ///
    /// @example
    /// let gone = Recent::probe(&recent.paths);
    pub fn probe(paths: &[PathBuf]) -> Vec<PathBuf> {
        paths
            .iter()
            .filter(|path| !path.is_dir())
            .cloned()
            .collect()
    }

    pub fn mark_gone(&mut self, gone: Vec<PathBuf>) {
        self.gone = gone.into_iter().collect();
    }

    pub fn is_gone(&self, path: &Path) -> bool {
        self.gone.contains(path)
    }

    pub fn default_folder(&self, config: &Config, roots: &[PathBuf]) -> String {
        self.paths
            .iter()
            .find(|path| !self.is_gone(path))
            .or(config.scan.roots.first())
            .or(roots.first())
            .map(|path| display_path(path))
            .unwrap_or_default()
    }
}

fn parse_recent(text: &str) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let Some(path) = recent_path(line) else {
            continue;
        };
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths.truncate(RECENT_LIMIT);
    paths
}

fn recent_line(path: &Path) -> String {
    match path.to_str() {
        Some(text) if text.trim() == text && !text.contains('\n') => format!("{text}\n"),
        _ => {
            let hex: String = path
                .as_os_str()
                .as_bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            format!("{NUL_HEX_PREFIX}{hex}\n")
        }
    }
}

fn recent_path(line: &str) -> Option<PathBuf> {
    let Some(hex) = line.strip_prefix(NUL_HEX_PREFIX) else {
        return Some(PathBuf::from(line));
    };
    let (digits, rest) = hex.as_bytes().as_chunks::<2>();
    if !rest.is_empty() {
        return None;
    }
    let bytes = digits
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok())
        .collect::<Option<Vec<u8>>>()?;
    if bytes.is_empty() || bytes.contains(&0) {
        return None;
    }
    Some(PathBuf::from(OsString::from_vec(bytes)))
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
    use kuizer::detect::Category;
    use kuizer::units::ByteSize;

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
    fn recent_keeps_non_utf8_paths_beside_plain_lines() {
        let odd = PathBuf::from(OsString::from_vec(b"/tmp/caf\xE9".to_vec()));
        let text = [Path::new("/plain dir"), odd.as_path()]
            .map(recent_line)
            .concat();
        assert!(text.starts_with("/plain dir\n"));
        assert_eq!(parse_recent(&text), [PathBuf::from("/plain dir"), odd]);
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
