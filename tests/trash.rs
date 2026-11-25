#![cfg(target_os = "linux")]

use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use tempfile::TempDir;

fn kuizer(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kuizer"))
        .args(args)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .output()
        .expect("runs kuizer")
}

#[test]
fn lists_and_empties_the_home_trash() {
    let tmp = TempDir::new().expect("tempdir");
    let home = fs::canonicalize(tmp.path())
        .expect("canonicalize")
        .join("home");
    let trash = home.join(".local/share/Trash");
    fs::create_dir_all(trash.join("files/dir")).expect("files");
    fs::create_dir_all(trash.join("info")).expect("info");
    fs::write(trash.join("files/a.txt"), vec![b'x'; 4096]).expect("a.txt");
    fs::write(trash.join("files/dir/b.txt"), vec![b'y'; 4096]).expect("b.txt");
    fs::write(trash.join("info/a.txt.trashinfo"), b"[Trash Info]\n").expect("a info");
    fs::write(trash.join("info/dir.trashinfo"), b"[Trash Info]\n").expect("dir info");
    let outside = home.join("keep.txt");
    fs::write(&outside, b"untouched").expect("keep");

    let listed = kuizer(&home, &["trash", "--json"]);
    assert!(listed.status.success(), "{listed:?}");
    let report: serde_json::Value =
        serde_json::from_slice(&listed.stdout).expect("valid trash JSON");
    assert_eq!(report["command"], "trash");
    assert_eq!(report["folders"].as_array().expect("folders").len(), 1);
    assert_eq!(report["folders"][0]["items"], 2);

    let cleaned = kuizer(&home, &["trash", "--clean", "--yes"]);
    assert!(cleaned.status.success(), "{cleaned:?}");
    assert_eq!(fs::read_dir(trash.join("files")).expect("files").count(), 0);
    assert_eq!(fs::read_dir(trash.join("info")).expect("info").count(), 0);
    assert!(outside.exists());

    let history = kuizer(&home, &["history", "--json"]);
    assert!(history.status.success(), "{history:?}");
    let history: serde_json::Value =
        serde_json::from_slice(&history.stdout).expect("valid history JSON");
    assert_eq!(history["runs"].as_array().expect("runs").len(), 1);
}
