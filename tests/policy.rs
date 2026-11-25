#![cfg(unix)]

mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use support::file;
use tempfile::TempDir;

fn kuizer(home: &Path, config: &Path, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kuizer"));
    cmd.arg("--config")
        .arg(config)
        .args(args)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .stdin(Stdio::null());
    cmd.output().expect("kuizer runs")
}

fn setup() -> (TempDir, PathBuf, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path().join("home");
    let root = tmp.path().join("work");
    let config = tmp.path().join("kuizer.toml");
    fs::create_dir_all(&home).expect("home");
    fs::create_dir_all(&root).expect("root");
    (tmp, home, root, config)
}

fn write_config(path: &Path, body: &str) {
    fs::write(path, body).expect("write config");
}

#[test]
fn config_check_rejects_invalid_policies() {
    let (_tmp, home, _root, config) = setup();

    // Unknown policy key
    write_config(
        &config,
        r#"
[policy.bad-key]
source = "projects"
bogus = true
"#,
    );
    let out = kuizer(&home, &config, &["config", "check"]);
    assert!(!out.status.success(), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("bogus") || stderr.contains("policy"),
        "{stderr}"
    );

    // Unknown rule name in rule list
    write_config(
        &config,
        r#"
[policy.stale-deps]
source = "projects"
rule = ["node-modlues"]
"#,
    );
    let out = kuizer(&home, &config, &["config", "check"]);
    assert!(!out.status.success(), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("policy.stale-deps.rule") && stderr.contains("node-modlues"),
        "{stderr}"
    );

    // Unknown rule name in select list
    write_config(
        &config,
        r#"
[policy.bad-select]
source = "cache"
select = ["not-a-cache"]
"#,
    );
    let out = kuizer(&home, &config, &["config", "check"]);
    assert!(!out.status.success(), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("policy.bad-select.select") && stderr.contains("not-a-cache"),
        "{stderr}"
    );

    // paths on a cache policy
    write_config(
        &config,
        r#"
[policy.cache-paths]
source = "cache"
paths = ["/tmp"]
"#,
    );
    let out = kuizer(&home, &config, &["config", "check"]);
    assert!(!out.status.success(), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("policy.cache-paths.paths"), "{stderr}");

    // invalid glob in exclude
    write_config(
        &config,
        r#"
[policy.bad-glob]
source = "projects"
exclude = ["[unclosed"]
"#,
    );
    let out = kuizer(&home, &config, &["config", "check"]);
    assert!(!out.status.success(), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("policy.bad-glob.exclude"), "{stderr}");
}

#[test]
fn policy_list_and_show_describe_configured_policies() {
    let (_tmp, home, root, config) = setup();
    write_config(
        &config,
        &format!(
            r#"
[delete]
mode = "trash"

[policy.stale-deps]
source = "projects"
paths = ["{}"]
category = ["dependencies", "build"]
min_size = "10KB"
idle = "60d"
exclude = ["keep-me"]
select = "safe"

[policy.npm-cache]
source = "cache"
select = ["npm"]
permanent = true
"#,
            root.display()
        ),
    );

    let out = kuizer(&home, &config, &["policy", "list"]);
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("stale-deps"), "{stdout}");
    assert!(stdout.contains("npm-cache"), "{stdout}");
    assert!(stdout.contains("projects"), "{stdout}");
    assert!(stdout.contains("cache"), "{stdout}");

    let out = kuizer(&home, &config, &["--json", "policy", "show", "stale-deps"]);
    assert!(out.status.success(), "{out:?}");
    let shown: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json");
    assert_eq!(shown["name"], "stale-deps");
    assert_eq!(shown["source"], "projects");
    assert_eq!(shown["select"], "safe");
    assert_eq!(shown["mode"], "trash");
    assert_eq!(shown["permanent"], false);
    assert_eq!(shown["min_size"], 10_000);

    // Unknown policy exits 2
    let out = kuizer(&home, &config, &["policy", "show", "missing"]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown policy `missing`"));
}

