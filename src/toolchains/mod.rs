pub mod pins;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, PoisonError};

use serde::{Deserialize, Serialize};

pub use pins::{Pin, PinFinder};

use crate::config::Config;
use crate::delete::{self, DeleteError, Guard, Mode, Target};
use crate::detect::{Category, Kind, RuleInfo, Safety};
use crate::output::display_path;
use crate::protect::Protected;
use crate::scan::{self, Cancel, Candidate};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tool {
    Rustup,
    Nvm,
    Fnm,
    Volta,
    Pyenv,
    Uv,
    Sdkman,
    Go,
}

impl Tool {
    pub const ALL: [Tool; 8] = [
        Self::Rustup,
        Self::Nvm,
        Self::Fnm,
        Self::Volta,
        Self::Pyenv,
        Self::Uv,
        Self::Sdkman,
        Self::Go,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Rustup => "rustup",
            Self::Nvm => "nvm",
            Self::Fnm => "fnm",
            Self::Volta => "volta",
            Self::Pyenv => "pyenv",
            Self::Uv => "uv",
            Self::Sdkman => "sdkman",
            Self::Go => "go",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tool| tool.name() == name)
    }

    /// The tool's own uninstall command, for tools that keep metadata outside the version folder.
    pub fn uninstaller(self) -> Option<&'static str> {
        match self {
            Self::Rustup => Some("rustup"),
            Self::Uv => Some("uv"),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Locations {
    pub rustup: PathBuf,
    pub nvm: PathBuf,
    pub fnm: PathBuf,
    pub volta: PathBuf,
    pub pyenv: PathBuf,
    pub uv: PathBuf,
    pub sdkman: PathBuf,
    pub go: PathBuf,
}

fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

impl Locations {
    pub fn under(home: &Path) -> Self {
        Self {
            rustup: home.join(".rustup"),
            nvm: home.join(".nvm"),
            fnm: home.join(".local/share/fnm"),
            volta: home.join(".volta"),
            pyenv: home.join(".pyenv"),
            uv: home.join(".local/share/uv/python"),
            sdkman: home.join(".sdkman"),
            go: home.join("sdk"),
        }
    }

    pub fn for_user() -> Option<Self> {
        let home = dirs::home_dir()?;
        let defaults = Self::under(&home);
        let legacy_fnm = home.join(".fnm");
        Some(Self {
            rustup: env_dir("RUSTUP_HOME").unwrap_or(defaults.rustup),
            nvm: env_dir("NVM_DIR").unwrap_or(defaults.nvm),
            fnm: env_dir("FNM_DIR").unwrap_or_else(|| {
                if legacy_fnm.is_dir() {
                    legacy_fnm
                } else {
                    dirs::data_dir().map_or(defaults.fnm, |data| data.join("fnm"))
                }
            }),
            volta: env_dir("VOLTA_HOME").unwrap_or(defaults.volta),
            pyenv: env_dir("PYENV_ROOT").unwrap_or(defaults.pyenv),
            uv: env_dir("UV_PYTHON_INSTALL_DIR")
                .or_else(|| env_dir("XDG_DATA_HOME").map(|data| data.join("uv/python")))
                .unwrap_or(defaults.uv),
            sdkman: env_dir("SDKMAN_DIR").unwrap_or(defaults.sdkman),
            go: defaults.go,
        })
    }

    /// @name roots
    /// @description Lists the folder each tool keeps its installed versions in, whether or not
    /// it exists.
    ///
    /// @example
    /// let skip = Locations::for_user()?.roots();
    pub fn roots(&self) -> Vec<PathBuf> {
        Tool::ALL
            .into_iter()
            .map(|tool| self.versions(tool))
            .collect()
    }

    fn versions(&self, tool: Tool) -> PathBuf {
        match tool {
            Tool::Rustup => self.rustup.join("toolchains"),
            Tool::Nvm => self.nvm.join("versions/node"),
            Tool::Fnm => self.fnm.join("node-versions"),
            Tool::Volta => self.volta.join("tools/image/node"),
            Tool::Pyenv => self.pyenv.join("versions"),
            Tool::Uv => self.uv.clone(),
            Tool::Sdkman => self.sdkman.join("candidates"),
            Tool::Go => self.go.clone(),
        }
    }
}

/// One installed version of a language toolchain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Toolchain {
    pub tool: Tool,
    pub runtime: String,
    pub version: String,
    pub path: PathBuf,
    pub default: bool,
    pub pins: Vec<PathBuf>,
}

