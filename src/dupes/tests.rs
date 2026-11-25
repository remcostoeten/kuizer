use super::walk::{Paths, Skip};
use crate::platform::dir::{EntryKind, Stat};
use crate::scan::walk::Visitor;
use std::fs;
use std::path::Path;

use super::*;
use crate::scan::{self, Cancel};

fn options() -> Options {
    Options {
        min_size: 1,
        threads: 2,
        verify: true,
        include_hidden: true,
    }
}

fn collect(root: &Path, options: Options) -> Files {
    let config = Config::default();
    let detector = config.detector().unwrap();
    let excludes = config.excludes().unwrap();
    let mounts = MountTable::load();
    let protected = Protected::default();
    let mut walker = Walker {
        scan: &config.scan,
        detector: &detector,
        excludes: &excludes,
        mounts: &mounts,
        protected: &protected,
        visitor: Files::with_skip(options, None),
    };
    walker.walk_root(root);
    walker.visitor
}

fn write(root: &Path, relative: &str, bytes: &[u8]) -> PathBuf {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn shared_scan_walk_finds_the_same_duplicates() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = fs::canonicalize(tmp.path()).unwrap();
    for relative in ["a/one.txt", "b/two.txt", "c/three.txt"] {
        write(&root, relative, b"identical contents");
    }
    fs::write(root.join("unique.txt"), b"something else").unwrap();
    let config = Config::default();
    let options = Options {
        min_size: 1,
        threads: 2,
        verify: true,
        include_hidden: true,
    };
    let roots = vec![root];

    let alone = find(&config, &roots, options, |_| {}).unwrap();
    let (handle, files) =
        scan::start_with(&config, &roots, Cancel::new(), Files::new(options)).unwrap();
    scan::collect(&handle, |_| {});
    let shared = group(files.recv().unwrap(), options, |_| {});

    let members = |found: &Duplicates| -> Vec<Vec<PathBuf>> {
        found
            .groups
            .iter()
            .map(|group| group.members.iter().map(|m| m.path.clone()).collect())
            .collect()
    };
    assert_eq!(members(&alone).len(), 1);
    assert_eq!(members(&alone), members(&shared));
    assert_eq!(alone.files, shared.files);
}

#[test]
fn same_samples_with_different_middles_are_not_duplicates() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = fs::canonicalize(tmp.path()).unwrap();
    let size = 200 * 1024;
    let base = vec![7u8; size];
    let mut changed = base.clone();
    changed[size / 4] = 9;
    write(&root, "a/one.bin", &base);
    write(&root, "b/two.bin", &base);
    write(&root, "c/three.bin", &changed);
    let options = Options {
        verify: false,
        ..options()
    };

    let found = group(collect(&root, options), options, |_| {});

    assert_eq!(found.groups.len(), 1);
    let names: Vec<_> = found.groups[0]
        .members
        .iter()
        .map(|member| member.path.strip_prefix(&root).unwrap().to_owned())
        .collect();
    assert_eq!(
        names,
        [PathBuf::from("a/one.bin"), PathBuf::from("b/two.bin")]
    );
}

#[test]
fn cancelling_returns_no_groups() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = fs::canonicalize(tmp.path()).unwrap();
    write(&root, "a/one.bin", &[1u8; 100 * 1024]);
    write(&root, "b/two.bin", &[1u8; 100 * 1024]);
    let files = collect(&root, options());
    let cancel = Cancel::new();
    cancel.cancel();

    let found = group_cancellable(files, options(), &cancel, |_| {});

    assert!(found.groups.is_empty());
}

#[test]
fn hidden_home_folders_and_tool_roots_are_skipped_unless_a_root_is_inside() {
    let tmp = tempfile::TempDir::new().unwrap();
    let home = fs::canonicalize(tmp.path()).unwrap();
    let protected = Protected::new(Some(&home));
    let tool = home.join("go/pkg/mod");
    let mut skip = Skip::new(protected.clone(), vec![tool.clone()], &[]);
    assert!(skip.skips(&home.join(".cache/pip/a.whl")));
    assert!(skip.skips(&tool.join("lib/a.go")));
    assert!(!skip.skips(&home.join("dev/app/a.go")));

    let mut lifted = Skip::new(protected, vec![tool], &[home.join(".config/app")]);
    assert!(!lifted.skips(&home.join(".config/app/a.toml")));
    assert!(lifted.skips(&home.join(".config/other/a.toml")));
}

#[test]
fn entering_a_skipped_folder_stops_collecting_its_files() {
    let tmp = tempfile::TempDir::new().unwrap();
    let home = fs::canonicalize(tmp.path()).unwrap();
    let skip = Skip::new(Protected::new(Some(&home)), Vec::new(), &[]);
    let mut files = Files::with_skip(options(), Some(skip));

    files.entered(&home.join(".cache/pip"));
    assert!(!files.wants_files());
    files.entered(&home.join("dev/app"));
    assert!(files.wants_files());
}

#[test]
fn standalone_walk_prunes_configured_tool_roots_before_reading_them() {
    let tmp = tempfile::TempDir::new().expect("temp directory");
    let root = fs::canonicalize(tmp.path()).expect("canonical root");
    let tool_root = root.join("managed-tools");
    write(&root, "visible.txt", b"visible");
    write(&root, "managed-tools/hidden.txt", b"hidden");
    let config = Config::default();
    let detector = config.detector().unwrap();
    let excludes = config.excludes().unwrap();
    let mounts = MountTable::from_mounts(Vec::new());
    let protected = Protected::default();
    let skip = Skip::new(protected.clone(), vec![tool_root], &[]);
    let mut walker = Walker {
        scan: &config.scan,
        detector: &detector,
        excludes: &excludes,
        mounts: &mounts,
        protected: &protected,
        visitor: Files::with_skip(options(), Some(skip)),
    };

    walker.walk_root(&root);

    assert_eq!(walker.visitor.counted, 1);
    assert_eq!(walker.visitor.paths.len(), 1);
}

#[test]
fn merged_path_stores_rebuild_every_path() {
    let mut left = Paths::default();
    let mut right = Paths::default();
    let stat = Stat {
        kind: EntryKind::File,
        device: 1,
        inode: 1,
        links: 1,
        len: 1,
        allocated: 1,
        modified: None,
    };
    for path in ["/a/x/one", "/a/x/two", "/a/y/three"] {
        left.push(Path::new(path), stat);
    }
    for path in ["/b/four", "/b/five"] {
        right.push(Path::new(path), stat);
    }
    left.append(right);

    let mut buffer = PathBuf::new();
    let rebuilt: Vec<PathBuf> = (0..left.len())
        .map(|index| {
            left.fill(index, &mut buffer);
            buffer.clone()
        })
        .collect();

    assert_eq!(
        rebuilt,
        ["/a/x/one", "/a/x/two", "/a/y/three", "/b/four", "/b/five"].map(PathBuf::from)
    );
}
