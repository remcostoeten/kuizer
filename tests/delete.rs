#![cfg(unix)]

mod support;

use std::fs;
use std::os::unix::fs::{MetadataExt, symlink};
use std::path::{Path, PathBuf};

use kuizer::delete::{DeleteError, Guard, Mode, Refusal, Target, delete};
use support::file;
use tempfile::TempDir;

fn root(tmp: &TempDir) -> PathBuf {
    fs::canonicalize(tmp.path()).unwrap()
}

fn target(path: &Path) -> Target {
    let metadata = fs::symlink_metadata(path).unwrap();
    Target {
        path: path.to_path_buf(),
        identity: Some((metadata.dev(), metadata.ino())),
    }
}

fn guard(root: &Path) -> Guard {
    Guard::new(&[root.to_path_buf()]).with_cwd(None)
}

fn refusal(result: Result<(), DeleteError>) -> Refusal {
    match result {
        Err(DeleteError::Refused(refusal)) => refusal,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn permanent_delete_removes_the_directory() {
    let tmp = TempDir::new().unwrap();
    let root = root(&tmp);
    file(&root, "web/package.json", 2);
    file(&root, "web/node_modules/a/index.js", 10);
    let modules = root.join("web/node_modules");

    delete(&guard(&root), &target(&modules), Mode::Permanent).unwrap();

    assert!(!modules.exists());
    assert!(root.join("web/package.json").exists());
}

#[test]
fn dry_run_touches_nothing() {
    let tmp = TempDir::new().unwrap();
    let root = root(&tmp);
    file(&root, "web/node_modules/a.js", 10);
    let modules = root.join("web/node_modules");

    delete(&guard(&root), &target(&modules), Mode::DryRun).unwrap();

    assert!(modules.join("a.js").exists());
}

#[test]
fn symlinks_inside_are_removed_not_followed() {
    let tmp = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let root = root(&tmp);
    let precious = file(outside.path(), "keep/precious.txt", 10);
    file(&root, "web/node_modules/a.js", 10);
    let modules = root.join("web/node_modules");
    symlink(outside.path().join("keep"), modules.join("escape")).unwrap();

    delete(&guard(&root), &target(&modules), Mode::Permanent).unwrap();

    assert!(!modules.exists());
    assert!(precious.exists());
}

#[test]
fn refuses_a_target_swapped_for_a_symlink() {
    let tmp = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let root = root(&tmp);
    let precious = file(outside.path(), "precious.txt", 10);
    file(&root, "web/node_modules/a.js", 10);
    let modules = root.join("web/node_modules");
    let scanned = target(&modules);

    fs::remove_dir_all(&modules).unwrap();
    symlink(outside.path(), &modules).unwrap();

    let result = delete(&guard(&root), &scanned, Mode::Permanent);
    assert_eq!(refusal(result), Refusal::Symlink);
    assert!(precious.exists());
}

#[test]
fn refuses_a_target_replaced_by_another_directory() {
    let tmp = TempDir::new().unwrap();
    let root = root(&tmp);
    file(&root, "web/node_modules/a.js", 10);
    let modules = root.join("web/node_modules");
    let scanned = target(&modules);
    file(&root, "web/elsewhere/keep.txt", 10);

    fs::remove_dir_all(&modules).unwrap();
    fs::rename(root.join("web/elsewhere"), &modules).unwrap();

    let result = delete(&guard(&root), &scanned, Mode::Permanent);
    assert_eq!(refusal(result), Refusal::Changed);
    assert!(modules.join("keep.txt").exists());
}

#[test]
fn refuses_roots_and_paths_outside_them() {
    let tmp = TempDir::new().unwrap();
    let other = TempDir::new().unwrap();
    let root = root(&tmp);
    let stray = file(other.path(), "x/y.txt", 1);

    assert_eq!(
        refusal(delete(&guard(&root), &target(&root), Mode::Permanent)),
        Refusal::Anchor
    );
    let stray_dir = fs::canonicalize(stray.parent().unwrap()).unwrap();
    assert_eq!(
        refusal(delete(&guard(&root), &target(&stray_dir), Mode::Permanent)),
        Refusal::OutsideRoots
    );
    assert!(root.exists() && stray.exists());
}

#[test]
fn refuses_a_parent_of_the_working_directory() {
    let tmp = TempDir::new().unwrap();
    let root = root(&tmp);
    file(&root, "web/node_modules/pkg/a.js", 1);
    let modules = root.join("web/node_modules");
    let guard = Guard::new(std::slice::from_ref(&root)).with_cwd(Some(modules.join("pkg")));

    assert_eq!(
        refusal(delete(&guard, &target(&modules), Mode::Permanent)),
        Refusal::ContainsCwd
    );
    assert!(modules.exists());
}

#[test]
fn missing_targets_are_reported() {
    let tmp = TempDir::new().unwrap();
    let root = root(&tmp);
    let gone = Target {
        path: root.join("web/node_modules"),
        identity: None,
    };
    assert_eq!(
        refusal(delete(&guard(&root), &gone, Mode::Permanent)),
        Refusal::Missing
    );
}

#[test]
fn refuses_paths_inside_app_profiles() {
    let tmp = TempDir::new().unwrap();
    let root = root(&tmp);
    file(&root, "discord/Local State", 2);
    file(&root, "discord/modules/node_modules/a.js", 10);
    let modules = root.join("discord/modules/node_modules");

    assert_eq!(
        refusal(delete(&guard(&root), &target(&modules), Mode::Permanent)),
        Refusal::AppData
    );
    assert!(modules.join("a.js").exists());
}
