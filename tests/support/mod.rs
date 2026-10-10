#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use kuizer::config::Config;
use kuizer::scan::{self, Cancel, Candidate, Collected};

pub fn file(root: &Path, relative: &str, bytes: usize) -> PathBuf {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("has parent")).expect("create parent");
    fs::write(&path, vec![b'x'; bytes]).expect("write file");
    path
}

pub fn dir(root: &Path, relative: &str) -> PathBuf {
    let path = root.join(relative);
    fs::create_dir_all(&path).expect("create dir");
    path
}

pub fn cachedir_tag(dir: &Path) {
    fs::create_dir_all(dir).expect("create dir");
    fs::write(
        dir.join("CACHEDIR.TAG"),
        "Signature: 8a477f597d28d172789f06886806bc55\n# created by a test\n",
    )
    .expect("write tag");
}

pub fn run(config: &Config, root: &Path) -> Collected {
    let handle = scan::start(config, &[root.to_path_buf()], Cancel::new()).expect("scan starts");
    scan::collect(&handle, |_| {})
}

pub fn scan(root: &Path) -> Collected {
    run(&Config::default(), root)
}

pub fn found(collected: &Collected, root: &Path) -> Vec<(String, String)> {
    let root = fs::canonicalize(root).expect("canonical root");
    let mut out: Vec<(String, String)> = collected
        .candidates
        .iter()
        .map(|candidate: &Candidate| {
            (
                candidate
                    .path
                    .strip_prefix(&root)
                    .expect("inside root")
                    .display()
                    .to_string(),
                candidate.rule.name.clone(),
            )
        })
        .collect();
    out.sort();
    out
}

pub fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
    items
        .iter()
        .map(|(path, rule)| ((*path).to_owned(), (*rule).to_owned()))
        .collect()
}
