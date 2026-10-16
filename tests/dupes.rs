mod support;

use std::fs;
use std::path::{Path, PathBuf};

use kuizer::config::Config;
use kuizer::dupes::{self, Duplicates, Options};
use support::file;
use tempfile::TempDir;

fn find(root: &Path, verify: bool) -> Duplicates {
    let options = Options {
        min_size: 1,
        threads: 2,
        verify,
    };
    dupes::find(&Config::default(), &[root.to_path_buf()], options, |_| {}).unwrap()
}

fn write(root: &Path, relative: &str, bytes: &[u8]) -> PathBuf {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, bytes).unwrap();
    path
}

fn relative(found: &Duplicates, root: &Path) -> Vec<Vec<String>> {
    let mut groups: Vec<Vec<String>> = found
        .groups
        .iter()
        .map(|group| {
            group
                .members
                .iter()
                .map(|member| {
                    member
                        .path
                        .strip_prefix(root)
                        .unwrap()
                        .display()
                        .to_string()
                })
                .collect()
        })
        .collect();
    groups.sort();
    groups
}

#[test]
fn groups_identical_contents_only() {
    let tmp = TempDir::new().unwrap();
    let root = fs::canonicalize(tmp.path()).unwrap();
    write(&root, "a/photo.jpg", b"same bytes here");
    write(&root, "b/photo copy.jpg", b"same bytes here");
    write(&root, "c/other.jpg", b"diff bytes here");
    write(&root, "empty-1", b"");
    write(&root, "empty-2", b"");
    let mut large_a = vec![7u8; 40 * 1024];
    let mut large_b = large_a.clone();
    large_a[39 * 1024] = 1;
    large_b[39 * 1024] = 2;
    write(&root, "large-a.bin", &large_a);
    write(&root, "large-b.bin", &large_b);
    write(&root, "large-c.bin", &large_b);

    let found = find(&root, false);

    assert_eq!(
        relative(&found, &root),
        [
            vec!["a/photo.jpg".to_owned(), "b/photo copy.jpg".to_owned()],
            vec!["large-b.bin".to_owned(), "large-c.bin".to_owned()],
        ]
    );
    let reclaimable: u64 = found.groups.iter().map(|group| group.allocated).sum();
    assert_eq!(found.reclaimable(), reclaimable);
}

#[cfg(unix)]
#[test]
fn hard_links_count_as_one_member() {
    let tmp = TempDir::new().unwrap();
    let root = fs::canonicalize(tmp.path()).unwrap();
    let original = write(&root, "one.txt", b"linked contents");
    fs::hard_link(&original, root.join("two.txt")).unwrap();

    assert!(find(&root, false).groups.is_empty());

    write(&root, "three.txt", b"linked contents");
    let found = find(&root, true);
    assert_eq!(found.groups.len(), 1);
    let group = &found.groups[0];
    assert_eq!(group.members.len(), 2);
    assert!(group.verified);
    let linked = group
        .members
        .iter()
        .find(|member| !member.links.is_empty())
        .expect("one member carries the hard link");
    assert_eq!(linked.links.len(), 1);
    assert_eq!(found.reclaimable(), group.allocated);
}

#[test]
fn skips_artifacts_and_app_profiles() {
    let tmp = TempDir::new().unwrap();
    let root = fs::canonicalize(tmp.path()).unwrap();
    file(&root, "web/package.json", 2);
    write(&root, "web/node_modules/a/index.js", b"module source");
    write(&root, "web/node_modules/b/index.js", b"module source");
    write(&root, "browser/Local State", b"{}");
    write(&root, "browser/Default/one", b"profile data");
    write(&root, "browser/Default/two", b"profile data");

    assert!(find(&root, false).groups.is_empty());
}

#[test]
fn respects_min_size() {
    let tmp = TempDir::new().unwrap();
    let root = fs::canonicalize(tmp.path()).unwrap();
    write(&root, "a", b"tiny");
    write(&root, "b", b"tiny");
    let options = Options {
        min_size: 4096,
        threads: 1,
        verify: false,
    };
    let found = dupes::find(&Config::default(), &[root], options, |_| {}).unwrap();
    assert!(found.groups.is_empty());
}

#[test]
fn skips_roots_on_excluded_filesystem_types() {
    let tmp = TempDir::new().unwrap();
    let root = fs::canonicalize(tmp.path()).unwrap();
    write(&root, "a/photo.jpg", b"same bytes here");
    write(&root, "b/photo.jpg", b"same bytes here");
    let Some(fs_type) = kuizer::platform::MountTable::load()
        .fs_type(&root)
        .map(str::to_owned)
    else {
        return;
    };
    let mut config = Config::default();
    config.scan.one_file_system = false;
    config.scan.excluded_fs_types = vec![fs_type];
    let options = Options {
        min_size: 1,
        threads: 2,
        verify: false,
    };
    let found = dupes::find(&config, std::slice::from_ref(&root), options, |_| {}).unwrap();
    assert!(found.groups.is_empty());
    assert!(
        found
            .issues
            .iter()
            .any(|issue| issue.kind == kuizer::scan::IssueKind::SkippedMount)
    );
}
