use std::ffi::OsStr;
use std::path::Path;

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

use super::expand_home;

#[derive(Debug, thiserror::Error)]
#[error("invalid exclude pattern `{pattern}`: {source}")]
pub struct ExcludeError {
    pattern: String,
    source: globset::Error,
}

pub struct Excludes {
    paths: GlobSet,
    names: GlobSet,
    extensions: Vec<String>,
}

fn build(patterns: &[String]) -> Result<GlobSet, ExcludeError> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let glob = GlobBuilder::new(pattern)
            .literal_separator(true)
            .backslash_escape(true)
            .build()
            .map_err(|source| ExcludeError {
                pattern: pattern.clone(),
                source,
            })?;
        builder.add(glob);
    }
    builder.build().map_err(|source| ExcludeError {
        pattern: patterns.join(", "),
        source,
    })
}

impl Excludes {
    pub fn new(patterns: &[String], extensions: &[String]) -> Result<Self, ExcludeError> {
        let (path_patterns, name_patterns): (Vec<String>, Vec<String>) = patterns
            .iter()
            .map(|pattern| {
                expand_home(Path::new(pattern))
                    .to_string_lossy()
                    .trim_end_matches('/')
                    .to_owned()
            })
            .partition(|pattern| pattern.contains('/'));
        Ok(Self {
            paths: build(&path_patterns)?,
            names: build(&name_patterns)?,
            extensions: extensions.to_vec(),
        })
    }

    pub fn path(&self, path: &Path) -> bool {
        path.file_name()
            .is_some_and(|name| self.names.is_match(name))
            || self.paths.is_match(path)
    }

    pub fn extension(&self, name: &OsStr) -> bool {
        !self.extensions.is_empty()
            && Path::new(name)
                .extension()
                .and_then(OsStr::to_str)
                .is_some_and(|ext| self.extensions.iter().any(|e| e.eq_ignore_ascii_case(ext)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn excludes(patterns: &[&str]) -> Excludes {
        let patterns: Vec<String> = patterns.iter().map(|p| (*p).to_owned()).collect();
        Excludes::new(&patterns, &["log".to_owned()]).expect("valid")
    }

    #[test]
    fn name_patterns_match_any_depth() {
        let set = excludes(&["vendor", "*.bak"]);
        assert!(set.path(Path::new("/a/b/vendor")));
        assert!(set.path(Path::new("/a/old.bak")));
        assert!(!set.path(Path::new("/a/vendored")));
    }

    #[test]
    fn path_patterns_respect_separators() {
        let set = excludes(&["/work/*/keep", "/data/**"]);
        assert!(set.path(Path::new("/work/app/keep")));
        assert!(!set.path(Path::new("/work/a/b/keep")));
        assert!(set.path(Path::new("/data/x/y")));
    }

    #[test]
    fn extensions_ignore_case() {
        let set = excludes(&[]);
        assert!(set.extension(OsStr::new("Server.LOG")));
        assert!(!set.extension(OsStr::new("log")));
    }
}
