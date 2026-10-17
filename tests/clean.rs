#![cfg(unix)]

mod support;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::SystemTime;

use support::file;
use tempfile::TempDir;

type Snapshot = BTreeMap<PathBuf, (bool, u64, Option<SystemTime>)>;

fn snapshot(root: &Path) -> Snapshot {
    let mut entries = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let entry = entry.unwrap();
            let metadata = entry.metadata().unwrap();
            if metadata.is_dir() {
                stack.push(entry.path());
            }
            entries.insert(
                entry.path(),
                (metadata.is_dir(), metadata.len(), metadata.modified().ok()),
            );
        }
    }
    entries
}

fn project(root: &Path) {
    file(root, "web/package.json", 2);
    file(root, "web/src/index.ts", 20);
    file(root, "web/node_modules/react/index.js", 100);
    file(root, "web/dist/app.js", 100);
}

fn kuizer(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kuizer"))
        .args(args)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env_remove("CARGO_HOME")
        .env_remove("BUN_INSTALL")
        .env_remove("GOPATH")
        .env_remove("GOMODCACHE")
        .env_remove("GRADLE_USER_HOME")
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .output()
        .expect("runs kuizer")
}

fn setup() -> (TempDir, PathBuf, PathBuf) {
    let tmp = TempDir::new().unwrap();
    let base = fs::canonicalize(tmp.path()).unwrap();
    let home = base.join("home");
    let root = base.join("work");
    fs::create_dir_all(&home).unwrap();
    project(&root);
    (tmp, home, root)
}

#[test]
fn dry_run_touches_nothing() {
    let (_tmp, home, root) = setup();
    let before = snapshot(&root);

    let output = kuizer(
        &home,
        &[
            "clean",
            root.to_str().unwrap(),
            "--select",
            "all",
            "--dry-run",
        ],
    );

    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("node_modules"), "{stdout}");
    assert!(stdout.contains("Dry run, nothing was removed"), "{stdout}");
    assert_eq!(snapshot(&root), before);
    assert!(!home.join(".kuizer-history.json").exists());
}

