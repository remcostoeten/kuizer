use std::fs;
use std::process::Command;

fn kuizer() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kuizer"))
}

#[test]
fn config_check_fix_rewrites_deprecated_and_invalid_keys_and_prints_changes() {
    let temp = tempfile::tempdir().expect("tempdir");
    let config = temp.path().join("config.toml");
    fs::write(
        &config,
        r#"# user notes
[ui]
motion = false

[scan]
ignore_file = ".sweepignore"
size_threads = 128
exclude = ["vendor", "["]

[delete]
mode = "trsh"

[unknown_section]
flag = true
"#,
    )
    .expect("write config");

    // Strict check fails before fixing.
    let before = kuizer()
        .args(["--config", config.to_str().unwrap(), "config", "check"])
        .output()
        .expect("run config check");
    assert!(!before.status.success());

    // --fix rewrites the file and prints every change.
    let fixed = kuizer()
        .args([
            "--config",
            config.to_str().unwrap(),
            "config",
            "check",
            "--fix",
        ])
        .output()
        .expect("run config check --fix");
    assert!(
        fixed.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&fixed.stderr)
    );
    let stdout = String::from_utf8_lossy(&fixed.stdout);
    assert!(stdout.contains("ui.motion"), "{stdout}");
    assert!(stdout.contains("scan.ignore_file"), "{stdout}");
    assert!(stdout.contains("scan.size_threads"), "{stdout}");
    assert!(stdout.contains("scan.exclude"), "{stdout}");
    assert!(stdout.contains("delete.mode"), "{stdout}");
    assert!(stdout.contains("unknown_section"), "{stdout}");

    let rewritten = fs::read_to_string(&config).expect("read rewritten config");
    assert!(rewritten.starts_with("# user notes\n"), "{rewritten}");

    // Strict check now passes cleanly.
    let after = kuizer()
        .args(["--config", config.to_str().unwrap(), "config", "check"])
        .output()
        .expect("run config check after fix");
    assert!(
        after.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&after.stderr)
    );
    assert!(String::from_utf8_lossy(&after.stdout).contains("config ok"));

    // Running --fix --json again reports zero changes.
    let again = kuizer()
        .args([
            "--json",
            "--config",
            config.to_str().unwrap(),
            "config",
            "check",
            "--fix",
        ])
        .output()
        .expect("run config check --fix --json");
    assert!(again.status.success());
    let json: serde_json::Value = serde_json::from_slice(&again.stdout).expect("valid json");
    assert_eq!(json["changes"].as_array().map(Vec::len), Some(0));
}

#[test]
fn config_check_fix_refuses_broken_toml_syntax() {
    let temp = tempfile::tempdir().expect("tempdir");
    let config = temp.path().join("broken.toml");
    fs::write(&config, "[scan\nmin_size = 1\n").expect("write broken config");

    let output = kuizer()
        .args([
            "--config",
            config.to_str().unwrap(),
            "config",
            "check",
            "--fix",
        ])
        .output()
        .expect("run config check --fix");
    assert!(!output.status.success());
    assert_eq!(
        fs::read_to_string(&config).expect("read"),
        "[scan\nmin_size = 1\n"
    );
}
