use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use globset::{Glob, GlobMatcher};

use super::{CACHEDIR_SIGNATURE, CACHEDIR_TAG, Category, Kind, RuleSpec, Safety};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleInfo {
    pub name: String,
    pub category: Category,
    pub safety: Safety,
    pub ecosystem: String,
}

pub type Match = Arc<RuleInfo>;

#[derive(Debug, thiserror::Error)]
pub enum RuleError {
    #[error("rule `{name}` has an invalid pattern `{pattern}`: {source}")]
    Pattern {
        name: String,
        pattern: String,
        source: globset::Error,
    },
    #[error("rule `{0}` is defined more than once")]
    Duplicate(String),
    #[error("rule `{name}`: `{value}` must be a plain file name, not a path")]
    NotAName { name: String, value: String },
}

enum Marker {
    Name(OsString),
    Glob(GlobMatcher),
}

struct Compiled {
    info: Match,
    markers: Vec<Marker>,
    contains: Vec<OsString>,
}

#[derive(Default)]
struct Index {
    literal: HashMap<OsString, Vec<usize>>,
    globs: Vec<(usize, GlobMatcher)>,
}

impl Index {
    fn candidates(&self, name: &OsStr) -> Vec<usize> {
        let mut found: Vec<usize> = self.literal.get(name).cloned().unwrap_or_default();
        found.extend(
            self.globs
                .iter()
                .filter(|(_, glob)| glob.is_match(name))
                .map(|(index, _)| *index),
        );
        found.sort_unstable();
        found
    }
}

pub struct Detector {
    rules: Vec<Compiled>,
    dirs: Index,
    files: Index,
    reported: Vec<Category>,
}

pub struct Listing {
    names: Vec<OsString>,
}

impl Listing {
    pub fn new(mut names: Vec<OsString>) -> Self {
        names.sort_unstable();
        Self { names }
    }

    pub fn has(&self, name: &OsStr) -> bool {
        self.names
            .binary_search_by(|probe| probe.as_os_str().cmp(name))
            .is_ok()
    }

    pub fn any(&self, glob: &GlobMatcher) -> bool {
        self.names.iter().any(|name| glob.is_match(name))
    }
}

fn is_plain_name(value: &str) -> bool {
    !value.is_empty() && value != "." && value != ".." && !value.contains(['/', '\\'])
}

fn has_glob_meta(pattern: &str) -> bool {
    pattern.contains(['*', '?', '[', ']', '{', '}'])
}

fn compile_glob(name: &str, pattern: &str) -> Result<GlobMatcher, RuleError> {
    Glob::new(pattern)
        .map(|glob| glob.compile_matcher())
        .map_err(|source| RuleError::Pattern {
            name: name.to_owned(),
            pattern: pattern.to_owned(),
            source,
        })
}

fn valid_cachedir_tag(dir: &Path) -> bool {
    let mut header = [0u8; CACHEDIR_SIGNATURE.len()];
    File::open(dir.join(CACHEDIR_TAG))
        .and_then(|mut file| file.read_exact(&mut header))
        .is_ok_and(|()| header == CACHEDIR_SIGNATURE)
}

