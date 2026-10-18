use std::cmp::Ordering;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::config::{Config, ConfigError};
use crate::platform::MountTable;
use crate::protect::Protected;
use crate::scan::Cancel;
use crate::scan::Issue;
use crate::scan::walk::{Flow, Visitor, Walker};

const MAX_PIN_FILE: u64 = 256 * 1024;

const LTS: &[(&str, &str)] = &[
    ("argon", "4"),
    ("boron", "6"),
    ("carbon", "8"),
    ("dubnium", "10"),
    ("erbium", "12"),
    ("fermium", "14"),
    ("gallium", "16"),
    ("hydrogen", "18"),
    ("iron", "20"),
    ("jod", "22"),
    ("krypton", "24"),
];

const NEWEST: &[&str] = &["node", "stable", "latest", "current", "lts/*", "lts"];

/// A version a project asks for, and the file that asks for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    pub runtime: String,
    pub spec: String,
    pub file: PathBuf,
}

fn first_word(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .and_then(|line| line.split_whitespace().next())
        .map(str::to_owned)
}

fn rust_channel(text: &str) -> Option<String> {
    match text.parse::<toml::Table>() {
        Ok(table) => table
            .get("toolchain")
            .and_then(|toolchain| toolchain.get("channel"))
            .and_then(toml::Value::as_str)
            .map(str::to_owned),
        Err(_) => first_word(text),
    }
}

fn tool_versions(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let runtime = match words.next()? {
                "nodejs" | "node" => "node",
                "python" => "python",
                "rust" => "rust",
                "golang" | "go" => "go",
                "java" => "java",
                _ => return None,
            };
            Some((runtime.to_owned(), words.next()?.to_owned()))
        })
        .collect()
}

/// @name parse
/// @description Reads the versions a pin file asks for. Knows `rust-toolchain`,
/// `rust-toolchain.toml`, `.nvmrc`, `.node-version`, `.python-version`, `.sdkmanrc`,
/// `.tool-versions`, the `toolchain` line of `go.mod` and the `volta` key of `package.json`.
/// Returns `(runtime, spec)` pairs, empty for any other file name.
///
/// @example
/// assert_eq!(pins::parse(".nvmrc", "v18\n"), [("node".to_owned(), "v18".to_owned())]);
pub fn parse(name: &str, text: &str) -> Vec<(String, String)> {
    let pair = |runtime: &str, spec: Option<String>| {
        spec.map(|spec| (runtime.to_owned(), spec))
            .into_iter()
            .collect::<Vec<_>>()
    };
    match name {
        "rust-toolchain" | "rust-toolchain.toml" => pair("rust", rust_channel(text)),
        ".nvmrc" | ".node-version" => pair("node", first_word(text)),
        ".python-version" => text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(|line| ("python".to_owned(), line.to_owned()))
            .collect(),
        ".sdkmanrc" => text
            .lines()
            .map(str::trim)
            .filter(|line| !line.starts_with('#'))
            .filter_map(|line| line.split_once('='))
            .map(|(runtime, spec)| (runtime.trim().to_owned(), spec.trim().to_owned()))
            .filter(|(runtime, spec)| !runtime.is_empty() && !spec.is_empty())
            .collect(),
        ".tool-versions" => tool_versions(text),
        "go.mod" => pair(
            "go",
            text.lines()
                .map(str::trim)
                .find_map(|line| line.strip_prefix("toolchain "))
                .map(|spec| spec.trim().to_owned()),
        ),
        "package.json" if text.contains("\"volta\"") => pair(
            "node",
            serde_json::from_str::<serde_json::Value>(text)
                .ok()
                .and_then(|json| json.get("volta")?.get("node")?.as_str().map(str::to_owned)),
        ),
        _ => Vec::new(),
    }
}

fn is_pin_file(name: &str) -> bool {
    matches!(
        name,
        "rust-toolchain"
            | "rust-toolchain.toml"
            | ".nvmrc"
            | ".node-version"
            | ".python-version"
            | ".sdkmanrc"
            | ".tool-versions"
            | "go.mod"
            | "package.json"
    )
}

fn read_small(path: &Path) -> Option<String> {
    let mut text = String::new();
    fs::File::open(path)
        .ok()?
        .take(MAX_PIN_FILE)
        .read_to_string(&mut text)
        .ok()?;
    Some(text)
}

/// Collects pin files during a walk. Use it as a scan companion or through `find`.
#[derive(Debug, Default)]
pub struct PinFinder {
    pub pins: Vec<Pin>,
    cancel: Option<Cancel>,
}

impl PinFinder {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Visitor for PinFinder {
    fn cancelled(&self) -> bool {
        self.cancel.as_ref().is_some_and(Cancel::is_cancelled)
    }

