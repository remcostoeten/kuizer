use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;
use std::time::Duration;

use super::{Item, Manager, SystemConfig};
use crate::detect::Safety;
use crate::process;
use crate::scan::{self, Cancel};
use crate::units::ByteSize;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Listing {
    pub items: Vec<Item>,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Tools {
    pub path: Vec<PathBuf>,
    pub pacman_conf: PathBuf,
    pub snaps_dir: PathBuf,
    pub apt_cache_dir: PathBuf,
    pub dnf_cache_dirs: Vec<PathBuf>,
    pub zypper_cache_dir: PathBuf,
}

impl Tools {
    /// @name Tools::for_user
    /// @description Builds default system tool search paths and cache locations from the environment.
    ///
    /// @example
    /// let tools = Tools::for_user();
    pub fn for_user() -> Self {
        let path = std::env::var_os("PATH")
            .map(|value| std::env::split_paths(&value).collect())
            .unwrap_or_default();
        Self {
            path,
            pacman_conf: PathBuf::from("/etc/pacman.conf"),
            snaps_dir: PathBuf::from("/var/lib/snapd/snaps"),
            apt_cache_dir: PathBuf::from("/var/cache/apt/archives"),
            dnf_cache_dirs: vec![
                PathBuf::from("/var/cache/libdnf5"),
                PathBuf::from("/var/cache/dnf"),
            ],
            zypper_cache_dir: PathBuf::from("/var/cache/zypp/packages"),
        }
    }
}

/// @name for_user
/// @description Lists reclaimable system package caches, disabled snap revisions, and systemd
/// journal usage using the current environment's tools without modifying the machine.
///
/// @example
/// let listing = system::for_user(&config.system);
pub fn for_user(config: &SystemConfig) -> Listing {
    list(config, &Tools::for_user())
}

/// @name list
/// @description Runs read-only inspections for each configured and installed system manager.
///
/// @example
/// let listing = system::list(&SystemConfig::default(), &Tools::for_user());
pub fn list(config: &SystemConfig, tools: &Tools) -> Listing {
    let mut listing = Listing::default();
    let timeout = config.timeout();

    for &manager in &config.managers {
        match manager {
            Manager::Pacman => list_pacman(tools, &mut listing),
            Manager::Apt => list_apt(tools, &mut listing),
            Manager::Dnf => list_dnf(tools, &mut listing),
            Manager::Zypper => list_zypper(tools, &mut listing),
            Manager::Flatpak => list_flatpak(tools, &mut listing),
            Manager::Snap => list_snap(tools, timeout, &mut listing),
            Manager::Homebrew => list_homebrew(tools, timeout, &mut listing),
            Manager::Journal => list_journal(tools, timeout, &mut listing),
        }
    }

    listing
        .items
        .retain(|item| item.bytes > 0 || !item.size_known);
    listing.items.sort_by(|a, b| {
        b.bytes
            .cmp(&a.bytes)
            .then_with(|| a.manager.cmp(&b.manager))
            .then_with(|| a.id.cmp(&b.id))
    });
    listing
}

fn find_program(tools: &Tools, name: &str) -> Option<PathBuf> {
    for dir in &tools.path {
        let candidate = dir.join(name);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn run_program(program: &Path, args: &[&str], timeout: Duration) -> Result<String, String> {
    let output = process::run(Command::new(program).args(args), timeout).map_err(|error| {
        match error.kind() {
            io::ErrorKind::TimedOut => "did not answer in time".to_owned(),
            _ => error.to_string(),
        }
    })?;
    if output.status.success() {
        String::from_utf8(output.stdout).map_err(|error| error.to_string())
    } else {
        let message = String::from_utf8_lossy(&output.stderr)
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or("command failed")
            .to_owned();
        Err(message)
    }
}

fn measure_dirs(dirs: &[PathBuf]) -> u64 {
    let cancel = Cancel::new();
    let mut total = 0u64;
    for dir in dirs {
        if dir.is_dir() {
            let measured = scan::measure(dir, &cancel);
            total = total.saturating_add(measured.usage.allocated);
        }
    }
    total
}

fn pacman_cache_dirs(conf: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(text) = fs::read_to_string(conf) {
        for raw in text.lines() {
            let line = raw.split('#').next().unwrap_or("").trim();
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            if key.trim().eq_ignore_ascii_case("CacheDir") {
                for part in value.split_whitespace() {
                    dirs.push(PathBuf::from(part));
                }
            }
        }
    }
    if dirs.is_empty() {
        dirs.push(PathBuf::from("/var/cache/pacman/pkg"));
    }
    dirs
}

fn list_pacman(tools: &Tools, listing: &mut Listing) {
    if find_program(tools, "pacman").is_none() {
        return;
    }
    let dirs = pacman_cache_dirs(&tools.pacman_conf);
    let bytes = measure_dirs(&dirs);
    listing.items.push(Item {
        manager: Manager::Pacman,
        id: "pacman-cache".to_owned(),
        label: "Pacman package cache".to_owned(),
        bytes,
        size_known: true,
        safety: Safety::Safe,
        needs_root: true,
        note: "Downloaded packages. Cleaning keeps the two newest versions of each, so you can still downgrade.".to_owned(),
    });
}

fn list_apt(tools: &Tools, listing: &mut Listing) {
    if find_program(tools, "apt-get").is_none() {
        return;
    }
    let bytes = measure_dirs(std::slice::from_ref(&tools.apt_cache_dir));
    listing.items.push(Item {
        manager: Manager::Apt,
        id: "apt-cache".to_owned(),
        label: "APT package cache".to_owned(),
        bytes,
        size_known: true,
        safety: Safety::Safe,
        needs_root: true,
        note: "Downloaded packages cached by APT.".to_owned(),
    });
}

fn list_dnf(tools: &Tools, listing: &mut Listing) {
    if find_program(tools, "dnf").is_none() {
        return;
    }
    let bytes = measure_dirs(&tools.dnf_cache_dirs);
    listing.items.push(Item {
        manager: Manager::Dnf,
        id: "dnf-cache".to_owned(),
        label: "DNF package cache".to_owned(),
        bytes,
        size_known: true,
        safety: Safety::Safe,
        needs_root: true,
        note: "Downloaded packages and metadata cached by DNF.".to_owned(),
    });
}

fn list_zypper(tools: &Tools, listing: &mut Listing) {
    if find_program(tools, "zypper").is_none() {
        return;
    }
    let bytes = measure_dirs(std::slice::from_ref(&tools.zypper_cache_dir));
    listing.items.push(Item {
        manager: Manager::Zypper,
        id: "zypper-cache".to_owned(),
        label: "Zypper package cache".to_owned(),
        bytes,
        size_known: true,
        safety: Safety::Safe,
        needs_root: true,
        note: "Downloaded packages cached by Zypper.".to_owned(),
    });
}

fn list_flatpak(tools: &Tools, listing: &mut Listing) {
    if find_program(tools, "flatpak").is_none() {
        return;
    }
    listing.items.push(Item {
        manager: Manager::Flatpak,
        id: "flatpak-unused".to_owned(),
        label: "Unused Flatpak runtimes".to_owned(),
        bytes: 0,
        size_known: false,
        safety: Safety::Safe,
        needs_root: false,
        note: "Runtimes no installed app uses. The size is known after removal.".to_owned(),
    });
}

fn list_snap(tools: &Tools, timeout: Duration, listing: &mut Listing) {
    let Some(program) = find_program(tools, "snap") else {
        return;
    };
    let output = match run_program(&program, &["list", "--all"], timeout) {
        Ok(output) => output,
        Err(message) => {
            listing.issues.push(format!("snap: {message}"));
            return;
        }
    };
    for line in output.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 6 {
            continue;
        }
        let notes = cols[5..].join(" ");
        if !notes
            .split(|c: char| c == ',' || c.is_whitespace())
            .any(|token| token == "disabled")
        {
            continue;
        }
        let name = cols[0];
        let rev = cols[2];
        let snap_file = tools.snaps_dir.join(format!("{name}_{rev}.snap"));
        let (bytes, size_known) = match fs::metadata(&snap_file) {
            Ok(meta) => (meta.len(), true),
            Err(_) => (0, false),
        };
        listing.items.push(Item {
            manager: Manager::Snap,
            id: format!("{name}@{rev}"),
            label: format!("Snap {name} (rev {rev})"),
            bytes,
            size_known,
            safety: Safety::Review,
            needs_root: true,
            note:
                "An older revision kept for rollback; you can no longer revert to it after removal."
                    .to_owned(),
        });
    }
}

fn list_homebrew(tools: &Tools, timeout: Duration, listing: &mut Listing) {
    let Some(program) = find_program(tools, "brew") else {
        return;
    };
    let output = match run_program(&program, &["cleanup", "--dry-run"], timeout) {
        Ok(output) => output,
        Err(message) => {
            listing.issues.push(format!("brew: {message}"));
            return;
        }
    };
    if let Some(bytes) = parse_brew_savings(&output) {
        listing.items.push(Item {
            manager: Manager::Homebrew,
            id: "homebrew-cleanup".to_owned(),
            label: "Homebrew old versions and downloads".to_owned(),
            bytes,
            size_known: true,
            safety: Safety::Safe,
            needs_root: false,
            note: "Old versions and downloads Homebrew no longer needs.".to_owned(),
        });
    }
}

fn list_journal(tools: &Tools, timeout: Duration, listing: &mut Listing) {
    let Some(program) = find_program(tools, "journalctl") else {
        return;
    };
    let output = match run_program(&program, &["--disk-usage"], timeout) {
        Ok(output) => output,
        Err(message) => {
            listing.issues.push(format!("journalctl: {message}"));
            return;
        }
    };
    let Some(bytes) = parse_journal_usage(&output) else {
        listing
            .issues
            .push("journalctl: could not read disk usage output".to_owned());
        return;
    };
    listing.items.push(Item {
        manager: Manager::Journal,
        id: "journal".to_owned(),
        label: "Systemd journal logs".to_owned(),
        bytes,
        size_known: true,
        safety: Safety::Review,
        needs_root: true,
        note: "System logs. Cleaning keeps the newest part, so recent problems can still be looked up.".to_owned(),
    });
}

fn parse_brew_savings(output: &str) -> Option<u64> {
    const MARKER: &str = "would free approximately ";
    for line in output.lines() {
        let Some(pos) = line.find(MARKER) else {
            continue;
        };
        let rest = &line[pos + MARKER.len()..];
        let token = rest
            .split(" of disk space")
            .next()
            .unwrap_or(rest)
            .trim()
            .trim_end_matches('.');
        if let Some(bytes) = parse_size_token(token, false) {
            return Some(bytes);
        }
    }
    None
}

fn parse_journal_usage(output: &str) -> Option<u64> {
    const MARKER: &str = "take up ";
    for line in output.lines() {
        let Some(pos) = line.find(MARKER) else {
            continue;
        };
        let rest = &line[pos + MARKER.len()..];
        let token = rest.split(" in").next().unwrap_or(rest).trim();
        if let Some(bytes) = parse_size_token(token, true) {
            return Some(bytes);
        }
    }
    None
}

fn parse_size_token(token: &str, binary_short: bool) -> Option<u64> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return None;
    }
    if binary_short {
        let split = trimmed
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(trimmed.len());
        let (number, unit) = trimmed.split_at(split);
        let mapped = match unit.trim() {
            "K" | "k" => format!("{number}KiB"),
            "M" | "m" => format!("{number}MiB"),
            "G" | "g" => format!("{number}GiB"),
            "T" | "t" => format!("{number}TiB"),
            _ => trimmed.to_owned(),
        };
        return ByteSize::from_str(&mapped).ok().map(|size| size.0);
    }
    ByteSize::from_str(trimmed).ok().map(|size| size.0)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write script");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod");
        path
    }

    fn tools_in(dir: &Path) -> Tools {
        Tools {
            path: vec![dir.to_path_buf()],
            pacman_conf: dir.join("pacman.conf"),
            snaps_dir: dir.join("snaps"),
            apt_cache_dir: dir.join("apt"),
            dnf_cache_dirs: vec![dir.join("dnf")],
            zypper_cache_dir: dir.join("zypp"),
        }
    }

    #[test]
    fn lists_pacman_cache_from_its_config() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let pkg = tmp.path().join("pkg");
        fs::create_dir_all(&pkg).expect("pkg dir");
        fs::write(pkg.join("foo.pkg.tar.zst"), vec![0u8; 2 * 1024 * 1024]).expect("pkg");
        script(tmp.path(), "pacman", "exit 0");
        fs::write(
            tmp.path().join("pacman.conf"),
            format!("[options]\nCacheDir = {}\n", pkg.display()),
        )
        .expect("conf");

        let listing = list(&SystemConfig::default(), &tools_in(tmp.path()));
        assert!(listing.issues.is_empty(), "{:?}", listing.issues);
        assert_eq!(listing.items.len(), 1);
        let item = &listing.items[0];
        assert_eq!(item.id, "pacman-cache");
        assert!(item.needs_root);
        assert_eq!(item.safety, Safety::Safe);
        assert!(item.bytes >= 2 * 1024 * 1024);
    }

    #[test]
    fn lists_disabled_snap_revisions() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let snaps = tmp.path().join("snaps");
        fs::create_dir_all(&snaps).expect("snaps dir");
        fs::write(snaps.join("firefox_1234.snap"), vec![1u8; 1024 * 1024]).expect("snap file");
        script(
            tmp.path(),
            "snap",
            "printf 'Name    Version  Rev   Tracking       Publisher  Notes\\n\
firefox 130.0    1235  latest/stable  mozilla    -\\n\
firefox 129.0    1234  latest/stable  mozilla    disabled\\n'",
        );

        let listing = list(&SystemConfig::default(), &tools_in(tmp.path()));
        assert!(listing.issues.is_empty(), "{:?}", listing.issues);
        assert_eq!(listing.items.len(), 1);
        let item = &listing.items[0];
        assert_eq!(item.id, "firefox@1234");
        assert_eq!(item.bytes, 1024 * 1024);
        assert_eq!(item.safety, Safety::Review);
        assert!(item.needs_root);
    }

    #[test]
    fn parses_journal_and_brew_sizes() {
        let tmp = tempfile::tempdir().expect("tempdir");
        script(
            tmp.path(),
            "journalctl",
            "printf 'Archived and active journals take up 1.5G in the file system.\\n'",
        );
        script(
            tmp.path(),
            "brew",
            "printf '==> This operation would free approximately 300MB of disk space.\\n'",
        );

        let listing = list(&SystemConfig::default(), &tools_in(tmp.path()));
        assert!(listing.issues.is_empty(), "{:?}", listing.issues);
        assert_eq!(listing.items.len(), 2);
        assert_eq!(listing.items[0].id, "journal");
        assert_eq!(listing.items[0].bytes, 1_610_612_736);
        assert_eq!(listing.items[1].id, "homebrew-cleanup");
        assert_eq!(listing.items[1].bytes, 300_000_000);
    }

    #[test]
    fn missing_tools_are_skipped_and_failures_are_issues() {
        let empty = tempfile::tempdir().expect("empty");
        let listing = list(&SystemConfig::default(), &tools_in(empty.path()));
        assert!(listing.items.is_empty());
        assert!(listing.issues.is_empty());

        let tmp = tempfile::tempdir().expect("tempdir");
        script(
            tmp.path(),
            "journalctl",
            "echo 'Permission denied' >&2\nexit 1",
        );
        let listing = list(&SystemConfig::default(), &tools_in(tmp.path()));
        assert!(listing.items.is_empty());
        assert_eq!(listing.issues.len(), 1);
        assert!(listing.issues[0].contains("Permission denied"));
    }
}