impl Toolchain {
    /// A default or pinned version is never safe: something still runs with it.
    pub fn safety(&self) -> Safety {
        if self.default || !self.pins.is_empty() {
            Safety::Review
        } else {
            Safety::Safe
        }
    }

    /// @name Toolchain::note
    /// @description Says why a version needs review: it is the tool's default, a project pins it,
    /// or both. Empty for a version nothing uses.
    ///
    /// @example
    /// assert_eq!(toolchain.note(), "pinned by ~/apps/web/.nvmrc");
    pub fn note(&self) -> String {
        let pinned = match self.pins.as_slice() {
            [] => None,
            [only] => Some(format!("pinned by {}", display_path(only))),
            [first, rest @ ..] => Some(format!(
                "pinned by {} and {} more",
                display_path(first),
                rest.len()
            )),
        };
        match (self.default, pinned) {
            (true, Some(pinned)) => format!("default, {pinned}"),
            (true, None) => "default".to_owned(),
            (false, Some(pinned)) => pinned,
            (false, None) => String::new(),
        }
    }

    fn rule(&self) -> RuleInfo {
        RuleInfo {
            name: self.tool.name().to_owned(),
            category: Category::Dependencies,
            safety: self.safety(),
            ecosystem: self.runtime.clone(),
        }
    }
}

fn dirs_in(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(String, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| Some((entry.file_name().into_string().ok()?, entry.path())))
        .filter(|(name, _)| !name.starts_with('.'))
        .collect();
    found.sort();
    found
}

fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
}

fn link_name(path: &Path) -> Option<String> {
    fs::read_link(path)
        .ok()?
        .components()
        .filter_map(|part| part.as_os_str().to_str().map(str::to_owned))
        .find(|part| part.starts_with('v') && part[1..].starts_with(|c: char| c.is_ascii_digit()))
}

