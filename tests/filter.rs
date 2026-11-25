#![cfg(unix)]

mod support;

use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use support::file;
use tempfile::TempDir;

#[cfg(target_os = "macos")]
const PIP_CACHE: &str = "Library/Caches/pip/http/b";
#[cfg(not(target_os = "macos"))]
const PIP_CACHE: &str = ".cache/pip/http/b";

fn kuizer(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kuizer"))
        .args(args)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
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

fn json(output: &Output) -> serde_json::Value {
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).expect("valid json")
}

fn lines(output: &Output) -> Vec<serde_json::Value> {
    assert!(output.status.success(), "{output:?}");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| serde_json::from_str(line).expect("one object per line"))
        .collect()
}

fn paths(rows: &serde_json::Value, root: &Path) -> Vec<String> {
    let mut paths: Vec<String> = rows
        .as_array()
        .expect("array")
        .iter()
        .map(|row| {
            Path::new(row["path"].as_str().expect("path"))
                .strip_prefix(root)
                .expect("inside root")
                .display()
                .to_string()
        })
        .collect();
    paths.sort();
    paths
}

fn setup() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
    let tmp = TempDir::new().unwrap();
    let base = fs::canonicalize(tmp.path()).unwrap();
    let home = base.join("home");
    let root = base.join("work");
    fs::create_dir_all(&home).unwrap();
    file(&root, "web/package.json", 2);
    file(&root, "web/node_modules/react/index.js", 100);
    file(&root, "web/dist/app.js", 100);
    file(&root, "api/package.json", 2);
    file(&root, "api/node_modules/express/index.js", 100);
    (tmp, home, root)
}

#[test]
fn scan_and_clean_dry_run_consider_the_same_filtered_items() {
    let (_tmp, home, root) = setup();
    let root_arg = root.to_str().unwrap();

    for (filters, expected) in [
        (
            &["--rule", "node-modules"][..],
            &["api/node_modules", "web/node_modules"][..],
        ),
        (&["--project", "api"][..], &["api/node_modules"][..]),
        (
            &["--exclude", "node_modules", "--exclude", "api"][..],
            &["web/dist"][..],
        ),
        (
            &["--rule", "node-modules", "--project", "web"][..],
            &["web/node_modules"][..],
        ),
    ] {
        let scan: Vec<&str> = ["--json", "scan", root_arg]
            .into_iter()
            .chain(filters.iter().copied())
            .collect();
        let listed = paths(&json(&kuizer(&home, &scan))["candidates"], &root);
        assert_eq!(listed, expected, "{filters:?}");

        let clean: Vec<&str> = ["--json", "clean", root_arg, "--select", "all", "--dry-run"]
            .into_iter()
            .chain(filters.iter().copied())
            .collect();
        let considered = paths(&json(&kuizer(&home, &clean))["items"], &root);
        assert_eq!(considered, listed, "{filters:?}");
    }
    assert!(root.join("api/node_modules").exists());

    let unknown = kuizer(&home, &["scan", root_arg, "--rule", "nope"]);
    assert_eq!(unknown.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("unknown rule `nope`"));
}

#[test]
fn json_lines_print_each_candidate_then_one_summary() {
    let (_tmp, home, root) = setup();
    let root_arg = root.to_str().unwrap();

    for args in [
        &["--json-lines", "scan", root_arg, "--rule", "node-modules"][..],
        &[
            "--json-lines",
            "scan",
            root_arg,
            "--rule",
            "node-modules",
            "--limit",
            "5",
        ][..],
    ] {
        let mut rows = lines(&kuizer(&home, args));
        let summary = rows.pop().expect("summary line");
        assert_eq!(summary["type"], "summary", "{args:?}");
        assert_eq!(summary["schema_version"], 2);
        assert_eq!(summary["command"], "scan");
        assert!(summary["summary"]["progress"]["candidates"].is_u64());
        assert!(summary.get("candidates").is_none());
        assert_eq!(rows.len(), 2, "{args:?}");
        for row in &rows {
            assert_eq!(row["type"], "candidate");
            assert_eq!(row["source"], "projects");
            assert_eq!(row["rule"], "node-modules");
        }
    }

    let refused = kuizer(&home, &["--json-lines", "rules"]);
    assert_eq!(refused.status.code(), Some(1));
    let conflict = kuizer(&home, &["--json", "--json-lines", "scan", root_arg]);
    assert_eq!(conflict.status.code(), Some(2));
}