    fn issue(&mut self, _issue: Issue) -> Flow {
        Flow::Continue
    }

    fn file(&mut self, path: &Path, file_type: fs::FileType) -> Flow {
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            return Flow::Continue;
        };
        if !file_type.is_file() || !is_pin_file(name) {
            return Flow::Continue;
        }
        if let Some(text) = read_small(path) {
            self.pins
                .extend(parse(name, &text).into_iter().map(|(runtime, spec)| Pin {
                    runtime,
                    spec,
                    file: path.to_path_buf(),
                }));
        }
        Flow::Continue
    }
}

/// @name find
/// @description Walks `roots` like a project scan, skipping artifact folders, excludes and
/// protected data, and returns every toolchain pin it finds.
///
/// @example
/// let pins = pins::find(&config, &roots, &Cancel::new())?;
pub fn find(config: &Config, roots: &[PathBuf], cancel: &Cancel) -> Result<Vec<Pin>, ConfigError> {
    let detector = config.detector()?;
    let excludes = config.excludes()?;
    let mounts = MountTable::load();
    let protected = Protected::for_user();
    let mut walker = Walker {
        scan: &config.scan,
        detector: &detector,
        excludes: &excludes,
        mounts: &mounts,
        protected: &protected,
        visitor: PinFinder {
            pins: Vec::new(),
            cancel: Some(cancel.clone()),
        },
    };
    for root in crate::scan::normalize_roots(roots) {
        walker.walk_root(&root);
    }
    Ok(walker.visitor.pins)
}

fn numbers(version: &str) -> Vec<u64> {
    version
        .trim_start_matches(['v', 'V'])
        .trim_start_matches("go")
        .split(['.', '-', '+'])
        .map_while(|part| part.parse().ok())
        .collect()
}

fn newer(a: &str, b: &str) -> Ordering {
    numbers(a).cmp(&numbers(b))
}

fn prefix(spec: &str, version: &str) -> bool {
    let wanted = numbers(spec);
    let have = numbers(version);
    !wanted.is_empty() && have.len() >= wanted.len() && have[..wanted.len()] == wanted[..]
}

fn uv_version(name: &str) -> &str {
    name.split('-').nth(1).unwrap_or(name)
}

/// @name matches
/// @description Says whether an installed version satisfies a pin for one runtime. Rust
/// toolchain names match their channel with or without the host suffix, numeric Node, Python and
/// Go versions match by leading components (`18` matches `v18.17.0`), anything else must match
/// exactly. Aliases like `lts/*` are resolved by `resolve`, not here.
///
/// @example
/// assert!(pins::matches("node", "18", "v18.17.0"));
pub fn matches(runtime: &str, spec: &str, version: &str) -> bool {
    let spec = spec.trim();
    if spec == version {
        return true;
    }
    match runtime {
        "rust" => version
            .strip_prefix(spec)
            .is_some_and(|rest| rest.starts_with('-')),
        "node" | "go" => prefix(spec, version),
        "python" => {
            let version = if version.starts_with("cpython-") || version.starts_with("pypy-") {
                uv_version(version)
            } else {
                version
            };
            spec == version || prefix(spec, version)
        }
        _ => false,
    }
}

