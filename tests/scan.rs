mod support;

use std::fs;

use kuizer::config::Config;
use kuizer::detect::Safety;
use kuizer::scan::{self, Cancel, IssueKind};
use kuizer::units::ByteSize;
use support::{cachedir_tag, dir, file, found, pairs, run, scan};
use tempfile::TempDir;

#[test]
fn finds_project_artifacts_with_context() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    file(root, "web/package.json", 2);
    file(root, "web/node_modules/react/index.js", 100);
    file(root, "web/.next/cache/a", 100);
    file(root, "web/dist/app.js", 100);
    file(root, "web/src/index.ts", 100);
    file(root, "web/tsconfig.tsbuildinfo", 10);
    file(root, "crate/Cargo.toml", 2);
    cachedir_tag(&root.join("crate/target"));
    file(root, "crate/target/debug/app", 100);
    file(root, "notes/dist/essay.md", 100);
    file(root, "notes/build/plan.md", 100);
    file(root, "notes/target/goals.md", 100);
    file(root, "notes/node_modules/x.js", 100);

    let collected = scan(root);

    assert_eq!(
        found(&collected, root),
        pairs(&[
            ("crate/target", "cargo-target"),
            ("web/.next", "next-build"),
            ("web/dist", "js-dist"),
            ("web/node_modules", "node-modules"),
            ("web/tsconfig.tsbuildinfo", "typescript-buildinfo"),
        ])
    );
    let dist = collected
        .candidates
        .iter()
        .find(|c| c.rule.name == "js-dist")
        .unwrap();
    assert_eq!(dist.rule.safety, Safety::Review);
    assert!(collected.issues.is_empty());
    assert!(!collected.summary.cancelled);
}

#[test]
fn artifacts_are_pruned_not_descended() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    file(root, "app/package.json", 2);
    file(root, "app/node_modules/dep/package.json", 2);
    file(root, "app/node_modules/dep/node_modules/inner/index.js", 50);
    file(root, "app/node_modules/dep/dist/index.js", 50);

    let collected = scan(root);

    assert_eq!(
        found(&collected, root),
        pairs(&[("app/node_modules", "node-modules")])
    );
    assert_eq!(collected.candidates[0].usage.entries, 7);
}

#[test]
fn cachedir_tag_requires_a_valid_signature() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    cachedir_tag(&root.join("tool-cache"));
    file(root, "tool-cache/blob", 10);
    file(root, "fake/CACHEDIR.TAG", 10);
    file(root, "fake/data", 10);

    let collected = scan(root);
    assert_eq!(
        found(&collected, root),
        pairs(&[("tool-cache", "cachedir-tag")])
    );
    assert_eq!(collected.candidates[0].rule.safety, Safety::Review);
}

#[test]
fn ignore_file_and_skip_dirs_hide_subtrees() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    file(root, "kept/package.json", 2);
    file(root, "kept/.kuizerignore", 0);
    file(root, "kept/node_modules/a.js", 10);
    file(root, ".git/package.json", 2);
    file(root, ".git/node_modules/a.js", 10);

    assert!(scan(root).candidates.is_empty());
}

#[test]
fn honours_legacy_sweepignore_marker() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    file(root, "legacy/package.json", 2);
    file(root, "legacy/.sweepignore", 0);
    file(root, "legacy/node_modules/a.js", 10);
    file(root, "open/package.json", 2);
    file(root, "open/node_modules/a.js", 10);

    let collected = scan(root);
    let paths: Vec<String> = found(&collected, root)
        .into_iter()
        .map(|(path, _)| path)
        .collect();
    assert_eq!(paths, ["open/node_modules"]);
}

#[test]
fn excludes_by_name_and_path() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    file(root, "a/package.json", 2);
    file(root, "a/node_modules/x.js", 10);
    file(root, "b/package.json", 2);
    file(root, "b/node_modules/x.js", 10);
    file(root, "c/package.json", 2);
    file(root, "c/node_modules/x.js", 10);

    let mut config = Config::default();
    let canonical = fs::canonicalize(root).unwrap();
    config.scan.exclude = vec!["a".to_owned(), format!("{}/b/**", canonical.display())];
    let collected = run(&config, root);

    assert_eq!(
        found(&collected, root),
        pairs(&[("c/node_modules", "node-modules")])
    );
}

#[test]
fn min_size_and_categories_filter_results() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    file(root, "p/package.json", 2);
    file(root, "p/node_modules/big.bin", 200_000);
    file(root, "p/.turbo/log", 10);

    let mut config = Config::default();
    config.scan.min_size = ByteSize(100_000);
    assert_eq!(
        found(&run(&config, root), root),
        pairs(&[("p/node_modules", "node-modules")])
    );

    let mut config = Config::default();
    config.detect.categories = vec![kuizer::detect::Category::Cache];
    assert_eq!(
        found(&run(&config, root), root),
        pairs(&[("p/.turbo", "turbo-cache")])
    );
}

#[test]
fn custom_rules_are_applied() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    file(root, "android/build.gradle", 2);
    file(root, "android/build/intermediates/x", 10);
    file(root, "docs/build/index.html", 10);

    let config = Config::parse(
        r#"
        [[rules]]
        name = "android-build"
        pattern = "build"
        markers = ["build.gradle"]
        contains = ["intermediates"]
        category = "build"
        "#,
        std::path::Path::new("inline.toml"),
    )
    .unwrap();

    assert_eq!(
        found(&run(&config, root), root),
        pairs(&[("android/build", "android-build")])
    );
}

#[test]
fn nested_roots_are_scanned_once() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    file(root, "p/package.json", 2);
    file(root, "p/node_modules/x.js", 10);

    let roots = vec![root.to_path_buf(), root.join("p"), root.to_path_buf()];
    let handle = scan::start(&Config::default(), &roots, Cancel::new()).unwrap();
    let collected = scan::collect(&handle, |_| {});
    assert_eq!(collected.candidates.len(), 1);
}

#[test]
fn missing_root_is_reported_not_fatal() {
    let tmp = TempDir::new().unwrap();
    let collected = scan(&tmp.path().join("missing"));
    assert!(collected.candidates.is_empty());
    assert_eq!(collected.issues.len(), 1);
    assert_eq!(collected.issues[0].kind, IssueKind::Vanished);
}

#[test]
fn file_root_is_rejected() {
    let tmp = TempDir::new().unwrap();
    let path = file(tmp.path(), "single.txt", 1);
    let collected = scan(&path);
    assert_eq!(collected.issues[0].kind, IssueKind::InvalidRoot);
}

#[test]
fn excluded_mount_root_is_skipped() {
    let tmp = TempDir::new().unwrap();
    let root = dir(tmp.path(), "mnt");
    file(&root, "p/package.json", 2);
    file(&root, "p/node_modules/x.js", 10);
    let mut config = Config::default();
    config.scan.excluded_mounts = vec![fs::canonicalize(tmp.path()).unwrap()];
    let collected = run(&config, &root);
    assert!(collected.candidates.is_empty());
    assert_eq!(collected.issues[0].kind, IssueKind::SkippedMount);
}

#[test]
fn max_depth_limits_discovery() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    file(root, "a/package.json", 2);
    file(root, "a/node_modules/x.js", 10);
    file(root, "deep/er/still/package.json", 2);
    file(root, "deep/er/still/node_modules/x.js", 10);
    let mut config = Config::default();
    config.scan.max_depth = Some(2);
    assert_eq!(
        found(&run(&config, root), root),
        pairs(&[("a/node_modules", "node-modules")])
    );
}