#[test]
fn safe_policy_refuses_widening_flags_with_exit_code_2() {
    let (_tmp, home, root, config) = setup();
    file(&root, "web/package.json", 2);
    file(&root, "web/node_modules/a/index.js", 4096);
    file(&root, "web/dist/app.js", 4096);
    let outside = root.parent().unwrap().join("outside");
    fs::create_dir_all(&outside).expect("outside");

    write_config(
        &config,
        &format!(
            r#"
[policy.safe-only]
source = "projects"
paths = ["{}"]
select = "safe"
permanent = false
"#,
            root.display()
        ),
    );

    // --select all refused (exit 2)
    let out = kuizer(
        &home,
        &config,
        &[
            "clean",
            "--policy",
            "safe-only",
            "--select",
            "all",
            "--dry-run",
        ],
    );
    assert_eq!(out.status.code(), Some(2), "{out:?}");

    // --select js-dist (review rule) refused (exit 2)
    let out = kuizer(
        &home,
        &config,
        &[
            "clean",
            "--policy",
            "safe-only",
            "--select",
            "js-dist",
            "--dry-run",
        ],
    );
    assert_eq!(out.status.code(), Some(2), "{out:?}");

    // --permanent refused when permanent = false (exit 2)
    let out = kuizer(
        &home,
        &config,
        &["clean", "--policy", "safe-only", "--permanent", "--dry-run"],
    );
    assert_eq!(out.status.code(), Some(2), "{out:?}");

    // path outside policy root refused (exit 2)
    let out = kuizer(
        &home,
        &config,
        &[
            "clean",
            outside.to_str().unwrap(),
            "--policy",
            "safe-only",
            "--dry-run",
        ],
    );
    assert_eq!(out.status.code(), Some(2), "{out:?}");

    // --select node-modules (safe rule) allowed
    let out = kuizer(
        &home,
        &config,
        &[
            "--json",
            "clean",
            "--policy",
            "safe-only",
            "--select",
            "node-modules",
            "--dry-run",
        ],
    );
    assert!(out.status.success(), "{out:?}");
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json");
    let items = report["items"].as_array().expect("items");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["rule"], "node-modules");
}