impl Detector {
    pub fn new(specs: &[RuleSpec]) -> Result<Self, RuleError> {
        let mut rules = Vec::with_capacity(specs.len());
        let mut dirs = Index::default();
        let mut files = Index::default();
        let mut seen = HashSet::new();

        for spec in specs {
            if !seen.insert(spec.name.as_str()) {
                return Err(RuleError::Duplicate(spec.name.clone()));
            }
            for value in spec.markers.iter().chain(&spec.contains) {
                if !is_plain_name(value) {
                    return Err(RuleError::NotAName {
                        name: spec.name.clone(),
                        value: value.clone(),
                    });
                }
            }
            let index = rules.len();
            let target = match spec.kind {
                Kind::Dir => &mut dirs,
                Kind::File => &mut files,
            };
            if has_glob_meta(&spec.pattern) {
                target
                    .globs
                    .push((index, compile_glob(&spec.name, &spec.pattern)?));
            } else if is_plain_name(&spec.pattern) {
                target
                    .literal
                    .entry(OsString::from(&spec.pattern))
                    .or_default()
                    .push(index);
            } else {
                return Err(RuleError::NotAName {
                    name: spec.name.clone(),
                    value: spec.pattern.clone(),
                });
            }
            let markers = spec
                .markers
                .iter()
                .map(|marker| {
                    if has_glob_meta(marker) {
                        compile_glob(&spec.name, marker).map(Marker::Glob)
                    } else {
                        Ok(Marker::Name(OsString::from(marker)))
                    }
                })
                .collect::<Result<_, _>>()?;
            rules.push(Compiled {
                info: Arc::new(RuleInfo {
                    name: spec.name.clone(),
                    category: spec.category,
                    safety: spec.safety,
                    ecosystem: spec.ecosystem.clone(),
                }),
                markers,
                contains: spec.contains.iter().map(OsString::from).collect(),
            });
        }

        Ok(Self {
            rules,
            dirs,
            files,
            reported: Category::ALL.to_vec(),
        })
    }

    /// @name reporting
    /// @description Limits which categories are reported. Rules of other categories still
    /// match, so the walker keeps pruning at `node_modules` even when only build output is wanted.
    ///
    /// @example
    /// let detector = Detector::new(&specs)?.reporting(vec![Category::Build]);
    pub fn reporting(mut self, categories: Vec<Category>) -> Self {
        self.reported = categories;
        self
    }

    pub fn reports(&self, rule: &RuleInfo) -> bool {
        self.reported.contains(&rule.category)
    }

    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    fn markers_match(rule: &Compiled, parent: &Listing) -> bool {
        rule.markers.is_empty()
            || rule.markers.iter().any(|marker| match marker {
                Marker::Name(name) => parent.has(name),
                Marker::Glob(glob) => parent.any(glob),
            })
    }

    pub fn classify_dir(&self, path: &Path, parent: &Listing, own: &Listing) -> Option<Match> {
        let name = path.file_name()?;
        self.dirs.candidates(name).into_iter().find_map(|index| {
            let rule = &self.rules[index];
            let contained = rule.contains.is_empty()
                || rule.contains.iter().any(|entry| {
                    own.has(entry) && (entry != CACHEDIR_TAG || valid_cachedir_tag(path))
                });
            (contained && Self::markers_match(rule, parent)).then(|| Arc::clone(&rule.info))
        })
    }

