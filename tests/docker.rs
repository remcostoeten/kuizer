use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, Instant};

use kuizer::delete::Mode;
use kuizer::detect::Safety;
use kuizer::docker::{self, Item, Object};

#[test]
fn removal_refuses_an_engine_outside_the_configured_allowlist() {
    let directory = tempfile::tempdir().expect("temporary engine directory");
    let executable = directory.path().join("docker");
    let marker = directory.path().join("invoked");
    fs::write(&executable, "#!/bin/sh\ntouch \"${0%/*}/invoked\"\n").expect("engine fixture");
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).expect("executable");
    let candidate = Item {
        engine: "docker".into(),
        program: executable.to_str().expect("engine path").into(),
        object: Object::Volume,
        id: "volume".into(),
        label: "volume".into(),
        bytes: 0,
        safety: Safety::Review,
        note: String::new(),
    }
    .candidate(1);
    let error = docker::remove(&["docker".into()], &candidate, Mode::Permanent)
        .expect_err("the saved executable must match the configured engine");
    assert!(error.to_string().contains("not enabled in docker.engines"));
    assert!(!marker.exists());
}

#[test]
fn engine_timeout_covers_output_kept_open_by_a_descendant() {
    let directory = tempfile::tempdir().expect("temporary engine directory");
    let executable = directory.path().join("docker");
    fs::write(&executable, "#!/bin/sh\nsleep 8 &\nexit 0\n").expect("engine fixture");
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).expect("executable");
    let started = Instant::now();
    let listing = docker::list(
        &[executable.to_str().expect("engine path").into()],
        Duration::from_millis(200),
        24,
    );
    assert!(started.elapsed() < Duration::from_secs(4));
    assert!(listing.items.is_empty());
    assert_eq!(listing.issues, ["docker did not answer in time"]);
}