fn rustup_settings(locations: &Locations) -> (Option<String>, Vec<Pin>) {
    let Some(table) = read_trimmed(&locations.rustup.join("settings.toml"))
        .and_then(|text| text.parse::<toml::Table>().ok())
    else {
        return (None, Vec::new());
    };
    let default = table
        .get("default_toolchain")
        .and_then(toml::Value::as_str)
        .map(str::to_owned);
    let overrides = table
        .get("overrides")
        .and_then(toml::Value::as_table)
        .map(|overrides| {
            overrides
                .iter()
                .filter_map(|(dir, toolchain)| {
                    Some(Pin {
                        runtime: "rust".to_owned(),
                        spec: toolchain.as_str()?.to_owned(),
                        file: PathBuf::from(dir),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    (default, overrides)
}

fn volta_default(locations: &Locations) -> Option<String> {
    let text = read_trimmed(&locations.volta.join("tools/user/platform.json"))?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    json.get("node")?
        .get("runtime")?
        .as_str()
        .map(str::to_owned)
}

fn tool_versions(locations: &Locations, tool: Tool) -> Vec<Toolchain> {
    let root = locations.versions(tool);
    let runtime = match tool {
        Tool::Rustup => "rust",
        Tool::Nvm | Tool::Fnm | Tool::Volta => "node",
        Tool::Pyenv | Tool::Uv => "python",
        Tool::Go => "go",
        Tool::Sdkman => "",
    };
    let version = |(name, path): (String, PathBuf), runtime: &str| Toolchain {
        tool,
        runtime: runtime.to_owned(),
        version: name,
        path,
        default: false,
        pins: Vec::new(),
    };
    match tool {
        Tool::Sdkman => dirs_in(&root)
            .into_iter()
            .flat_map(|(candidate, dir)| {
                let current = fs::read_link(dir.join("current"))
                    .ok()
                    .and_then(|target| target.file_name()?.to_str().map(str::to_owned));
                dirs_in(&dir)
                    .into_iter()
                    .filter(|(name, _)| name != "current")
                    .map(|entry| {
                        let mut toolchain = version(entry, &candidate);
                        toolchain.default = current.as_deref() == Some(&toolchain.version);
                        toolchain
                    })
                    .collect::<Vec<_>>()
            })
            .collect(),
        Tool::Go => dirs_in(&root)
            .into_iter()
            .filter(|(name, _)| {
                name.strip_prefix("go")
                    .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
            })
            .map(|entry| version(entry, runtime))
            .collect(),
        _ => dirs_in(&root)
            .into_iter()
            .map(|entry| version(entry, runtime))
            .collect(),
    }
}

fn mark(toolchains: &mut [Toolchain], spec: &str, mut apply: impl FnMut(&mut Toolchain)) {
    let Some(runtime) = toolchains
        .first()
        .map(|toolchain| toolchain.runtime.clone())
    else {
        return;
    };
    let versions: Vec<&str> = toolchains
        .iter()
        .map(|toolchain| toolchain.version.as_str())
        .collect();
    for index in pins::resolve(&runtime, spec, &versions) {
        apply(&mut toolchains[index]);
    }
}

fn defaults(locations: &Locations, tool: Tool, toolchains: &mut [Toolchain]) -> Vec<Pin> {
    let mut extra = Vec::new();
    let default = match tool {
        Tool::Rustup => {
            let (default, overrides) = rustup_settings(locations);
            extra = overrides;
            default
        }
        Tool::Nvm => read_trimmed(&locations.nvm.join("alias/default")),
        Tool::Fnm => link_name(&locations.fnm.join("aliases/default")),
        Tool::Volta => volta_default(locations),
        Tool::Pyenv => read_trimmed(&locations.pyenv.join("version")),
        Tool::Uv | Tool::Sdkman | Tool::Go => None,
    };
    if let Some(default) = default {
        for spec in default
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
        {
            mark(toolchains, spec, |toolchain| toolchain.default = true);
        }
    }
    extra
}

/// @name find
/// @description Lists the installed versions of `tools` under `locations`, with each tool's
/// default version marked. rustup directory overrides count as pins. Symlinked versions, such
/// as `rustup toolchain link`, are left out because kuizer does not own what they point to.
///
/// @example
/// let found = toolchains::find(&Locations::for_user()?, &Tool::ALL);
pub fn find(locations: &Locations, tools: &[Tool]) -> Vec<Toolchain> {
    let mut found = Vec::new();
    let mut overrides = Vec::new();
    for &tool in tools {
        let mut toolchains = tool_versions(locations, tool);
        if tool == Tool::Sdkman {
            found.extend(toolchains);
            continue;
        }
        overrides.extend(defaults(locations, tool, &mut toolchains));
        found.extend(toolchains);
    }
    apply_pins(&mut found, &overrides);
    found
}

/// @name apply_pins
/// @description Records on each toolchain the files that pin it. Each pin is resolved against
/// the versions of one tool at a time, so `lts/*` marks the newest Node of nvm and of fnm.
///
/// @example
/// toolchains::apply_pins(&mut found, &pins::find(&config, &roots, &cancel)?);
pub fn apply_pins(toolchains: &mut [Toolchain], pins: &[Pin]) {
    let mut groups: Vec<(Tool, String)> = toolchains
        .iter()
        .map(|toolchain| (toolchain.tool, toolchain.runtime.clone()))
        .collect();
    groups.sort();
    groups.dedup();
    for (tool, runtime) in groups {
        let indexes: Vec<usize> = (0..toolchains.len())
            .filter(|&index| toolchains[index].tool == tool && toolchains[index].runtime == runtime)
            .collect();
        let versions: Vec<&str> = indexes
            .iter()
            .map(|&index| toolchains[index].version.as_str())
            .collect();
        let mut hits: Vec<(usize, PathBuf)> = Vec::new();
        for pin in pins.iter().filter(|pin| pin.runtime == runtime) {
            for matched in pins::resolve(&runtime, &pin.spec, &versions) {
                hits.push((indexes[matched], pin.file.clone()));
            }
        }
        for (index, file) in hits {
            let pins = &mut toolchains[index].pins;
            if !pins.contains(&file) {
                pins.push(file);
            }
        }
    }
    for toolchain in toolchains {
        toolchain.pins.sort();
    }
}

/// @name measure
/// @description Measures every toolchain in parallel and pairs it with a removal candidate,
/// largest first. The rule name is the tool, so `--select nvm` picks every nvm version.
///
/// @example
/// let entries = toolchains::measure(toolchains::find(&locations, &Tool::ALL));
pub fn measure(found: Vec<Toolchain>) -> Vec<(Toolchain, Candidate)> {
    measure_with(found, &Cancel::new(), |_| {})
}

/// @name measure_with
/// @description Like `measure`, but stops sizing once `cancel` is set and calls `on_measured`
/// from the measuring thread as each toolchain is done. Sizing runs on as many threads as a scan
/// of the folders holding the versions would use, not one per version.
///
/// @example
/// let entries = toolchains::measure_with(found, &cancel, |candidate| done.push(candidate.id));
pub fn measure_with(
    found: Vec<Toolchain>,
    cancel: &Cancel,
    on_measured: impl Fn(&Candidate) + Sync,
) -> Vec<(Toolchain, Candidate)> {
    let mut parents: Vec<PathBuf> = found
        .iter()
        .filter_map(|toolchain| toolchain.path.parent().map(Path::to_path_buf))
        .collect();
    parents.sort();
    parents.dedup();
    let threads = scan::scan_threads(&Config::default(), &parents);
    measure_with_threads(found, cancel, threads, on_measured)
}

/// @name measure_with_threads
/// @description Like `measure_with`, on at most `threads` threads, so a frontend can pass the
/// count from `scan::size_threads` or `scan::scan_threads` for its configuration.
///
/// @example
/// let entries = toolchains::measure_with_threads(found, &cancel, 4, |_| {});
pub fn measure_with_threads(
    found: Vec<Toolchain>,
    cancel: &Cancel,
    threads: usize,
    on_measured: impl Fn(&Candidate) + Sync,
) -> Vec<(Toolchain, Candidate)> {
    let on_measured = &on_measured;
    let workers = threads.clamp(1, found.len().max(1));
    let queue = Mutex::new(found.into_iter().enumerate().collect::<Vec<_>>());
    let done = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let next = queue.lock().unwrap_or_else(PoisonError::into_inner).pop();
                    let Some((index, toolchain)) = next else {
                        break;
                    };
                    let measured = scan::measure(&toolchain.path, cancel);
                    let candidate = Candidate {
                        id: index as u64 + 1,
                        path: toolchain.path.clone(),
                        kind: Kind::Dir,
                        rule: Arc::new(toolchain.rule()),
                        project: None,
                        usage: measured.usage,
                        newest: measured.newest,
                        identity: measured.identity,
                        project_last_active: None,
                    };
                    on_measured(&candidate);
                    done.lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push((toolchain, candidate));
                }
            });
        }
    });
    let mut entries = done.into_inner().unwrap_or_else(PoisonError::into_inner);
    entries.sort_by(|(a, x), (b, y)| {
        y.usage
            .allocated
            .cmp(&x.usage.allocated)
            .then_with(|| a.path.cmp(&b.path))
    });
    entries
}

