use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use toml_edit::{Array, DocumentMut, Item, Table, Value};

use super::{ConfigError, Excludes};
use crate::platform;

#[derive(Debug, thiserror::Error)]
pub enum EditError {
    #[error("cannot read config {path}: {source}")]
    Read { path: PathBuf, source: io::Error },
    #[error("cannot write config {path}: {source}")]
    Write { path: PathBuf, source: io::Error },
    #[error("invalid config {path}: {message}")]
    Parse { path: PathBuf, message: String },
    #[error("cannot add the exclude to {path}: {source}")]
    Invalid {
        path: PathBuf,
        source: Box<ConfigError>,
    },
}

/// @name exclude_pattern
/// @description Turns a path into a `scan.exclude` pattern that matches exactly that path: glob
/// characters in it are escaped, and a path under the home folder is written as `~/...` so the
/// config stays readable and keeps working when the home folder moves.
///
/// @example
/// assert_eq!(exclude_pattern(Path::new("/p/a[1]")), "/p/a\\[1\\]");
pub fn exclude_pattern(path: &Path) -> String {
    let shown = match platform::home_dir().and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) if !rest.as_os_str().is_empty() => Path::new("~").join(rest),
        _ => path.to_path_buf(),
    };
    let mut pattern = String::new();
    for ch in shown.to_string_lossy().chars() {
        if matches!(ch, '\\' | '*' | '?' | '[' | ']' | '{' | '}') {
            pattern.push('\\');
        }
        pattern.push(ch);
    }
    pattern
}

/// @name add_exclude
/// @description Appends a pattern to `scan.exclude` in the config file at `file`, creating the
/// file and the `[scan]` table when they are missing. Comments and layout of the rest of the file
/// are kept, and only the new pattern is checked before it is written, so the rest of the file may
/// hold settings that do not validate. Returns `false` when the pattern was already there.
///
/// @example
/// add_exclude(&config_path, &exclude_pattern(Path::new("/p/site/node_modules")))?;
pub fn add_exclude(file: &Path, pattern: &str) -> Result<bool, EditError> {
    let text = match fs::read_to_string(file) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(source) => {
            return Err(EditError::Read {
                path: file.to_path_buf(),
                source,
            });
        }
    };
    let parse_error = |message: String| EditError::Parse {
        path: file.to_path_buf(),
        message,
    };
    let mut document: DocumentMut = text
        .parse()
        .map_err(|error: toml_edit::TomlError| parse_error(error.to_string()))?;
    let scan = document
        .entry("scan")
        .or_insert_with(|| Item::Table(Table::new()))
        .as_table_like_mut()
        .ok_or_else(|| parse_error("`scan` is not a table".to_owned()))?;
    let exclude = scan
        .entry("exclude")
        .or_insert(Item::Value(Value::Array(Array::new())))
        .as_array_mut()
        .ok_or_else(|| parse_error("`scan.exclude` is not a list".to_owned()))?;
    if exclude.iter().any(|value| value.as_str() == Some(pattern)) {
        return Ok(false);
    }
    Excludes::new(&[pattern.to_owned()], &[]).map_err(|source| EditError::Invalid {
        path: file.to_path_buf(),
        source: Box::new(ConfigError::Exclude(source)),
    })?;
    exclude.push(pattern);
    let updated = document.to_string();
    let write_error = |source| EditError::Write {
        path: file.to_path_buf(),
        source,
    };
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).map_err(write_error)?;
    }
    let partial = file.with_extension("toml.partial");
    fs::write(&partial, updated).map_err(write_error)?;
    fs::rename(&partial, file).map_err(write_error)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn patterns_escape_glob_characters_and_match_only_that_path() {
        let path = Path::new("/work/app [old]/node_modules");
        let pattern = exclude_pattern(path);
        assert_eq!(pattern, "/work/app \\[old\\]/node_modules");
        let excludes = super::super::Excludes::new(&[pattern], &[]).expect("valid");
        assert!(excludes.path(path));
        assert!(!excludes.path(Path::new("/work/app o/node_modules")));
        assert!(!excludes.path(Path::new("/work/app [old]/dist")));
    }

    #[test]
    fn adds_the_pattern_once_and_keeps_the_rest_of_the_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("kuizer").join("config.toml");
        assert!(add_exclude(&file, "/p/a/dist").expect("creates the file"));
        let config = Config::load(Some(&file)).expect("loads").config;
        assert_eq!(config.scan.exclude, ["/p/a/dist"]);

        let text = "# my settings\n[scan]\nmin_size = \"1 MB\" # keep small\nexclude = [\"vendor\"]\n\n[appearance]\nmotion = false\n";
        fs::write(&file, text).expect("write");
        assert!(add_exclude(&file, "/p/b/node_modules").expect("adds"));
        assert!(!add_exclude(&file, "/p/b/node_modules").expect("already there"));
        let written = fs::read_to_string(&file).expect("read");
        assert!(written.starts_with("# my settings\n"));
        assert!(written.contains("# keep small"));
        let config = Config::parse(&written, &file).expect("still valid");
        assert_eq!(config.scan.exclude, ["vendor", "/p/b/node_modules"]);
        assert!(!config.appearance.motion);
        assert!(
            config
                .excludes()
                .expect("valid")
                .path(Path::new("/p/b/node_modules"))
        );
    }

    #[test]
    fn other_invalid_settings_do_not_block_adding_an_exclude() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("config.toml");
        fs::write(&file, "[appearance]\nmotion = \"no\"\n").expect("write");
        assert!(add_exclude(&file, "/p/a").expect("adds"));
        assert!(matches!(
            add_exclude(&file, "["),
            Err(EditError::Invalid { .. })
        ));
        let loaded = Config::load(Some(&file)).expect("loads");
        assert_eq!(loaded.config.scan.exclude, ["/p/a"]);
        assert_eq!(loaded.warnings.len(), 1);
    }

    #[test]
    fn a_broken_config_is_left_alone() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("config.toml");
        fs::write(&file, "[scan\n").expect("write");
        assert!(matches!(
            add_exclude(&file, "/p/a"),
            Err(EditError::Parse { .. })
        ));
        fs::write(&file, "scan = 3\n").expect("write");
        assert!(add_exclude(&file, "/p/a").is_err());
        assert_eq!(fs::read_to_string(&file).expect("read"), "scan = 3\n");
    }
}
