#![cfg(all(unix, not(target_os = "macos")))]

mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use serde_json::Value;
use support::file;
use tempfile::TempDir;

fn kuizer(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kuizer"))
        .args(args)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn json(output: &Output) -> Value {
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

fn setup() -> (TempDir, PathBuf, PathBuf) {
    let tmp = TempDir::new().unwrap();
    let base = fs::canonicalize(tmp.path()).unwrap();
    let home = base.join("home");
    let root = base.join("work");
    fs::create_dir_all(home.join(".local/share")).unwrap();
    file(&root, "web/package.json", 2);
    file(&root, "web/node_modules/react/index.js", 100);
    file(&root, "web/dist/app.js", 100);
    (tmp, home, root)
}

fn clean_all(home: &Path, root: &Path) {
    let output = kuizer(
        home,
        &["clean", root.to_str().unwrap(), "--select", "all", "--yes"],
    );
    assert!(output.status.success(), "{output:?}");
}

fn states(home: &Path, run: &str) -> Vec<(String, String)> {
    let report = json(&kuizer(home, &["--json", "history", "show", run]));
    let mut states: Vec<(String, String)> = report["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            let path = PathBuf::from(item["path"].as_str().unwrap());
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            (name, item["state"].as_str().unwrap().to_owned())
        })
        .collect();
    states.sort();
    states
}

fn item_id(home: &Path, run: &str, name: &str) -> String {
    let report = json(&kuizer(home, &["--json", "history", "show", run]));
    report["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["path"].as_str().unwrap().ends_with(name))
        .map(|item| item["id"].as_str().unwrap().to_owned())
        .unwrap()
}

fn statuses(report: &Value) -> Vec<String> {
    let mut statuses: Vec<String> = report["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["status"].as_str().unwrap().to_owned())
        .collect();
    statuses.sort();
    statuses
}

#[test]
fn restores_a_whole_session_and_records_it() {
    let (_tmp, home, root) = setup();
    clean_all(&home, &root);
    assert!(!root.join("web/node_modules").exists());

    let report = json(&kuizer(&home, &["--json", "restore", "s1"]));

    assert_eq!(statuses(&report), ["restored", "restored"]);
    assert!(root.join("web/node_modules/react/index.js").is_file());
    assert!(root.join("web/dist/app.js").is_file());
    assert_eq!(
        states(&home, "s1"),
        [
            ("dist".to_owned(), "restored".to_owned()),
            ("node_modules".to_owned(), "restored".to_owned())
        ]
    );
    let history = json(&kuizer(&home, &["--json", "history"]));
    assert_eq!(history["runs"][1]["action"], "restore");
    assert_eq!(history["cumulative"]["bytes"], 0);

    let again = json(&kuizer(&home, &["--json", "restore", "s1"]));
    assert_eq!(statuses(&again), ["skipped", "skipped"]);
}

#[test]
fn restores_one_item() {
    let (_tmp, home, root) = setup();
    clean_all(&home, &root);
    let dist = item_id(&home, "s1", "dist");

    let report = json(&kuizer(&home, &["--json", "restore", &dist]));

    assert_eq!(statuses(&report), ["restored"]);
    assert!(root.join("web/dist/app.js").is_file());
    assert!(!root.join("web/node_modules").exists());
}

#[test]
fn dry_run_restores_nothing() {
    let (_tmp, home, root) = setup();
    clean_all(&home, &root);

    let report = json(&kuizer(
        &home,
        &["--json", "restore", "--last", "--dry-run"],
    ));

    assert_eq!(statuses(&report), ["would-restore", "would-restore"]);
    assert!(!root.join("web/node_modules").exists());
    let history = json(&kuizer(&home, &["--json", "history"]));
    assert_eq!(history["runs"].as_array().unwrap().len(), 1);
}

#[test]
fn skips_items_already_emptied_from_the_trash() {
    let (_tmp, home, root) = setup();
    clean_all(&home, &root);
    let trash = home.join(".local/share/Trash");
    fs::remove_dir_all(trash.join("files/node_modules")).unwrap();
    fs::remove_file(trash.join("info/node_modules.trashinfo")).unwrap();

    let report = json(&kuizer(&home, &["--json", "restore", "s1"]));

    assert_eq!(statuses(&report), ["restored", "skipped"]);
    let skipped = report["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["status"] == "skipped")
        .unwrap();
    assert_eq!(skipped["reason"], "is no longer in the trash");
    assert!(root.join("web/dist/app.js").is_file());
}

#[test]
fn never_overwrites_a_reoccupied_path() {
    let (_tmp, home, root) = setup();
    clean_all(&home, &root);
    file(&root, "web/node_modules/new/index.js", 7);

    let report = json(&kuizer(&home, &["--json", "restore", "s1"]));

    assert_eq!(statuses(&report), ["restored", "skipped"]);
    assert!(root.join("web/node_modules/new/index.js").is_file());
    assert!(!root.join("web/node_modules/react").exists());
}

#[test]
fn permanent_deletions_cannot_be_restored() {
    let (_tmp, home, root) = setup();
    let output = kuizer(
        &home,
        &["clean", root.to_str().unwrap(), "--permanent", "--yes"],
    );
    assert!(output.status.success(), "{output:?}");

    let report = json(&kuizer(&home, &["--json", "restore", "s1"]));

    assert_eq!(statuses(&report), ["skipped"]);
    assert_eq!(report["items"][0]["reason"], "was deleted permanently");
}

#[test]
fn restores_the_copy_from_the_requested_session() {
    let (_tmp, home, root) = setup();
    fs::write(root.join("web/node_modules/marker"), "first").unwrap();
    clean_all(&home, &root);
    std::thread::sleep(Duration::from_secs(2));
    file(&root, "web/node_modules/react/index.js", 100);
    fs::write(root.join("web/node_modules/marker"), "second").unwrap();
    clean_all(&home, &root);

    let node_modules = item_id(&home, "s1", "node_modules");
    let report = json(&kuizer(&home, &["--json", "restore", &node_modules]));

    assert_eq!(statuses(&report), ["restored"]);
    assert_eq!(
        fs::read_to_string(root.join("web/node_modules/marker")).unwrap(),
        "first"
    );
}

#[test]
fn unknown_ids_are_an_error() {
    let (_tmp, home, root) = setup();
    clean_all(&home, &root);

    let output = kuizer(&home, &["restore", "s9"]);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no run or item `s9`"));
}
