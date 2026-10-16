#![cfg(unix)]

mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, SystemTime};

use support::file;
use tempfile::TempDir;

const DAY: Duration = Duration::from_secs(86_400);

fn aged(root: &Path, relative: &str, age: Duration) -> PathBuf {
    let path = file(root, relative, 10);
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(SystemTime::now() - age)
        .unwrap();
    path
}

fn kuizer(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kuizer"))
        .args(args)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("GIT_CEILING_DIRECTORIES", home.parent().unwrap())
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .output()
        .expect("runs kuizer")
}

fn git(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn setup() -> (TempDir, PathBuf, PathBuf) {
    let tmp = TempDir::new().unwrap();
    let base = fs::canonicalize(tmp.path()).unwrap();
    let home = base.join("home");
    let root = base.join("work");
    fs::create_dir_all(&home).unwrap();
    aged(&root, "old/package.json", DAY * 120);
    aged(&root, "old/src/index.ts", DAY * 100);
    aged(&root, "old/node_modules/react/index.js", Duration::ZERO);
    aged(&root, "fresh/package.json", DAY * 120);
    aged(&root, "fresh/src/index.ts", DAY * 2);
    aged(&root, "fresh/node_modules/react/index.js", DAY * 120);
    (tmp, home, root)
}

fn paths(output: &Output, key: &str) -> Vec<String> {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let mut paths: Vec<String> = json[key]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["path"].as_str().unwrap().to_owned())
        .collect();
    paths.sort();
    paths
}

#[test]
fn scan_and_clean_list_the_same_idle_projects() {
    let (_tmp, home, root) = setup();
    let root_arg = root.to_str().unwrap();
    let scanned = kuizer(&home, &["--json", "scan", root_arg, "--idle", "60d"]);
    let cleaned = kuizer(
        &home,
        &[
            "--json",
            "clean",
            root_arg,
            "--older-than",
            "60d",
            "--dry-run",
            "--select",
            "all",
        ],
    );
    let expected = vec![root.join("old/node_modules").display().to_string()];
    assert_eq!(paths(&scanned, "candidates"), expected);
    assert_eq!(paths(&cleaned, "items"), expected);
}

#[test]
fn reports_project_activity_and_sorts_by_it() {
    let (_tmp, home, root) = setup();
    let output = kuizer(
        &home,
        &["--json", "scan", root.to_str().unwrap(), "--sort", "idle"],
    );
    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let order: Vec<(&str, bool)> = json["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            (
                item["path"].as_str().unwrap(),
                item["project_last_active"].is_u64(),
            )
        })
        .collect();
    let old = root.join("old/node_modules").display().to_string();
    let fresh = root.join("fresh/node_modules").display().to_string();
    assert_eq!(order, vec![(old.as_str(), true), (fresh.as_str(), true)]);
}

#[test]
fn a_recent_commit_keeps_a_project_active() {
    let (_tmp, home, root) = setup();
    let old = root.join("old");
    let committed =
        git(&old, &["init", "-q"]) && git(&old, &["commit", "-q", "--allow-empty", "-m", "touch"]);
    if !committed {
        return;
    }
    let output = kuizer(
        &home,
        &["--json", "scan", root.to_str().unwrap(), "--idle", "60d"],
    );
    assert_eq!(paths(&output, "candidates"), Vec::<String>::new());
}

#[test]
fn rejects_a_duration_without_a_unit() {
    let (_tmp, home, root) = setup();
    let output = kuizer(&home, &["scan", root.to_str().unwrap(), "--idle", "60"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("60d"));
}
