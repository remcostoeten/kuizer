use super::actions::{delete_ref_at, git, has_ref, succeeds};
use std::fs;
use std::process::{Command, Stdio};

use super::*;

fn run(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args([
            "-c",
            "init.defaultBranch=main",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn commit(dir: &Path, file: &str) -> bool {
    fs::write(dir.join(file), file).is_ok()
        && run(dir, &["add", file])
        && run(dir, &["commit", "-q", "-m", file])
}

fn repository(dir: &Path) -> bool {
    run(dir, &["init", "-q"]) && commit(dir, "a.txt")
}

#[test]
fn parses_count_objects_output() {
    let objects = parse_count(
        "count: 12\nsize: 48\nin-pack: 3000\npacks: 2\nsize-pack: 1200\n\
         prune-packable: 3\ngarbage: 1\nsize-garbage: 4\n",
    );
    assert_eq!(
        objects,
        Objects {
            loose: 12,
            loose_bytes: 48 * KIB,
            packs: 2,
            packed_bytes: 1200 * KIB,
            prunable: 3,
            garbage_bytes: 4 * KIB,
        }
    );
    assert_eq!(objects.gain(), 52 * KIB);
    assert_eq!(parse_count("nonsense\nsize: x\n"), Objects::default());
}

#[test]
fn lists_merged_branches_but_never_kept_or_checked_out_ones() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    if !repository(root) {
        return;
    }
    assert!(run(root, &["branch", "done"]));
    assert!(run(root, &["branch", "develop"]));
    assert!(run(root, &["checkout", "-q", "-b", "open"]));
    assert!(commit(root, "b.txt"));
    assert!(run(root, &["checkout", "-q", "-b", "current", "main"]));

    let found = inspect(root, &GitConfig::default()).expect("inspects");
    assert_eq!(found.default_branch.as_deref(), Some("main"));
    assert_eq!(found.current_branch.as_deref(), Some("current"));
    let names: Vec<&str> = found.merged.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["done"]);
    assert!(found.objects.loose > 0);
    assert!(found.worth(1));
    assert_eq!(found.busy, None);
}

#[test]
fn plain_folders_are_not_repositories() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert_eq!(
        inspect(dir.path(), &GitConfig::default()),
        Err(RepoError::NotRepository(dir.path().to_path_buf()))
    );
}

#[test]
fn deletes_merged_branches_after_checking_them_again() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    if !repository(root) {
        return;
    }
    assert!(run(root, &["branch", "done"]));
    assert!(run(root, &["branch", "moved"]));
    let config = GitConfig::default();
    let found = inspect(root, &config).expect("inspects");
    assert_eq!(found.merged.len(), 2);

    assert!(run(root, &["checkout", "-q", "moved"]));
    assert!(commit(root, "c.txt"));
    assert!(run(root, &["checkout", "-q", "main"]));

    let mut asked = found.merged.clone();
    asked.push(Branch {
        name: "main".to_owned(),
        commit: String::new(),
    });
    let pruned = delete_branches(&found, &asked, &config);
    let results: Vec<(&str, &Result<(), RepoError>)> = pruned
        .iter()
        .map(|pruned| (pruned.branch.name.as_str(), &pruned.result))
        .collect();
    assert_eq!(
        results,
        [
            ("done", &Ok(())),
            ("moved", &Err(RepoError::Changed("moved".to_owned()))),
            ("main", &Err(RepoError::Kept("main".to_owned()))),
        ]
    );
    assert!(!has_ref(root, None, "refs/heads/done"));
    assert!(has_ref(root, None, "refs/heads/moved"));
}

#[test]
fn busy_repositories_are_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    if !repository(root) {
        return;
    }
    assert!(run(root, &["branch", "done"]));
    let config = GitConfig::default();
    let found = inspect(root, &config).expect("inspects");
    fs::write(root.join(".git/index.lock"), "").expect("lock");
    assert_eq!(busy(&found.git_dir), Some(Busy::Locked));
    assert_eq!(
        collect_garbage(&found, &config),
        Err(RepoError::Busy(Busy::Locked))
    );
    let pruned = delete_branches(&found, &found.merged, &config);
    assert_eq!(pruned[0].result, Err(RepoError::Busy(Busy::Locked)));
    assert!(has_ref(root, None, "refs/heads/done"));
    fs::remove_file(root.join(".git/index.lock")).expect("unlock");
    fs::create_dir(root.join(".git/rebase-merge")).expect("rebase");
    assert_eq!(busy(&found.git_dir), Some(Busy::Rebase));
}