/// @name roots
/// @description The guard roots for removing toolchains: the folder that holds each version,
/// because toolchains live outside the scanned folders.
///
/// @example
/// let guard = Guard::new(&toolchains::roots(&selected));
pub fn roots(candidates: &[Candidate]) -> Vec<PathBuf> {
    let mut parents: Vec<PathBuf> = candidates
        .iter()
        .filter_map(|candidate| candidate.path.parent().map(Path::to_path_buf))
        .collect();
    parents.sort();
    parents.dedup();
    parents
}

fn uninstall(program: &str, tool: Tool, version: &str) -> Result<(), DeleteError> {
    let args: &[&str] = match tool {
        Tool::Rustup => &["toolchain", "uninstall", version],
        _ => &["python", "uninstall", version],
    };
    let output = Command::new(program).args(args).output().map_err(|error| {
        DeleteError::Tool(if error.kind() == std::io::ErrorKind::NotFound {
            format!(
                "{program} is not on PATH; it keeps metadata outside the version folder, so kuizer only removes it through `{program} {}`",
                args[..args.len() - 1].join(" ")
            )
        } else {
            format!("cannot run {program}: {error}")
        })
    })?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(DeleteError::Tool(format!(
        "{program} failed: {}",
        stderr.lines().last().unwrap_or("no output").trim()
    )))
}

