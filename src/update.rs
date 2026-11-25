//! Opt-in check for newer kuizer releases on GitHub.
//!
//! Disabled by default (`appearance.update_check = false`). When enabled, this is the only
//! network request kuizer makes, and it never downloads or installs binaries automatically.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::platform;

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const RELEASES_API_URL: &str = "https://api.github.com/repos/remcostoeten/kuizer/releases/latest";
const DEFAULT_RELEASE_URL: &str = "https://github.com/remcostoeten/kuizer/releases/latest";
const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateNotice {
    pub current: String,
    pub latest: String,
    pub url: String,
}

impl UpdateNotice {
    pub fn message(&self) -> String {
        format!(
            "Update available: v{} (current v{})",
            self.latest, self.current
        )
    }
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct CachedCheck {
    checked_at_secs: u64,
    latest_tag: Option<String>,
    url: Option<String>,
}

/// @name check_if_enabled
/// @description Checks GitHub releases for a newer stable version when `appearance.update_check`
/// is enabled. Returns `None` immediately when disabled, up to date, offline, or when `curl` is
/// unavailable. Never auto-installs anything.
///
/// @example
/// if let Some(notice) = kuizer::update::check_if_enabled(&config) {
///     eprintln!("{}", notice.message());
/// }
pub fn check_if_enabled(config: &Config) -> Option<UpdateNotice> {
    if !config.appearance.update_check {
        return None;
    }
    let cache = cache_path();
    if let Some(path) = cache.as_deref()
        && let Some(cached) = read_fresh_cache(path, SystemTime::now())
    {
        return notice_from_tag(
            CURRENT_VERSION,
            cached.latest_tag.as_deref()?,
            cached.url.as_deref(),
        );
    }

    let body = fetch_latest_release_json()?;
    let release = parse_github_release(&body)?;
    if let Some(path) = cache.as_deref() {
        write_cache(path, SystemTime::now(), &release);
    }
    notice_from_release(CURRENT_VERSION, &release)
}

/// @name parse_release
/// @description Parses a GitHub Releases API JSON payload and returns an [`UpdateNotice`] when
/// it describes a stable release newer than `current`.
///
/// @example
/// let notice = kuizer::update::parse_release("0.1.3", r#"{"tag_name":"v0.2.0"}"#);
/// assert_eq!(notice.unwrap().latest, "0.2.0");
pub fn parse_release(current: &str, body: &str) -> Option<UpdateNotice> {
    let release = parse_github_release(body)?;
    notice_from_release(current, &release)
}

fn parse_github_release(body: &str) -> Option<GitHubRelease> {
    serde_json::from_str::<GitHubRelease>(body).ok()
}

fn notice_from_release(current: &str, release: &GitHubRelease) -> Option<UpdateNotice> {
    if release.draft || release.prerelease {
        return None;
    }
    notice_from_tag(current, &release.tag_name, Some(release.html_url.as_str()))
}

fn notice_from_tag(current: &str, tag: &str, url: Option<&str>) -> Option<UpdateNotice> {
    let latest = strip_v(tag);
    if !is_newer(current, latest) {
        return None;
    }
    let url = url
        .map(str::trim)
        .filter(|value| value.starts_with("https://"))
        .unwrap_or(DEFAULT_RELEASE_URL);
    Some(UpdateNotice {
        current: strip_v(current).to_owned(),
        latest: latest.to_owned(),
        url: url.to_owned(),
    })
}

fn strip_v(version: &str) -> &str {
    let trimmed = version.trim();
    trimmed
        .strip_prefix('v')
        .or_else(|| trimmed.strip_prefix('V'))
        .unwrap_or(trimmed)
}

fn parse_semver(version: &str) -> Option<((u64, u64, u64), bool)> {
    let clean = strip_v(version);
    let without_build = clean.split('+').next()?;
    let (core, is_prerelease) = match without_build.split_once('-') {
        Some((core, _)) => (core, true),
        None => (without_build, false),
    };
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(((major, minor, patch), is_prerelease))
}

/// @name is_newer
/// @description Returns whether `latest` is a newer stable SemVer release than `current`.
///
/// @example
/// assert!(kuizer::update::is_newer("0.1.3", "v0.2.0"));
/// assert!(!kuizer::update::is_newer("0.1.3", "0.1.3"));
pub fn is_newer(current: &str, latest: &str) -> bool {
    let Some((cur, cur_pre)) = parse_semver(current) else {
        return false;
    };
    let Some((lat, lat_pre)) = parse_semver(latest) else {
        return false;
    };
    if lat_pre {
        return false;
    }
    lat > cur || (lat == cur && cur_pre)
}

fn fetch_latest_release_json() -> Option<String> {
    let output = Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--fail",
            "--max-time",
            "5",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            &format!("User-Agent: kuizer/{CURRENT_VERSION}"),
            RELEASES_API_URL,
        ])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

