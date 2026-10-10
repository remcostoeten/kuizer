#![cfg(unix)]

mod support;

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::time::{Duration, Instant};

use kuizer::config::Config;
use kuizer::scan::{self, Cancel, IssueKind, measure};
use support::{file, found, pairs, scan};
use tempfile::TempDir;

#[test]
fn symlinks_are_never_followed() {
    let tmp = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let root = tmp.path();
    file(outside.path(), "p/package.json", 2);
    file(outside.path(), "p/node_modules/secret.js", 5000);
    symlink(outside.path().join("p"), root.join("linked-project")).unwrap();
    file(root, "real/package.json", 2);
    file(root, "real/node_modules/a.js", 10);
    symlink(outside.path(), root.join("real/node_modules/escape")).unwrap();
    symlink(
        outside.path().join("p/node_modules"),
        root.join("real/node_modules-link"),
    )
    .unwrap();

    let collected = scan(root);

    assert_eq!(
        found(&collected, root),
        pairs(&[("real/node_modules", "node-modules")])
    );
    let usage = collected.candidates[0].usage;
    assert!(
        usage.apparent < 5000,
        "symlink target was counted: {usage:?}"
    );
}

#[test]
fn symlinked_root_inside_scan_is_skipped() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    symlink("/", root.join("slash")).unwrap();
    let collected = scan(root);
    assert!(collected.candidates.is_empty());
    assert!(collected.summary.progress.dirs <= 2);
}

#[test]
fn hard_links_are_counted_once() {
    let tmp = TempDir::new().unwrap();
    let single = tmp.path().join("single");
    let linked = tmp.path().join("linked");
    file(&single, "a.bin", 50_000);
    let original = file(&linked, "a.bin", 50_000);
    fs::hard_link(&original, linked.join("b.bin")).unwrap();
    fs::hard_link(&original, linked.join("c.bin")).unwrap();

    let cancel = Cancel::new();
    let one = measure(&single, &cancel).usage;
    let many = measure(&linked, &cancel).usage;

    let dir_len = |path: &std::path::Path| fs::metadata(path).unwrap().len();
    assert_eq!(
        one.apparent - dir_len(&single),
        many.apparent - dir_len(&linked)
    );
    assert_eq!(one.allocated, many.allocated);
    assert_eq!(many.entries, 3);
}

fn permissions_enforced(probe: &std::path::Path) -> bool {
    fs::read_dir(probe).is_err()
}

#[test]
fn unreadable_directories_become_issues() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    file(root, "open/package.json", 2);
    file(root, "open/node_modules/a.js", 10);
    let locked = support::dir(root, "locked");
    file(root, "locked/package.json", 2);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();

    if !permissions_enforced(&locked) {
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        eprintln!("skipped: running with privileges that bypass permissions");
        return;
    }
    let collected = scan(root);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();

    assert_eq!(
        found(&collected, root),
        pairs(&[("open/node_modules", "node-modules")])
    );
    assert_eq!(collected.issues.len(), 1);
    assert_eq!(collected.issues[0].kind, IssueKind::PermissionDenied);
}

#[test]
fn unreadable_artifact_contents_mark_size_partial() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    file(root, "p/package.json", 2);
    file(root, "p/node_modules/dep/a.js", 10);
    let dep = root.join("p/node_modules/dep");
    fs::set_permissions(&dep, fs::Permissions::from_mode(0o000)).unwrap();
    if !permissions_enforced(&dep) {
        fs::set_permissions(&dep, fs::Permissions::from_mode(0o755)).unwrap();
        eprintln!("skipped: running with privileges that bypass permissions");
        return;
    }
    let collected = scan(root);
    fs::set_permissions(&dep, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(collected.candidates[0].usage.partial);
}

#[test]
fn cancellation_stops_a_large_scan_quickly() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    for project in 0..200 {
        for module in 0..20 {
            file(root, &format!("p{project}/src/m{module}/index.ts"), 1);
        }
    }
    let cancel = Cancel::new();
    let handle = scan::start(&Config::default(), &[root.to_path_buf()], cancel.clone()).unwrap();
    cancel.cancel();
    let started = Instant::now();
    let collected = scan::collect(&handle, |_| {});
    assert!(collected.summary.cancelled);
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn dropping_a_scan_does_not_hang() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    for project in 0..300 {
        file(root, &format!("p{project}/package.json"), 2);
        file(root, &format!("p{project}/node_modules/a.js"), 10);
    }
    let started = Instant::now();
    let handle = scan::start(&Config::default(), &[root.to_path_buf()], Cancel::new()).unwrap();
    drop(handle);
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn files_vanishing_mid_scan_do_not_crash() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    for project in 0..100 {
        file(&root, &format!("p{project}/package.json"), 2);
        file(&root, &format!("p{project}/node_modules/a/b/c.js"), 10);
    }
    let handle = scan::start(
        &Config::default(),
        std::slice::from_ref(&root),
        Cancel::new(),
    )
    .unwrap();
    for project in 0..100 {
        let _ = fs::remove_dir_all(root.join(format!("p{project}")));
    }
    let collected = scan::collect(&handle, |_| {});
    assert!(!collected.summary.cancelled);
}
