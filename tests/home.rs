#![cfg(unix)]

mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use support::file;
use tempfile::TempDir;

fn node_project(root: &Path, relative: &str) {
    file(root, &format!("{relative}/package.json"), 2);
    file(root, &format!("{relative}/node_modules/pkg/index.js"), 10);
}

fn scan(home: &Path, root: &Path) -> Vec<(String, String)> {
    let output = Command::new(env!("CARGO_BIN_EXE_kuizer"))
        .args(["--json", "scan", "--no-live"])
        .arg(root)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .output()
        .expect("runs kuizer");
    assert!(output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json output");
    let root = fs::canonicalize(root).expect("canonical root");
    let mut found: Vec<(String, String)> = report["candidates"]
        .as_array()
        .expect("candidates")
        .iter()
        .map(|candidate| {
            let path = PathBuf::from(candidate["path"].as_str().expect("path"));
            (
                path.strip_prefix(&root)
                    .expect("inside root")
                    .display()
                    .to_string(),
                candidate["safety"].as_str().expect("safety").to_owned(),
            )
        })
        .collect();
    found.sort();
    found
}

fn real_home() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().expect("tempdir");
    let home = fs::canonicalize(tmp.path())
        .expect("canonical")
        .join("home");
    fs::create_dir_all(&home).expect("home");
    (tmp, home)
}

#[test]
fn global_tools_and_extensions_are_skipped() {
    let (_tmp, home) = real_home();
    node_project(&home, ".vscode/extensions/esbenp.prettier-vscode-11.0.0");
    node_project(&home, ".cursor/extensions/some.ext-1.0.0");
    node_project(&home, ".local/share/pnpm/global/5");
    node_project(&home, ".bun/install/global");
    node_project(&home, ".config/yarn/global");
    node_project(&home, "code/web");

    assert_eq!(
        scan(&home, &home),
        vec![("code/web/node_modules".to_owned(), "safe".to_owned())]
    );
}

#[test]
fn matches_under_hidden_home_dirs_need_review() {
    let (_tmp, home) = real_home();
    node_project(&home, ".config/some-tool");
    node_project(&home, "code/.drafts/web");

    assert_eq!(
        scan(&home, &home),
        vec![
            (
                ".config/some-tool/node_modules".to_owned(),
                "review".to_owned()
            ),
            (
                "code/.drafts/web/node_modules".to_owned(),
                "safe".to_owned()
            ),
        ]
    );
}