#[test]
fn refuses_without_confirmation_when_not_interactive() {
    let (_tmp, home, root) = setup();
    let before = snapshot(&root);

    let output = kuizer(&home, &["clean", root.to_str().unwrap(), "--permanent"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("--yes"));
    assert_eq!(snapshot(&root), before);
}

#[test]
fn removes_only_safe_items_by_default_and_records_history() {
    let (_tmp, home, root) = setup();

    let output = kuizer(
        &home,
        &["clean", root.to_str().unwrap(), "--permanent", "--yes"],
    );

    assert!(output.status.success(), "{output:?}");
    assert!(!root.join("web/node_modules").exists());
    assert!(root.join("web/dist/app.js").exists());
    assert!(root.join("web/src/index.ts").exists());
    let history = fs::read_to_string(home.join(".kuizer-history.json")).unwrap();
    assert!(history.contains("node_modules"));
    assert!(!history.contains("dist"));
}

#[test]
fn carries_legacy_sweep_history_into_the_new_file() {
    let (_tmp, home, root) = setup();
    let legacy = home.join(".sweep-history.json");
    let legacy_text = r#"{"runs":[{"started_at":"2026-01-01T00:00:00+00:00","finished_at":"2026-01-01T00:00:01+00:00","command":"sweep clean","roots":[],"deleted":[],"total":{"items":0,"bytes":0,"size":"0 B"}}]}"#;
    fs::write(&legacy, legacy_text).unwrap();

    let output = kuizer(
        &home,
        &["clean", root.to_str().unwrap(), "--permanent", "--yes"],
    );

    assert!(output.status.success(), "{output:?}");
    assert_eq!(fs::read_to_string(&legacy).unwrap(), legacy_text);
    let history = fs::read_to_string(home.join(".kuizer-history.json")).unwrap();
    assert!(history.contains("sweep clean"), "{history}");
    assert!(history.contains("node_modules"), "{history}");
}

#[test]
fn select_all_includes_review_items_and_reports_json() {
    let (_tmp, home, root) = setup();

    let output = kuizer(
        &home,
        &[
            "--json",
            "clean",
            root.to_str().unwrap(),
            "--select",
            "all",
            "--permanent",
            "-y",
        ],
    );

    assert!(output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["command"], "clean");
    assert_eq!(report["mode"], "permanent");
    assert_eq!(report["summary"]["removed"], 2);
    assert_eq!(report["summary"]["failed"], 0);
    assert!(!root.join("web/dist").exists());
    assert!(root.join("web/src/index.ts").exists());
}

#[test]
fn select_by_rule_and_reject_unknown_rules() {
    let (_tmp, home, root) = setup();

    let output = kuizer(
        &home,
        &[
            "clean",
            root.to_str().unwrap(),
            "--select",
            "js-dist",
            "--permanent",
            "-y",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    assert!(!root.join("web/dist").exists());
    assert!(root.join("web/node_modules").exists());

    let output = kuizer(
        &home,
        &["clean", root.to_str().unwrap(), "--select", "nope", "-y"],
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown selection `nope`"));
    assert!(root.join("web/node_modules").exists());
}

#[test]
fn never_touches_browser_or_electron_profiles() {
    let (_tmp, home, root) = setup();
    let app = root.join("discord");
    file(&app, "Local State", 2);
    file(&app, "Cookies", 2);
    file(&app, "modules/voice/package.json", 2);
    file(&app, "modules/voice/node_modules/x/index.js", 100);
    let profile = root.join("browser/Default");
    file(&profile, "Cookies", 2);
    file(&profile, "Preferences", 2);
    file(&profile, "Extensions/abc/package.json", 2);
    file(&profile, "Extensions/abc/node_modules/y.js", 100);
    let before_app = snapshot(&app);
    let before_profile = snapshot(&profile);

    let output = kuizer(
        &home,
        &[
            "clean",
            root.to_str().unwrap(),
            "--select",
            "all",
            "--permanent",
            "-y",
        ],
    );

    assert!(output.status.success(), "{output:?}");
    assert!(!root.join("web/node_modules").exists());
    assert_eq!(snapshot(&app), before_app);
    assert_eq!(snapshot(&profile), before_profile);

    let output = kuizer(
        &home,
        &["clean", app.to_str().unwrap(), "--select", "all", "-y"],
    );
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("Nothing to remove"));
    assert_eq!(snapshot(&app), before_app);
}

#[cfg(target_os = "macos")]
const PIP_CACHE: &str = "Library/Caches/pip/http/b";
#[cfg(not(target_os = "macos"))]
const PIP_CACHE: &str = ".cache/pip/http/b";

#[test]
fn cache_lists_and_cleans_known_caches_only() {
    let (_tmp, home, _root) = setup();
    file(&home, ".npm/_cacache/index-v5/a", 100);
    file(&home, PIP_CACHE, 100);
    file(
        &home,
        ".cache/BraveSoftware/Brave-Browser/Default/Cache/c",
        100,
    );
    file(&home, ".config/BraveSoftware/Brave-Browser/Local State", 2);
    let brave = snapshot(&home.join(".config/BraveSoftware"));
    let brave_cache = snapshot(&home.join(".cache/BraveSoftware"));

    let output = kuizer(&home, &["--json", "cache"]);
    assert!(output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let names: Vec<&str> = report["caches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|cache| cache["name"].as_str().unwrap())
        .collect();
    assert_eq!(names.len(), 2, "{names:?}");
    assert!(names.contains(&"npm") && names.contains(&"pip"));

    let output = kuizer(
        &home,
        &["cache", "--clean", "--select", "npm", "--permanent", "-y"],
    );
    assert!(output.status.success(), "{output:?}");
    assert!(!home.join(".npm/_cacache").exists());
    assert!(home.join(".npm").exists());
    assert!(home.join(PIP_CACHE).exists());
    assert_eq!(snapshot(&home.join(".config/BraveSoftware")), brave);
    assert_eq!(snapshot(&home.join(".cache/BraveSoftware")), brave_cache);
}
