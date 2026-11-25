#![cfg(unix)]

use std::fs::{self, File};
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
fn big_lists_and_never_deletes() {
    let tmp = TempDir::new().expect("tempdir");
    let home = fs::canonicalize(tmp.path())
        .expect("canonicalize")
        .join("home");
    let video = home.join("Videos/huge.mkv");
    fs::create_dir_all(video.parent().expect("parent")).expect("mkdir");
    let file = File::create(&video).expect("create");
    file.set_len(600_000_000).expect("set_len");

    let output = kuizer(&home, &["big", "--json", home.to_str().expect("utf8")]);
    assert!(output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("valid big JSON");
    assert_eq!(report["command"], "big");
    let candidates = report["candidates"].as_array().expect("candidates");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0]["category"], "files");
    assert_eq!(candidates[0]["safety"], "review");
    assert_eq!(candidates[0]["apparent_bytes"], 600_000_000);
    assert!(video.exists());
}