#[test]
fn collects_garbage_and_reports_the_gain() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    if !repository(root) {
        return;
    }
    for index in 0..20 {
        assert!(commit(root, &format!("{index}.txt")));
    }
    let config = GitConfig::default();
    let found = inspect(root, &config).expect("inspects");
    assert!(found.objects.loose >= 40);
    let collected = collect_garbage(&found, &config).expect("gc");
    assert!(collected.after < collected.before, "{collected:?}");
    let after = inspect(root, &config).expect("inspects");
    assert_eq!(after.objects.loose, 0);
    assert!(after.objects.packs >= 1);
}

#[test]
fn the_walk_finds_nested_repositories() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let nested = root.join("apps/kearn");
    fs::create_dir_all(&nested).expect("mkdir");
    fs::create_dir_all(root.join("notes")).expect("mkdir");
    if !run(&nested, &["init", "-q"]) || !run(root, &["init", "-q"]) {
        return;
    }
    let found = find(&Config::default(), &[root.to_path_buf()]).expect("walks");
    let root = fs::canonicalize(root).expect("canonicalize");
    assert_eq!(found, [root.clone(), root.join("apps/kearn")]);
}

#[test]
fn a_branch_that_moved_after_the_check_survives() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    if !repository(root) {
        return;
    }
    let run = |args: &[&str]| assert!(git(root, args).expect("git").status.success());
    let identity = ["-c", "user.name=t", "-c", "user.email=t@example.invalid"];
    let commit = |message: &str| {
        let mut args = identity.to_vec();
        args.extend(["commit", "--allow-empty", "-m", message]);
        run(&args);
    };
    commit("first");
    run(&["branch", "work"]);
    let checked = read(root, &["rev-parse", "refs/heads/work"]).expect("commit");
    run(&["checkout", "-q", "work"]);
    commit("unmerged");
    run(&["checkout", "-q", "-"]);
    assert!(matches!(
        delete_ref_at(root, "work", &checked),
        Err(RepoError::Changed(_))
    ));
    assert!(succeeds(
        root,
        &["rev-parse", "--verify", "refs/heads/work"]
    ));
}

#[test]
fn inspect_all_with_reports_each_root_kept_or_not() {
    let dir = tempfile::tempdir().expect("tempdir");
    let plain = dir.path().join("plain");
    let other = dir.path().join("other");
    fs::create_dir_all(&plain).expect("plain");
    fs::create_dir_all(&other).expect("other");
    let seen = Mutex::new(Vec::new());
    let found = inspect_all_with(
        vec![plain.clone(), other.clone()],
        &GitConfig::default(),
        &Cancel::new(),
        |root| seen.lock().expect("seen").push(root.to_path_buf()),
    );
    let mut seen = seen.into_inner().expect("seen");
    seen.sort();
    assert!(found.is_empty());
    assert_eq!(seen, [other, plain]);
}

#[test]
fn failed_inspections_are_reported_as_issues() {
    let dir = tempfile::tempdir().expect("tempdir");
    let gone = dir.path().join("gone");
    let inspected = inspect_all_collecting(
        vec![gone.clone()],
        &GitConfig::default(),
        &Cancel::new(),
        |_| {},
    );
    assert!(inspected.repositories.is_empty());
    assert_eq!(inspected.issues.len(), 1);
    assert_eq!(inspected.issues[0].path, gone);
    assert!(matches!(inspected.issues[0].kind, IssueKind::Vanished));
}

#[test]
fn reads_the_git_folder_like_git_does() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    if !repository(root) {
        return;
    }
    for index in 0..5 {
        assert!(commit(root, &format!("{index}.txt")));
    }
    assert!(run(root, &["branch", "done"]));
    assert!(run(root, &["branch", "develop"]));
    assert!(run(root, &["checkout", "-q", "-b", "open"]));
    assert!(commit(root, "b.txt"));
    assert!(run(root, &["checkout", "-q", "-b", "current", "main"]));
    assert!(run(root, &["repack", "-q"]));
    assert!(commit(root, "late.txt"));
    assert!(run(root, &["pack-refs", "--all"]));
    assert!(run(root, &["branch", "fresh", "main"]));
    assert!(run(
        root,
        &["update-ref", "refs/remotes/origin/main", "main"]
    ));
    assert!(run(
        root,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main"
        ]
    ));
    let git_dir = root.join(".git");
    fs::write(git_dir.join("objects/pack/tmp_pack_x"), [0; 3000]).expect("garbage");

    let config = GitConfig::default();
    let on_disk = OnDisk::read(&git_dir).expect("plain layout");
    let fast = inspect_with(root, git_dir.clone(), Some(&on_disk), &config).expect("fs");
    let slow = inspect_with(root, git_dir, None, &config).expect("git");
    assert_eq!(fast, slow);
    assert!(fast.objects.prunable > 0, "{:?}", fast.objects);
    assert!(fast.objects.packs > 0 && fast.objects.garbage_bytes > 0);
    assert_eq!(fast.default_branch.as_deref(), Some("main"));
    assert!(fast.merged.iter().any(|branch| branch.name == "fresh"));
}