/// @name resolve
/// @description Returns the indexes of `versions` that a pin selects. Node aliases resolve to the
/// newest installed version (`lts/*`, `node`, `stable`) or to the newest of an LTS line
/// (`lts/hydrogen`). A partial version selects every match, since the tool picks one of them at
/// run time.
///
/// @example
/// assert_eq!(pins::resolve("node", "lts/*", &["v18.1.0", "v20.2.0"]), [1]);
pub fn resolve(runtime: &str, spec: &str, versions: &[&str]) -> Vec<usize> {
    let spec = spec.trim();
    if runtime == "node" {
        let lowered = spec.to_ascii_lowercase();
        let line = lowered
            .strip_prefix("lts/")
            .and_then(|name| LTS.iter().find(|(codename, _)| *codename == name))
            .map(|(_, major)| *major);
        let candidates: Vec<usize> = match line {
            Some(major) => (0..versions.len())
                .filter(|&index| prefix(major, versions[index]))
                .collect(),
            None if NEWEST.contains(&lowered.as_str()) => (0..versions.len()).collect(),
            None => Vec::new(),
        };
        if line.is_some() || NEWEST.contains(&lowered.as_str()) {
            return candidates
                .into_iter()
                .max_by(|&a, &b| newer(versions[a], versions[b]))
                .into_iter()
                .collect();
        }
    }
    (0..versions.len())
        .filter(|&index| matches(runtime, spec, versions[index]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(runtime, spec)| ((*runtime).to_owned(), (*spec).to_owned()))
            .collect()
    }

    #[test]
    fn parses_every_pin_file() {
        assert_eq!(
            parse("rust-toolchain.toml", "[toolchain]\nchannel = \"1.80.0\"\n"),
            pairs(&[("rust", "1.80.0")])
        );
        assert_eq!(
            parse("rust-toolchain", "nightly-2024-05-01\n"),
            pairs(&[("rust", "nightly-2024-05-01")])
        );
        assert_eq!(
            parse(".nvmrc", "# node\nv18.17.0\n"),
            pairs(&[("node", "v18.17.0")])
        );
        assert_eq!(
            parse(".python-version", "3.11\n3.12.1\n"),
            pairs(&[("python", "3.11"), ("python", "3.12.1")])
        );
        assert_eq!(
            parse(".sdkmanrc", "# java\njava=17.0.8-tem\ngradle = 8.5\n"),
            pairs(&[("java", "17.0.8-tem"), ("gradle", "8.5")])
        );
        assert_eq!(
            parse(
                ".tool-versions",
                "nodejs 20.1.0\nruby 3.3.0\ngolang 1.22.1\n"
            ),
            pairs(&[("node", "20.1.0"), ("go", "1.22.1")])
        );
        assert_eq!(
            parse("go.mod", "module x\n\ngo 1.21\n\ntoolchain go1.21.4\n"),
            pairs(&[("go", "go1.21.4")])
        );
        assert_eq!(
            parse(
                "package.json",
                r#"{"name":"web","volta":{"node":"18.17.0"}}"#
            ),
            pairs(&[("node", "18.17.0")])
        );
        assert!(parse("package.json", r#"{"name":"web"}"#).is_empty());
        assert!(parse("README.md", "v18").is_empty());
    }

    #[test]
    fn versions_match_by_runtime() {
        assert!(matches("rust", "stable", "stable-x86_64-unknown-linux-gnu"));
        assert!(matches("rust", "1.80.0", "1.80.0-aarch64-apple-darwin"));
        assert!(!matches("rust", "1.80", "1.80.0-x86_64-unknown-linux-gnu"));
        assert!(!matches("rust", "stable", "beta-x86_64-unknown-linux-gnu"));
        assert!(matches("node", "18", "v18.17.0"));
        assert!(matches("node", "v18.17", "18.17.1"));
        assert!(!matches("node", "18", "v20.1.0"));
        assert!(!matches("node", "1", "v18.0.0"));
        assert!(matches("python", "3.11", "3.11.4"));
        assert!(matches("python", "3.11", "cpython-3.11.4-linux-x86_64-gnu"));
        assert!(!matches("python", "3.1", "3.11.4"));
        assert!(matches("go", "go1.21.4", "go1.21.4"));
        assert!(matches("go", "1.21", "go1.21.4"));
        assert!(matches("java", "17.0.8-tem", "17.0.8-tem"));
        assert!(!matches("java", "17", "17.0.8-tem"));
    }

    #[test]
    fn node_aliases_pick_the_newest_install() {
        let versions = ["v16.20.0", "v18.17.0", "v18.19.1", "v20.11.0"];
        assert_eq!(resolve("node", "lts/*", &versions), [3]);
        assert_eq!(resolve("node", "lts/hydrogen", &versions), [2]);
        assert_eq!(resolve("node", "lts/argon", &versions), Vec::<usize>::new());
        assert_eq!(resolve("node", "18", &versions), [1, 2]);
        assert_eq!(resolve("node", "system", &versions), Vec::<usize>::new());
    }

    #[test]
    fn finds_pins_outside_artifacts() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        fs::create_dir_all(root.join("web/node_modules/pkg")).expect("dirs");
        fs::write(root.join("web/package.json"), "{}").expect("package");
        fs::write(root.join("web/.nvmrc"), "20\n").expect("nvmrc");
        fs::write(root.join("web/node_modules/pkg/.nvmrc"), "16\n").expect("nested");
        fs::create_dir_all(root.join("tool")).expect("tool");
        fs::write(
            root.join("tool/rust-toolchain.toml"),
            "[toolchain]\nchannel = \"1.80.0\"\n",
        )
        .expect("rust");

        let mut pins =
            find(&Config::default(), &[root.to_path_buf()], &Cancel::new()).expect("pins");
        pins.sort_by(|a, b| a.file.cmp(&b.file));
        let found: Vec<(&str, &str)> = pins
            .iter()
            .map(|pin| (pin.runtime.as_str(), pin.spec.as_str()))
            .collect();
        assert_eq!(found, [("rust", "1.80.0"), ("node", "20")]);
    }
}