#[test]
fn min_size_and_filters_narrow_without_widening() {
    let (_tmp, home, root, config) = setup();
    // Small node_modules (~4KB), medium node_modules (~32KB), large node_modules (~128KB)
    file(&root, "small/package.json", 2);
    file(&root, "small/node_modules/a/index.js", 2_000);

    file(&root, "medium/package.json", 2);
    file(&root, "medium/node_modules/a/index.js", 32_000);

    file(&root, "large/package.json", 2);
    file(&root, "large/node_modules/a/index.js", 128_000);
    file(&root, "large/.next/chunk.js", 128_000);

    write_config(
        &config,
        &format!(
            r#"
[policy.sized]
source = "projects"
paths = ["{}"]
category = ["dependencies", "build"]
min_size = "20KB"
select = "safe"
"#,
            root.display()
        ),
    );

    // Baseline policy run: medium + large (3 items: medium/node_modules, large/node_modules, large/.next)
    let out = kuizer(
        &home,
        &config,
        &["--json", "clean", "--policy", "sized", "--dry-run"],
    );
    assert!(out.status.success(), "{out:?}");
    let base: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json");
    assert_eq!(base["items"].as_array().unwrap().len(), 3, "{base}");

    // Smaller --min-size is ignored (still 3 items, small is NOT included)
    let out = kuizer(
        &home,
        &config,
        &[
            "--json",
            "clean",
            "--policy",
            "sized",
            "--min-size",
            "1KB",
            "--dry-run",
        ],
    );
    assert!(out.status.success(), "{out:?}");
    let smaller: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json");
    assert_eq!(smaller["items"].as_array().unwrap().len(), 3, "{smaller}");

    // Larger --min-size narrows (only large's 2 items remain)
    let out = kuizer(
        &home,
        &config,
        &[
            "--json",
            "clean",
            "--policy",
            "sized",
            "--min-size",
            "80KB",
            "--dry-run",
        ],
    );
    assert!(out.status.success(), "{out:?}");
    let larger: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json");
    assert_eq!(larger["items"].as_array().unwrap().len(), 2, "{larger}");

    // --category dependencies narrows out .next (build)
    let out = kuizer(
        &home,
        &config,
        &[
            "--json",
            "clean",
            "--policy",
            "sized",
            "--min-size",
            "80KB",
            "--category",
            "dependencies",
            "--dry-run",
        ],
    );
    assert!(out.status.success(), "{out:?}");
    let deps_only: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json");
    let items = deps_only["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "{deps_only}");
    assert_eq!(items[0]["rule"], "node-modules");

    // Disjoint --category cache yields 0 items (never widens)
    let out = kuizer(
        &home,
        &config,
        &[
            "--json",
            "clean",
            "--policy",
            "sized",
            "--category",
            "cache",
            "--dry-run",
        ],
    );
    assert!(out.status.success(), "{out:?}");
    let disjoint: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json");
    assert_eq!(disjoint["items"].as_array().unwrap().len(), 0, "{disjoint}");
}

#[test]
fn dry_run_lists_what_policy_show_describes_on_fixed_tree() {
    let (_tmp, home, root, config) = setup();
    file(&root, "alpha/package.json", 2);
    file(&root, "alpha/node_modules/pkg/index.js", 32_000);
    file(&root, "alpha/dist/bundle.js", 32_000);

    file(&root, "beta/package.json", 2);
    file(&root, "beta/node_modules/pkg/index.js", 32_000);

    write_config(
        &config,
        &format!(
            r#"
[policy.alpha-deps]
source = "projects"
paths = ["{}"]
category = ["dependencies"]
exclude = ["beta"]
min_size = "10KB"
select = "safe"
"#,
            root.display()
        ),
    );

    let show = kuizer(&home, &config, &["--json", "policy", "show", "alpha-deps"]);
    assert!(show.status.success(), "{show:?}");
    let shown: serde_json::Value = serde_json::from_slice(&show.stdout).expect("json");
    assert_eq!(shown["categories"], serde_json::json!(["dependencies"]));
    assert_eq!(shown["exclude"], serde_json::json!(["beta"]));
    assert_eq!(shown["select"], "safe");

    let dry = kuizer(
        &home,
        &config,
        &["--json", "clean", "--policy", "alpha-deps", "--dry-run"],
    );
    assert!(dry.status.success(), "{dry:?}");
    let report: serde_json::Value = serde_json::from_slice(&dry.stdout).expect("json");
    let items = report["items"].as_array().expect("items");
    assert_eq!(items.len(), 1, "{report}");
    assert_eq!(items[0]["rule"], "node-modules");
    assert_eq!(items[0]["status"], "would-remove");
    assert!(
        items[0]["path"]
            .as_str()
            .unwrap()
            .ends_with("alpha/node_modules")
    );
    // Dry run touches nothing
    assert!(root.join("alpha/node_modules/pkg/index.js").exists());
    assert!(root.join("beta/node_modules/pkg/index.js").exists());
}

#[test]
fn cache_policy_touches_only_selected_caches_and_records_policy_in_history() {
    let (_tmp, home, root, config) = setup();
    file(&home, ".npm/_cacache/index-v5/entry", 8192);
    file(&home, ".npm/_npx/1234/index.js", 8192);

    write_config(
        &config,
        r#"
[delete]
mode = "permanent"

[policy.npm-only]
source = "cache"
select = ["npm"]
"#,
    );

    // Paths on a cache policy are a usage error (exit 2)
    let out = kuizer(
        &home,
        &config,
        &[
            "clean",
            root.to_str().unwrap(),
            "--policy",
            "npm-only",
            "--dry-run",
        ],
    );
    assert_eq!(out.status.code(), Some(2), "{out:?}");

    // Selecting another cache not in the policy is refused (exit 2)
    let out = kuizer(
        &home,
        &config,
        &[
            "clean",
            "--policy",
            "npm-only",
            "--select",
            "npx",
            "--dry-run",
        ],
    );
    assert_eq!(out.status.code(), Some(2), "{out:?}");

    // Run the policy for real
    let out = kuizer(&home, &config, &["clean", "--policy", "npm-only", "--yes"]);
    assert!(out.status.success(), "{out:?}");
    assert!(!home.join(".npm/_cacache").exists());
    assert!(home.join(".npm/_npx/1234/index.js").exists());

    // History records the policy name in JSON and text output
    let hist_json = kuizer(&home, &config, &["--json", "history"]);
    assert!(hist_json.status.success(), "{hist_json:?}");
    let hist: serde_json::Value = serde_json::from_slice(&hist_json.stdout).expect("json");
    let runs = hist["runs"].as_array().expect("runs");
    assert_eq!(runs.len(), 1, "{hist}");
    assert_eq!(runs[0]["policy"], "npm-only");

    let hist_text = kuizer(&home, &config, &["history"]);
    assert!(hist_text.status.success(), "{hist_text:?}");
    let text = String::from_utf8_lossy(&hist_text.stdout);
    assert!(text.contains("[policy: npm-only]"), "{text}");
}