    pub fn classify_file(&self, name: &OsStr, parent: &Listing) -> Option<Match> {
        self.files.candidates(name).into_iter().find_map(|index| {
            let rule = &self.rules[index];
            Self::markers_match(rule, parent).then(|| Arc::clone(&rule.info))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::builtin_rules;

    type Case = (
        &'static str,
        &'static [&'static str],
        &'static [&'static str],
        Option<&'static str>,
    );

    fn listing(names: &[&str]) -> Listing {
        Listing::new(names.iter().map(OsString::from).collect())
    }

    fn detector() -> Detector {
        Detector::new(&builtin_rules()).expect("builtin rules compile")
    }

    #[test]
    fn node_modules_needs_package_json() {
        let detector = detector();
        let path = Path::new("/p/node_modules");
        let own = listing(&["react"]);
        let hit = detector.classify_dir(path, &listing(&["package.json", "node_modules"]), &own);
        assert_eq!(hit.map(|m| m.name.clone()).as_deref(), Some("node-modules"));
        assert!(
            detector
                .classify_dir(path, &listing(&["node_modules"]), &own)
                .is_none()
        );
    }

    #[test]
    fn dist_without_project_is_ignored() {
        let detector = detector();
        let path = Path::new("/p/dist");
        let own = listing(&["index.js"]);
        assert!(
            detector
                .classify_dir(path, &listing(&["dist", "README.md"]), &own)
                .is_none()
        );
        let hit = detector
            .classify_dir(path, &listing(&["dist", "package.json"]), &own)
            .expect("dist next to package.json");
        assert_eq!(hit.safety, Safety::Review);
    }

    #[test]
    fn coverage_needs_report_contents() {
        let detector = detector();
        let path = Path::new("/p/coverage");
        let parent = listing(&["package.json", "coverage"]);
        assert!(
            detector
                .classify_dir(path, &parent, &listing(&["notes.md"]))
                .is_none()
        );
        assert!(
            detector
                .classify_dir(path, &parent, &listing(&["lcov.info"]))
                .is_some()
        );
    }

    #[test]
    fn cache_name_alone_is_not_enough() {
        let detector = detector();
        let path = Path::new("/home/me/.cache");
        let own = listing(&["fontconfig"]);
        assert!(
            detector
                .classify_dir(path, &listing(&[".cache"]), &own)
                .is_none()
        );
    }

    #[test]
    fn file_rules_match_globs() {
        let detector = detector();
        let parent = listing(&["tsconfig.json"]);
        assert!(
            detector
                .classify_file(OsStr::new("tsconfig.tsbuildinfo"), &parent)
                .is_some()
        );
        assert!(
            detector
                .classify_file(OsStr::new("index.ts"), &parent)
                .is_none()
        );
    }

    #[test]
    fn ecosystem_rules_need_their_evidence() {
        let detector = detector();
        let cases: &[Case] = &[
            (
                ".venv",
                &["pyproject.toml"],
                &["pyvenv.cfg", "lib"],
                Some("python-venv"),
            ),
            ("venv", &[], &["pyvenv.cfg", "bin"], Some("python-venv")),
            (".venv", &["pyproject.toml"], &["lib"], None),
            (".tox", &["tox.ini"], &["py312"], Some("tox-env")),
            (".tox", &["README.md"], &["py312"], None),
            (".nox", &["noxfile.py"], &["tests"], Some("nox-env")),
            (".nox", &["pyproject.toml"], &["tests"], None),
            ("bin", &["App.csproj"], &["Debug"], Some("dotnet-bin")),
            ("bin", &["App.fsproj"], &["Release"], Some("dotnet-bin")),
            ("bin", &["App.csproj"], &["deploy.sh"], None),
            ("bin", &["Makefile"], &["Debug"], None),
            (
                "obj",
                &["Lib.vbproj"],
                &["project.assets.json", "Debug"],
                Some("dotnet-obj"),
            ),
            ("obj", &["Lib.csproj"], &["notes"], None),
            ("obj", &["README.md"], &["project.assets.json"], None),
            (
                "Pods",
                &["Podfile", "Podfile.lock"],
                &["Manifest.lock", "Alamofire"],
                Some("cocoapods"),
            ),
            ("Pods", &["Podfile"], &["notes.md"], None),
            ("Pods", &["README.md"], &["Manifest.lock"], None),
            (
                "_build",
                &["mix.exs", "mix.lock"],
                &["dev"],
                Some("elixir-build"),
            ),
            ("_build", &["CMakeLists.txt"], &["dev"], None),
            ("deps", &["mix.exs"], &["phoenix"], Some("elixir-deps")),
            ("deps", &["Makefile"], &["phoenix"], None),
            (
                ".zig-cache",
                &["build.zig", "build.zig.zon"],
                &["o", "h"],
                Some("zig-cache"),
            ),
            ("zig-cache", &["build.zig"], &["o"], Some("zig-cache")),
            (".zig-cache", &["main.zig"], &["o"], None),
            ("zig-out", &["build.zig"], &["bin"], Some("zig-out")),
            ("zig-out", &["README.md"], &["bin"], None),
            (
                ".stack-work",
                &["stack.yaml", "app.cabal"],
                &["dist"],
                Some("haskell-stack"),
            ),
            (".stack-work", &["README.md"], &["dist"], None),
            (
                "dist-newstyle",
                &["cabal.project"],
                &["cache"],
                Some("haskell-cabal"),
            ),
            (
                "dist-newstyle",
                &["my-lib.cabal"],
                &["cache"],
                Some("haskell-cabal"),
            ),
            ("dist-newstyle", &["Setup.hs"], &["cache"], None),
            (
                ".build",
                &["Package.swift", "Package.resolved"],
                &["debug", "checkouts"],
                Some("swift-build"),
            ),
            (".build", &["Makefile"], &["debug"], None),
            (
                "target",
                &["pom.xml", "src"],
                &["classes", "maven-status"],
                Some("maven-target"),
            ),
            ("target", &["pom.xml"], &["deploy.yaml"], None),
            ("target", &["build.xml"], &["classes"], None),
            (
                "target",
                &["Cargo.toml", "pom.xml"],
                &[".rustc_info.json", "classes"],
                Some("cargo-target"),
            ),
            (
                ".terraform",
                &["main.tf", "variables.tf"],
                &["providers"],
                Some("terraform"),
            ),
            (".terraform", &["main.tf"], &["notes"], None),
            (".terraform", &["main.tf.bak"], &["providers"], None),
        ];
        for (dir, parent, own, expected) in cases {
            let hit =
                detector.classify_dir(&Path::new("/p").join(dir), &listing(parent), &listing(own));
            assert_eq!(
                hit.map(|m| m.name.clone()).as_deref(),
                *expected,
                "{dir} beside {parent:?} holding {own:?}"
            );
        }
    }

    #[test]
    fn custom_rules_win_over_builtins() {
        let mut specs = vec![RuleSpec {
            name: "keep-dist".to_owned(),
            kind: Kind::Dir,
            pattern: "dist".to_owned(),
            markers: vec!["package.json".to_owned()],
            contains: vec![],
            category: Category::Build,
            safety: Safety::Safe,
            ecosystem: "custom".to_owned(),
        }];
        specs.extend(builtin_rules());
        let detector = Detector::new(&specs).expect("compiles");
        let hit = detector
            .classify_dir(
                Path::new("/p/dist"),
                &listing(&["package.json"]),
                &listing(&[]),
            )
            .expect("matches");
        assert_eq!(hit.name, "keep-dist");
    }

    #[test]
    fn markers_can_be_globs() {
        let mut spec = builtin_rules().remove(0);
        spec.markers = vec!["*.csproj".to_owned()];
        let detector = Detector::new(&[spec]).expect("compiles");
        let path = Path::new("/p/node_modules");
        let own = listing(&[]);
        assert!(
            detector
                .classify_dir(path, &listing(&["App.csproj", "node_modules"]), &own)
                .is_some()
        );
        assert!(
            detector
                .classify_dir(path, &listing(&["App.csproj.user", "node_modules"]), &own)
                .is_none()
        );
    }

    #[test]
    fn rejects_bad_rules() {
        let mut spec = builtin_rules().remove(0);
        spec.markers = vec!["../package.json".to_owned()];
        assert!(matches!(
            Detector::new(&[spec]),
            Err(RuleError::NotAName { .. })
        ));
        let mut spec = builtin_rules().remove(0);
        spec.markers = vec!["*/x.csproj".to_owned()];
        assert!(matches!(
            Detector::new(&[spec]),
            Err(RuleError::NotAName { .. })
        ));
        let mut spec = builtin_rules().remove(0);
        spec.markers = vec!["[.csproj".to_owned()];
        assert!(matches!(
            Detector::new(&[spec]),
            Err(RuleError::Pattern { .. })
        ));
        let twice = vec![builtin_rules().remove(0), builtin_rules().remove(0)];
        assert!(matches!(
            Detector::new(&twice),
            Err(RuleError::Duplicate(_))
        ));
    }
}
