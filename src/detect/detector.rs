use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::fs::File;
use std::hash::{BuildHasherDefault, Hasher};
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
    #[error("rule `{0}`: kind must be dir or file")]
    External(String),
}

#[derive(Default)]
struct FxHasher(u64);

impl Hasher for FxHasher {
    fn write(&mut self, bytes: &[u8]) {
        const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;
        for chunk in bytes.chunks(8) {
            let mut word = [0u8; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            self.0 = (self.0.rotate_left(5) ^ u64::from_le_bytes(word)).wrapping_mul(SEED);
        }
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

type FxMap<K, V> = HashMap<K, V, BuildHasherDefault<FxHasher>>;

const INLINE_BITS: u32 = 256;

/// The evidence a directory listing holds: one bit per name or name pattern that some rule,
/// ignore file or protection marker asks about, filled in one pass over the entries.
#[derive(Debug, Clone, Default)]
pub struct Listing {
    words: [u64; (INLINE_BITS / 64) as usize],
    spill: Vec<u32>,
}

impl Listing {
    fn set(&mut self, bit: u32) {
        if bit < INLINE_BITS {
            self.words[(bit / 64) as usize] |= 1 << (bit % 64);
        } else if !self.spill.contains(&bit) {
            self.spill.push(bit);
        }
    }

    fn get(&self, bit: u32) -> bool {
        if bit < INLINE_BITS {
            self.words[(bit / 64) as usize] & (1 << (bit % 64)) != 0
        } else {
            self.spill.contains(&bit)
        }
    }

    fn intersects(&self, mask: &Listing) -> bool {
        self.words
            .iter()
            .zip(&mask.words)
            .any(|(own, wanted)| own & wanted != 0)
            || mask.spill.iter().any(|bit| self.spill.contains(bit))
    }

    fn is_empty(&self) -> bool {
        self.words.iter().all(|word| *word == 0) && self.spill.is_empty()
    }
}

enum Part {
    Literal(Box<[u8]>),
    Any,
    Prefix(Box<[u8]>),
    Suffix(Box<[u8]>),
    Glob(GlobMatcher),
}

impl Part {
    fn matches(&self, bytes: &[u8], name: &OsStr) -> bool {
        match self {
            Self::Literal(literal) => **literal == *bytes,
            Self::Any => true,
            Self::Prefix(prefix) => bytes.starts_with(prefix),
            Self::Suffix(suffix) => bytes.ends_with(suffix),
            Self::Glob(glob) => glob.is_match(name),
        }
    }
}

fn simple_part(pattern: &str) -> Option<Part> {
    if pattern.contains(['?', '[', ']', '{', '}', '\\']) {
        return None;
    }
    let bytes = pattern.as_bytes();
    match bytes.iter().filter(|byte| **byte == b'*').count() {
        0 => Some(Part::Literal(bytes.into())),
        1 if bytes == b"*" => Some(Part::Any),
        1 if bytes[0] == b'*' => Some(Part::Suffix(bytes[1..].into())),
        1 if bytes[bytes.len() - 1] == b'*' => Some(Part::Prefix(bytes[..bytes.len() - 1].into())),
        _ => None,
    }
}

fn parts(name: &str, pattern: &str) -> Result<Vec<Part>, RuleError> {
    let simple = match pattern
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
    {
        Some(inner) => inner.split(',').map(simple_part).collect(),
        None => simple_part(pattern).map(|part| vec![part]),
    };
    match simple {
        Some(parts) => Ok(parts),
        None => Ok(vec![Part::Glob(
            parse_glob(name, pattern)?.compile_matcher(),
        )]),
    }
}

#[derive(Default)]
struct Evidence {
    names: FxMap<Box<[u8]>, u32>,
    patterns: Vec<(u32, Part)>,
    pattern_bits: HashMap<String, Vec<u32>>,
    count: u32,
}

impl Evidence {
    fn next_bit(&mut self) -> u32 {
        self.count += 1;
        self.count - 1
    }

    fn literal(&mut self, name: &[u8]) -> u32 {
        if let Some(&bit) = self.names.get(name) {
            return bit;
        }
        let bit = self.next_bit();
        self.names.insert(name.into(), bit);
        bit
    }

    fn pattern(&mut self, rule: &str, pattern: &str) -> Result<Vec<u32>, RuleError> {
        if let Some(bits) = self.pattern_bits.get(pattern) {
            return Ok(bits.clone());
        }
        let mut bits = Vec::new();
        for part in parts(rule, pattern)? {
            match part {
                Part::Literal(literal) => bits.push(self.literal(&literal)),
                part => {
                    let bit = self.next_bit();
                    self.patterns.push((bit, part));
                    bits.push(bit);
                }
            }
        }
        self.pattern_bits.insert(pattern.to_owned(), bits.clone());
        Ok(bits)
    }

    fn note(&self, listing: &mut Listing, name: &OsStr) {
        let bytes = name.as_encoded_bytes();
        if let Some(&bit) = self.names.get(bytes) {
            listing.set(bit);
        }
        for (bit, part) in &self.patterns {
            if part.matches(bytes, name) {
                listing.set(*bit);
            }
        }
    }
}

struct Compiled {
    info: Match,
    markers: Listing,
    contains: Listing,
    contains_tag: Option<u32>,
    needs_contents: bool,
}

#[derive(Default)]
struct Index {
    literal: FxMap<Box<[u8]>, Vec<usize>>,
    patterns: Vec<(usize, Part)>,
}

impl Index {
    fn add(&mut self, index: usize, parts: Vec<Part>) {
        for part in parts {
            match part {
                Part::Literal(literal) => self.literal.entry(literal).or_default().push(index),
                part => self.patterns.push((index, part)),
            }
        }
    }

    fn find_map<R>(&self, name: &OsStr, mut hit: impl FnMut(usize) -> Option<R>) -> Option<R> {
        let bytes = name.as_encoded_bytes();
        let literal: &[usize] = if self.literal.is_empty() {
            &[]
        } else {
            self.literal.get(bytes).map_or(&[], Vec::as_slice)
        };
        let mut literal = literal.iter().copied().peekable();
        let mut patterns = self
            .patterns
            .iter()
            .filter(|(_, part)| part.matches(bytes, name))
            .map(|(index, _)| *index)
            .peekable();
        let mut last = None;
        loop {
            let next = match (literal.peek(), patterns.peek()) {
                (Some(&a), Some(&b)) if a <= b => literal.next(),
                (_, Some(_)) => patterns.next(),
                (Some(_), None) => literal.next(),
                (None, None) => return None,
            }?;
            if last == Some(next) {
                continue;
            }
            last = Some(next);
            if let Some(found) = hit(next) {
                return Some(found);
            }
        }
    }
}

pub struct Detector {
    rules: Vec<Compiled>,
    dirs: Index,
    files: Index,
    evidence: Evidence,
    reported: Vec<Category>,
}

fn is_plain_name(value: &str) -> bool {
    !value.is_empty() && value != "." && value != ".." && !value.contains(['/', '\\'])
}

fn has_glob_meta(pattern: &str) -> bool {
    pattern.contains(['*', '?', '[', ']', '{', '}'])
}

fn parse_glob(name: &str, pattern: &str) -> Result<Glob, RuleError> {
    Glob::new(pattern).map_err(|source| RuleError::Pattern {
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
        let mut evidence = Evidence::default();
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
                Kind::External => return Err(RuleError::External(spec.name.clone())),
            };
            if has_glob_meta(&spec.pattern) {
                target.add(index, parts(&spec.name, &spec.pattern)?);
            } else if is_plain_name(&spec.pattern) {
                target.add(index, vec![Part::Literal(spec.pattern.as_bytes().into())]);
            } else {
                return Err(RuleError::NotAName {
                    name: spec.name.clone(),
                    value: spec.pattern.clone(),
                });
            }
            let mut markers = Listing::default();
            for marker in &spec.markers {
                let bits = if has_glob_meta(marker) {
                    evidence.pattern(&spec.name, marker)?
                } else {
                    vec![evidence.literal(marker.as_bytes())]
                };
                bits.into_iter().for_each(|bit| markers.set(bit));
            }
            let mut contains = Listing::default();
            let mut contains_tag = None;
            for entry in &spec.contains {
                let bit = evidence.literal(entry.as_bytes());
                if entry == CACHEDIR_TAG {
                    contains_tag = Some(bit);
                } else {
                    contains.set(bit);
                }
            }
            rules.push(Compiled {
                info: Arc::new(RuleInfo {
                    name: spec.name.clone(),
                    category: spec.category,
                    safety: spec.safety,
                    ecosystem: spec.ecosystem.clone(),
                }),
                markers,
                contains,
                contains_tag,
                needs_contents: !spec.contains.is_empty(),
            });
        }

        let detector = Self {
            rules,
            dirs,
            files,
            evidence,
            reported: Category::ALL.to_vec(),
        };
        Ok(detector.watching(
            super::APP_DATA_MARKERS
                .iter()
                .flat_map(|set| set.iter().copied()),
        ))
    }

    /// @name watching
    /// @description Makes listings record whether they hold any of `names`, so
    /// [`Detector::lists`] can answer for them. Rule markers and contents are recorded already.
    ///
    /// @example
    /// let detector = Detector::new(&specs)?.watching([".kuizerignore"]);
    pub fn watching<'n>(mut self, names: impl IntoIterator<Item = &'n str>) -> Self {
        for name in names {
            self.evidence.literal(name.as_bytes());
        }
        self
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

    /// @name watches
    /// @description Returns whether listings record an entry called `name`, so
    /// [`Detector::lists`] may be asked about it.
    ///
    /// @example
    /// if detector.watches(".kuizerignore") && detector.lists(&own, ".kuizerignore") { skip(); }
    pub fn watches(&self, name: &str) -> bool {
        self.evidence.names.contains_key(name.as_bytes())
    }

    /// @name note
    /// @description Records one directory entry in `listing`. Call it for every entry of a
    /// directory before classifying it or its children.
    ///
    /// @example
    /// let mut own = Listing::default();
    /// detector.note(&mut own, entry.file_name().as_ref());
    pub fn note(&self, listing: &mut Listing, name: &OsStr) {
        self.evidence.note(listing, name);
    }

    /// @name listing
    /// @description Builds the listing of a directory holding `names`.
    ///
    /// @example
    /// let parent = detector.listing(["package.json", "node_modules"]);
    pub fn listing<S: AsRef<OsStr>>(&self, names: impl IntoIterator<Item = S>) -> Listing {
        let mut listing = Listing::default();
        for name in names {
            self.note(&mut listing, name.as_ref());
        }
        listing
    }

    /// @name lists
    /// @description Returns whether `listing` holds an entry called `name`. Only names a rule
    /// asks about or that were passed to [`Detector::watching`] are recorded.
    ///
    /// @example
    /// let ignored = detector.lists(&own, ".kuizerignore");
    pub fn lists(&self, listing: &Listing, name: &str) -> bool {
        let bit = self.evidence.names.get(name.as_bytes());
        debug_assert!(bit.is_some(), "`{name}` is not watched by the detector");
        bit.is_some_and(|bit| listing.get(*bit))
    }

    fn markers_match(rule: &Compiled, parent: &Listing) -> bool {
        rule.markers.is_empty() || parent.intersects(&rule.markers)
    }

    pub fn classify_dir(&self, path: &Path, parent: &Listing, own: &Listing) -> Option<Match> {
        let name = path.file_name()?;
        self.dirs.find_map(name, |index| {
            let rule = &self.rules[index];
            let contained = !rule.needs_contents
                || own.intersects(&rule.contains)
                || rule
                    .contains_tag
                    .is_some_and(|bit| own.get(bit) && valid_cachedir_tag(path));
            (contained && Self::markers_match(rule, parent)).then(|| Arc::clone(&rule.info))
        })
    }

    pub fn classify_file(&self, name: &OsStr, parent: &Listing) -> Option<Match> {
        if self.files.literal.is_empty() && self.files.patterns.is_empty() {
            return None;
        }
        self.files.find_map(name, |index| {
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

    fn listing(detector: &Detector, names: &[&str]) -> Listing {
        detector.listing(names)
    }

    fn detector() -> Detector {
        Detector::new(&builtin_rules()).expect("builtin rules compile")
    }

    #[test]
    fn node_modules_needs_package_json() {
        let detector = detector();
        let path = Path::new("/p/node_modules");
        let own = listing(&detector, &["react"]);
        let hit = detector.classify_dir(
            path,
            &listing(&detector, &["package.json", "node_modules"]),
            &own,
        );
        assert_eq!(hit.map(|m| m.name.clone()).as_deref(), Some("node-modules"));
        assert!(
            detector
                .classify_dir(path, &listing(&detector, &["node_modules"]), &own)
                .is_none()
        );
    }

    #[test]
    fn dist_without_project_is_ignored() {
        let detector = detector();
        let path = Path::new("/p/dist");
        let own = listing(&detector, &["index.js"]);
        assert!(
            detector
                .classify_dir(path, &listing(&detector, &["dist", "README.md"]), &own)
                .is_none()
        );
        let hit = detector
            .classify_dir(path, &listing(&detector, &["dist", "package.json"]), &own)
            .expect("dist next to package.json");
        assert_eq!(hit.safety, Safety::Review);
    }

    #[test]
    fn coverage_needs_report_contents() {
        let detector = detector();
        let path = Path::new("/p/coverage");
        let parent = listing(&detector, &["package.json", "coverage"]);
        assert!(
            detector
                .classify_dir(path, &parent, &listing(&detector, &["notes.md"]))
                .is_none()
        );
        assert!(
            detector
                .classify_dir(path, &parent, &listing(&detector, &["lcov.info"]))
                .is_some()
        );
    }

    #[test]
    fn cache_name_alone_is_not_enough() {
        let detector = detector();
        let path = Path::new("/home/me/.cache");
        let own = listing(&detector, &["fontconfig"]);
        assert!(
            detector
                .classify_dir(path, &listing(&detector, &[".cache"]), &own)
                .is_none()
        );
    }

    #[test]
    fn file_rules_match_globs() {
        let detector = detector();
        let parent = listing(&detector, &["tsconfig.json"]);
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
            (
                "build",
                &["build.gradle.kts", "package.json"],
                &["intermediates", "outputs"],
                Some("gradle-build"),
            ),
            (
                "build",
                &["build.gradle"],
                &["classes", "libs"],
                Some("gradle-build"),
            ),
            ("build", &["build.gradle"], &["index.html"], None),
            ("build", &["settings.gradle"], &["tmp"], None),
            (
                ".dart_tool",
                &["pubspec.yaml", "lib"],
                &["package_config.json"],
                Some("dart-tool"),
            ),
            (".dart_tool", &["README.md"], &["package_config.json"], None),
            (
                "build",
                &["pubspec.yaml", "package.json"],
                &["web"],
                Some("flutter-build"),
            ),
            ("build", &["analysis_options.yaml"], &["web"], None),
            (
                "build",
                &["CMakeLists.txt", "package.json"],
                &["CMakeCache.txt", "CMakeFiles"],
                Some("cmake-build"),
            ),
            (
                "cmake-build-debug",
                &["CMakeLists.txt"],
                &["CMakeCache.txt"],
                Some("cmake-build"),
            ),
            (
                "build-release",
                &[],
                &["CMakeCache.txt"],
                Some("cmake-build"),
            ),
            ("build", &["CMakeLists.txt"], &["notes.md"], None),
            (
                "engine",
                &["CMakeLists.txt"],
                &["CMakeCache.txt", "CMakeLists.txt"],
                None,
            ),
            (
                "Library",
                &["Assets", "ProjectSettings", "Packages"],
                &["ArtifactDB", "PackageCache"],
                Some("unity-library"),
            ),
            ("Library", &["ProjectSettings"], &["Fonts"], None),
            ("Library", &["Assets"], &["ArtifactDB"], None),
            (
                "Temp",
                &["Assets", "ProjectSettings"],
                &["UnityTempFile-1"],
                Some("unity-temp"),
            ),
            ("Temp", &["Assets"], &["UnityTempFile-1"], None),
        ];
        for (dir, parent, own, expected) in cases {
            let hit = detector.classify_dir(
                &Path::new("/p").join(dir),
                &listing(&detector, parent),
                &listing(&detector, own),
            );
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
                &listing(&detector, &["package.json"]),
                &listing(&detector, &[]),
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
        let own = listing(&detector, &[]);
        assert!(
            detector
                .classify_dir(
                    path,
                    &listing(&detector, &["App.csproj", "node_modules"]),
                    &own
                )
                .is_some()
        );
        assert!(
            detector
                .classify_dir(
                    path,
                    &listing(&detector, &["App.csproj.user", "node_modules"]),
                    &own
                )
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
