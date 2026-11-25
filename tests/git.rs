use std::fs::File;
use std::process::Command;

use kuizer::git;

#[test]
fn uncommitted_changes_reads_more_than_a_pipe_buffer() {
    let repo = tempfile::tempdir().expect("temporary repository");
    let initialized = Command::new("git")
        .args(["init", "-q"])
        .current_dir(repo.path())
        .status()
        .expect("run git init");
    assert!(initialized.success());

    let count = 1_024;
    let suffix = "x".repeat(128);
    for index in 0..count {
        File::create(repo.path().join(format!("change-{index}-{suffix}"))).expect("untracked file");
    }
    assert_eq!(git::uncommitted_changes(repo.path()), Some(count));
}