fn cache_path() -> Option<PathBuf> {
    Some(dirs::cache_dir()?.join("kuizer").join("update-check.json"))
}

fn now_secs(now: SystemTime) -> u64 {
    now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

fn read_fresh_cache(path: &Path, now: SystemTime) -> Option<CachedCheck> {
    let raw = fs::read_to_string(path).ok()?;
    let cached: CachedCheck = serde_json::from_str(&raw).ok()?;
    let age = now_secs(now).saturating_sub(cached.checked_at_secs);
    (age < CACHE_TTL.as_secs()).then_some(cached)
}

fn write_cache(path: &Path, now: SystemTime, release: &GitHubRelease) {
    let cached = CachedCheck {
        checked_at_secs: now_secs(now),
        latest_tag: (!release.draft && !release.prerelease).then(|| release.tag_name.clone()),
        url: (!release.html_url.is_empty()).then(|| release.html_url.clone()),
    };
    if let Ok(bytes) = serde_json::to_vec(&cached) {
        let _ = platform::write_atomic(path, &bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_config_never_returns_an_update() {
        let config = Config::default();
        assert!(!config.appearance.update_check);
        assert_eq!(check_if_enabled(&config), None);
    }

    #[test]
    fn compares_semver_versions_accurately() {
        assert!(is_newer("0.1.3", "v0.1.4"));
        assert!(is_newer("0.1.3", "0.2.0"));
        assert!(is_newer("0.1.3", "1.0.0"));
        assert!(is_newer("0.2.0-rc.1", "0.2.0"));
        assert!(!is_newer("0.1.3", "0.1.3"));
        assert!(!is_newer("0.2.0", "0.1.9"));
        assert!(!is_newer("0.1.3", "0.2.0-beta.1"));
        assert!(!is_newer("0.1.3", "not-a-version"));
    }

    #[test]
    fn parses_github_release_payload_and_skips_drafts_or_prereleases() {
        let newer = parse_release(
            "0.1.3",
            r#"{"tag_name":"v0.2.0","html_url":"https://github.com/remcostoeten/kuizer/releases/tag/v0.2.0","draft":false,"prerelease":false}"#,
        )
        .expect("newer release");
        assert_eq!(newer.current, "0.1.3");
        assert_eq!(newer.latest, "0.2.0");
        assert_eq!(
            newer.url,
            "https://github.com/remcostoeten/kuizer/releases/tag/v0.2.0"
        );
        assert_eq!(newer.message(), "Update available: v0.2.0 (current v0.1.3)");

        assert_eq!(
            parse_release(
                "0.1.3",
                r#"{"tag_name":"v0.2.0","draft":true,"prerelease":false}"#
            ),
            None
        );
        assert_eq!(
            parse_release(
                "0.1.3",
                r#"{"tag_name":"v0.2.0-rc.1","draft":false,"prerelease":true}"#
            ),
            None
        );
        assert_eq!(parse_release("0.2.0", r#"{"tag_name":"v0.2.0"}"#), None);
    }

    #[test]
    fn cache_round_trips_and_expires_after_ttl() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("update-check.json");
        let now = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let release = GitHubRelease {
            tag_name: "v0.3.0".to_owned(),
            html_url: "https://github.com/remcostoeten/kuizer/releases/tag/v0.3.0".to_owned(),
            draft: false,
            prerelease: false,
        };
        write_cache(&path, now, &release);

        let fresh = read_fresh_cache(&path, now + Duration::from_secs(60)).unwrap();
        assert_eq!(fresh.latest_tag.as_deref(), Some("v0.3.0"));
        assert!(read_fresh_cache(&path, now + CACHE_TTL + Duration::from_secs(1)).is_none());
    }
}