fn without(mut value: serde_json::Value, fields: &[&str]) -> serde_json::Value {
    let object = value.as_object_mut().expect("object");
    for field in fields {
        object.remove(*field);
    }
    value
}

#[test]
fn clean_dry_run_json_lines_match_the_json_report() {
    let (_tmp, home, root) = setup();
    let root_arg = root.to_str().unwrap();
    let args = [root_arg, "--select", "all", "--dry-run"];

    let report = json(&kuizer(
        &home,
        &[&["--json", "clean"][..], &args[..]].concat(),
    ));
    let mut rows = lines(&kuizer(
        &home,
        &[&["--json-lines", "clean"][..], &args[..]].concat(),
    ));
    let summary = rows.pop().expect("summary line");

    assert_eq!(summary["type"], "summary");
    assert_eq!(
        without(summary, &["type"]),
        without(report.clone(), &["items"])
    );
    let items = report["items"].as_array().expect("items");
    assert_eq!(rows.len(), 3);
    assert_eq!(rows.len(), items.len());
    for row in &rows {
        assert_eq!(row["type"], "removal");
        assert_eq!(row["status"], "would-remove");
        let item = items
            .iter()
            .find(|item| item["path"] == row["path"])
            .expect("the same item in the report");
        assert_eq!(
            without(row.clone(), &["type", "id"]),
            without(item.clone(), &["id"])
        );
    }
    assert!(root.join("web/node_modules/react/index.js").exists());
}

#[test]
fn clean_json_lines_print_one_line_per_removed_item() {
    let (_tmp, home, root) = setup();
    let root_arg = root.to_str().unwrap();

    for format in ["--json", "--json-lines"] {
        let unconfirmed = kuizer(&home, &[format, "clean", root_arg, "--select", "all"]);
        assert_eq!(unconfirmed.status.code(), Some(1), "{format}");
        assert!(unconfirmed.stdout.is_empty(), "{format}");
    }
    assert!(root.join("web/node_modules").exists());

    let mut rows = lines(&kuizer(
        &home,
        &[
            "--json-lines",
            "clean",
            root_arg,
            "--select",
            "all",
            "--yes",
        ],
    ));
    let summary = rows.pop().expect("summary line");
    assert_eq!(summary["type"], "summary");
    assert_eq!(summary["command"], "clean");
    assert_eq!(summary["summary"]["removed"], 3);
    assert_eq!(summary["summary"]["failed"], 0);
    assert!(summary.get("items").is_none());
    let removed = paths(&serde_json::Value::Array(rows.clone()), &root);
    assert_eq!(
        removed,
        ["api/node_modules", "web/dist", "web/node_modules"]
    );
    for row in &rows {
        assert_eq!(row["type"], "removal");
        assert_eq!(row["status"], "removed");
        assert!(!Path::new(row["path"].as_str().unwrap()).exists());
    }
    assert!(root.join("web/package.json").exists());
}

#[test]
fn cache_takes_the_same_filters_and_json_lines() {
    let (_tmp, home, _root) = setup();
    file(&home, ".npm/_cacache/index-v5/a", 100);
    file(&home, PIP_CACHE, 100);

    let mut rows = lines(&kuizer(&home, &["--json-lines", "cache", "--rule", "pip"]));
    let summary = rows.pop().expect("summary line");
    assert_eq!(summary["type"], "summary");
    assert_eq!(summary["command"], "cache");
    assert_eq!(summary["summary"]["items"], 1);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], "pip");

    let report = json(&kuizer(&home, &["--json", "cache", "--exclude", ".npm"]));
    let names: Vec<&str> = report["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["pip"]);

    let report = json(&kuizer(
        &home,
        &[
            "--json",
            "cache",
            "clean",
            "--rule",
            "pip",
            "--select",
            "all",
            "--dry-run",
        ],
    ));
    let rules: Vec<&str> = report["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["rule"].as_str().unwrap())
        .collect();
    assert_eq!(rules, ["pip"]);
    assert!(home.join(PIP_CACHE).exists());
}