/// @name remove
/// @description Removes one toolchain version for good. rustup and uv versions go through their
/// own uninstall command so the tool's metadata stays consistent; every other version folder is
/// deleted permanently through the guard. Toolchains never go to the trash: they are large and
/// the tool downloads them again on demand. A dry run only revalidates.
///
/// @example
/// toolchains::remove(&Guard::new(&toolchains::roots(&selected)), &candidate, Mode::Permanent)?;
pub fn remove(guard: &Guard, candidate: &Candidate, mode: Mode) -> Result<(), DeleteError> {
    let tool = Tool::from_name(&candidate.rule.name)
        .ok_or_else(|| DeleteError::Tool(format!("{} is not a toolchain", candidate.rule.name)))?;
    let target = Target::from(candidate);
    let volta = tool == Tool::Volta
        && candidate
            .path
            .parent()
            .is_some_and(|parent| parent.ends_with("tools/image/node"));
    let unprotected;
    let guard = if volta {
        unprotected = guard.clone().with_protected(Protected::default());
        &unprotected
    } else {
        guard
    };
    let mode = match mode {
        Mode::DryRun => Mode::DryRun,
        Mode::Trash | Mode::Permanent => Mode::Permanent,
    };
    match tool.uninstaller() {
        Some(program) => {
            guard.check(&target.path, target.identity)?;
            if mode == Mode::DryRun {
                return Ok(());
            }
            let version = candidate
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| DeleteError::Tool("version folder has no name".to_owned()))?;
            uninstall(program, tool, version)
        }
        None => delete::delete(guard, &target, mode),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path();
        for dir in [
            ".rustup/toolchains/stable-x86_64-unknown-linux-gnu",
            ".rustup/toolchains/nightly-x86_64-unknown-linux-gnu",
            ".rustup/toolchains/1.80.0-x86_64-unknown-linux-gnu",
            ".nvm/versions/node/v18.17.0",
            ".nvm/versions/node/v20.11.0",
            ".nvm/alias",
            ".pyenv/versions/3.11.4",
            ".pyenv/versions/3.12.1",
            ".sdkman/candidates/java/17.0.8-tem",
            ".sdkman/candidates/java/21.0.1-tem",
            "sdk/go1.21.4",
            "sdk/gotip-not-a-version",
            ".local/share/uv/python/cpython-3.12.1-linux-x86_64-gnu",
        ] {
            fs::create_dir_all(home.join(dir)).expect("dir");
        }
        fs::write(
            home.join(".rustup/settings.toml"),
            "default_toolchain = \"stable-x86_64-unknown-linux-gnu\"\n",
        )
        .expect("settings");
        fs::write(home.join(".nvm/alias/default"), "20\n").expect("nvm default");
        fs::write(home.join(".pyenv/version"), "3.12.1\n").expect("pyenv default");
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            home.join(".sdkman/candidates/java/21.0.1-tem"),
            home.join(".sdkman/candidates/java/current"),
        )
        .expect("current");
        tmp
    }

    fn summary(found: &[Toolchain]) -> Vec<(&str, &str, Safety)> {
        found
            .iter()
            .map(|toolchain| {
                (
                    toolchain.tool.name(),
                    toolchain.version.as_str(),
                    toolchain.safety(),
                )
            })
            .collect()
    }

    #[test]
    fn defaults_and_pins_are_never_safe() {
        let tmp = home();
        let mut found = find(&Locations::under(tmp.path()), &Tool::ALL);
        let project = tmp.path().join("apps/tool");
        apply_pins(
            &mut found,
            &[
                Pin {
                    runtime: "rust".to_owned(),
                    spec: "1.80.0".to_owned(),
                    file: project.join("rust-toolchain.toml"),
                },
                Pin {
                    runtime: "python".to_owned(),
                    spec: "3.12".to_owned(),
                    file: project.join(".python-version"),
                },
            ],
        );
        let rows = summary(&found);
        let expected = [
            ("rustup", "1.80.0-x86_64-unknown-linux-gnu", Safety::Review),
            ("rustup", "nightly-x86_64-unknown-linux-gnu", Safety::Safe),
            ("rustup", "stable-x86_64-unknown-linux-gnu", Safety::Review),
            ("nvm", "v18.17.0", Safety::Safe),
            ("nvm", "v20.11.0", Safety::Review),
            ("pyenv", "3.11.4", Safety::Safe),
            ("pyenv", "3.12.1", Safety::Review),
            ("uv", "cpython-3.12.1-linux-x86_64-gnu", Safety::Review),
        ];
        for row in expected {
            assert!(rows.contains(&row), "{row:?} in {rows:?}");
        }
        let stable = found
            .iter()
            .find(|toolchain| toolchain.version.starts_with("stable"))
            .expect("stable");
        assert_eq!(stable.note(), "default");
        let pinned = found
            .iter()
            .find(|toolchain| toolchain.version.starts_with("1.80.0"))
            .expect("pinned");
        assert!(pinned.note().starts_with("pinned by "), "{}", pinned.note());
        let go: Vec<&str> = found
            .iter()
            .filter(|toolchain| toolchain.tool == Tool::Go)
            .map(|toolchain| toolchain.version.as_str())
            .collect();
        assert_eq!(go, ["go1.21.4"]);
    }

    #[cfg(unix)]
    #[test]
    fn sdkman_current_is_the_default() {
        let tmp = home();
        let found = find(&Locations::under(tmp.path()), &[Tool::Sdkman]);
        assert_eq!(
            summary(&found),
            [
                ("sdkman", "17.0.8-tem", Safety::Safe),
                ("sdkman", "21.0.1-tem", Safety::Review),
            ]
        );
        assert!(found.iter().all(|toolchain| toolchain.runtime == "java"));
    }

    #[test]
    fn rustup_overrides_pin_their_toolchain() {
        let tmp = home();
        fs::write(
            tmp.path().join(".rustup/settings.toml"),
            "default_toolchain = \"stable\"\n[overrides]\n\"/work/old\" = \"nightly-x86_64-unknown-linux-gnu\"\n",
        )
        .expect("settings");
        let found = find(&Locations::under(tmp.path()), &[Tool::Rustup]);
        let nightly = found
            .iter()
            .find(|toolchain| toolchain.version.starts_with("nightly"))
            .expect("nightly");
        assert_eq!(nightly.pins, [PathBuf::from("/work/old")]);
        assert!(
            found
                .iter()
                .find(|toolchain| toolchain.version.starts_with("stable"))
                .is_some_and(|toolchain| toolchain.default)
        );
    }

    #[test]
    fn no_tool_folders_lists_nothing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        assert!(find(&Locations::under(tmp.path()), &Tool::ALL).is_empty());
    }

    #[test]
    fn measures_and_removes_a_plain_version_permanently() {
        let tmp = home();
        let locations = Locations::under(tmp.path());
        let version = tmp.path().join(".nvm/versions/node/v18.17.0");
        fs::create_dir_all(version.join("bin")).expect("bin");
        fs::write(version.join("bin/node"), vec![0u8; 8192]).expect("node");
        let entries = measure(find(&locations, &[Tool::Nvm]));
        let (_, candidate) = entries
            .iter()
            .find(|(toolchain, _)| toolchain.version == "v18.17.0")
            .expect("v18");
        assert!(candidate.usage.apparent >= 8192);
        assert_eq!(candidate.rule.name, "nvm");
        assert_eq!(candidate.rule.ecosystem, "node");
        let guard = Guard::new(&roots(std::slice::from_ref(candidate))).with_cwd(None);
        remove(&guard, candidate, Mode::DryRun).expect("dry run");
        assert!(version.exists());
        remove(&guard, candidate, Mode::Trash).expect("remove");
        assert!(!version.exists());
    }

    #[test]
    fn a_missing_uninstaller_fails_the_item() {
        let tmp = home();
        let entries = measure(find(&Locations::under(tmp.path()), &[Tool::Rustup]));
        let (_, candidate) = &entries[0];
        let guard = Guard::new(&roots(std::slice::from_ref(candidate))).with_cwd(None);
        let error = uninstall("kuizer-no-such-rustup", Tool::Rustup, "stable").unwrap_err();
        assert!(error.to_string().contains("not on PATH"), "{error}");
        remove(&guard, candidate, Mode::DryRun).expect("dry run only checks");
        assert!(candidate.path.exists());
    }

    #[test]
    fn measure_with_reports_every_toolchain_it_sized() {
        let tmp = home();
        let found = find(&Locations::under(tmp.path()), &[Tool::Rustup]);
        let total = found.len();
        let seen = std::sync::Mutex::new(Vec::new());
        let entries = measure_with(found, &Cancel::new(), |candidate| {
            seen.lock().expect("seen").push(candidate.path.clone());
        });
        let mut seen = seen.into_inner().expect("seen");
        seen.sort();
        let mut measured: Vec<PathBuf> = entries.iter().map(|(t, _)| t.path.clone()).collect();
        measured.sort();
        assert_eq!(total, 3);
        assert_eq!(seen, measured);
    }
}
