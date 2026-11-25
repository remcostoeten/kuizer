use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::Instant;

use super::*;

const TIMEOUT: Duration = Duration::from_secs(10);

fn script(dir: &Path, name: &str, body: &str) -> String {
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("script");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod");
    path.display().to_string()
}

fn fake_docker(dir: &Path) -> String {
    script(
        dir,
        "docker",
        r#"log="$(dirname "$0")/calls"
echo "$*" >> "$log"
case "$1 $2" in
  "image ls") printf 'sha256:aaaaaaaaaaaaaaaa\t1.5GB\t3 weeks ago\nsha256:bbbbbbbbbbbbbbbb\t200MB\t2 days ago\n' ;;
  "image inspect") cat "$(dirname "$0")/tags" 2>/dev/null || echo 0 ;;
  "container ls") printf 'c1\told_api\tapi:1\t12.5MB (virtual 900MB)\tExited (0) 3 days ago\nc2\tfresh\tweb:2\t1kB (virtual 10MB)\tExited (1) 2 hours ago\nc3\tnever\tweb:2\t0B (virtual 10MB)\tCreated\n' ;;
  "system df")
if [ "$3" = "--verbose" ]; then
  echo '[{"Name":"pgdata","Size":"300MB","Links":0}]'
else
  printf 'Images\t2.1GB (90%%)\nContainers\t12.5MB (100%%)\nLocal Volumes\t300MB (100%%)\nBuild Cache\t4.2GB\n'
fi ;;
  "volume ls") echo pgdata ;;
  "builder du") printf '{"Reclaimable":true,"Shared":false,"Size":"4.1GB"}\n{"Reclaimable":true,"Shared":false,"Size":"100MB"}\n{"Reclaimable":true,"Shared":true,"Size":"9GB"}\n{"Reclaimable":false,"Shared":false,"Size":"7GB"}\n' ;;
  *) exit 0 ;;
esac"#,
    )
}

#[test]
fn lists_every_object_kind() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let program = fake_docker(tmp.path());
    let listing = list(std::slice::from_ref(&program), TIMEOUT, 24);
    assert!(listing.issues.is_empty(), "{:?}", listing.issues);
    let rows: Vec<(Object, &str, u64, Safety)> = listing
        .items
        .iter()
        .map(|item| (item.object, item.id.as_str(), item.bytes, item.safety))
        .collect();
    assert_eq!(
        rows,
        [
            (Object::BuildCache, "all", 4_200_000_000, Safety::Safe),
            (
                Object::Image,
                "aaaaaaaaaaaaaaaa",
                1_500_000_000,
                Safety::Safe
            ),
            (Object::Volume, "pgdata", 300_000_000, Safety::Review),
            (Object::Image, "bbbbbbbbbbbbbbbb", 200_000_000, Safety::Safe),
            (Object::Container, "c1", 12_500_000, Safety::Safe),
            (Object::Container, "c2", 1_000, Safety::Review),
            (Object::Container, "c3", 0, Safety::Safe),
        ]
    );
    let entries = entries(listing.items);
    let (item, candidate) = &entries[1];
    assert_eq!(candidate.kind, Kind::External);
    assert_eq!(candidate.rule.name, "image");
    assert_eq!(candidate.rule.ecosystem, "docker");
    assert_eq!(
        candidate.path,
        PathBuf::from(format!("{program}:image/aaaaaaaaaaaaaaaa"))
    );
    assert_eq!(item.label, "<none> aaaaaaaaaaaa");
}

#[test]
fn removes_through_the_engine_without_force() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let program = fake_docker(tmp.path());
    let entries = entries(list(std::slice::from_ref(&program), TIMEOUT, 24).items);
    for (_, candidate) in &entries {
        remove(std::slice::from_ref(&program), candidate, Mode::DryRun).expect("dry run");
    }
    let before = fs::read_to_string(tmp.path().join("calls")).expect("calls");
    for (_, candidate) in &entries {
        remove(std::slice::from_ref(&program), candidate, Mode::Permanent).expect("removed");
    }
    let calls = fs::read_to_string(tmp.path().join("calls")).expect("calls");
    let removals: Vec<&str> = calls[before.len()..].lines().collect();
    assert_eq!(
        removals,
        [
            "builder prune --force",
            "image inspect --format {{len .RepoTags}} aaaaaaaaaaaaaaaa",
            "image rm aaaaaaaaaaaaaaaa",
            "volume rm pgdata",
            "image inspect --format {{len .RepoTags}} bbbbbbbbbbbbbbbb",
            "image rm bbbbbbbbbbbbbbbb",
            "container rm c1",
            "container rm c2",
            "container rm c3",
        ]
    );
}

#[test]
fn an_image_tagged_after_listing_is_not_removed() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let program = fake_docker(tmp.path());
    let entries = entries(list(std::slice::from_ref(&program), TIMEOUT, 24).items);
    let (_, image) = entries
        .iter()
        .find(|(item, _)| item.id == "aaaaaaaaaaaaaaaa")
        .expect("image");
    fs::write(tmp.path().join("tags"), "1\n").expect("tags");

    let error = remove(std::slice::from_ref(&program), image, Mode::Permanent).unwrap_err();

    assert!(error.to_string().contains("is tagged now"), "{error}");
    let calls = fs::read_to_string(tmp.path().join("calls")).expect("calls");
    assert!(!calls.contains("image rm"), "{calls}");
}

#[test]
fn a_stopped_daemon_is_one_issue() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let program = script(
        tmp.path(),
        "docker",
        "echo 'Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?' >&2\nexit 1",
    );
    let listing = list(&[program], TIMEOUT, 24);
    assert!(listing.items.is_empty());
    assert_eq!(listing.issues.len(), 1);
    assert!(
        listing.issues[0].starts_with("docker is not running: Cannot connect"),
        "{}",
        listing.issues[0]
    );
}

#[test]
fn missing_engines_are_one_issue() {
    let listing = list(
        &[
            "/nonexistent/kuizer/docker".to_owned(),
            "/nonexistent/kuizer/podman".to_owned(),
        ],
        TIMEOUT,
        24,
    );
    assert!(listing.items.is_empty());
    assert_eq!(listing.issues, ["neither docker nor podman is installed"]);
}

#[test]
fn a_hanging_engine_times_out() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let program = script(tmp.path(), "podman", "sleep 5");
    let started = Instant::now();
    let listing = list(&[program], Duration::from_millis(200), 24);
    assert!(started.elapsed() < Duration::from_secs(3));
    assert_eq!(listing.issues, ["podman did not answer in time"]);
}

#[test]
fn one_engine_answers_its_calls_side_by_side() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let program = script(
        tmp.path(),
        "docker",
        r#"sleep 1
case "$1 $2" in
  "volume ls") echo pgdata ;;
  "system df") if [ "$3" = "--verbose" ]; then echo '[{"Name":"pgdata","Size":"300MB","Links":0}]'; fi ;;
esac"#,
    );
    let started = Instant::now();
    let listing = list(&[program], TIMEOUT, 24);
    assert!(listing.issues.is_empty(), "{:?}", listing.issues);
    assert_eq!(listing.items.len(), 1);
    assert!(started.elapsed() < Duration::from_millis(3500));
}

#[test]
fn volume_sizes_arrive_after_the_fast_listing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let program = script(
        tmp.path(),
        "docker",
        r#"case "$1 $2" in
  "volume ls") echo pgdata ;;
  "system df") sleep 2; echo '[{"Name":"pgdata","Size":"300MB","Links":0}]' ;;
  "container ls") printf 'c1\told\tapi:1\t1MB\tExited (0) 3 days ago\n' ;;
esac"#,
    );
    let started = Instant::now();
    let (mut listing, sizing) = list_fast(&[program], TIMEOUT, 24);
    assert!(started.elapsed() < Duration::from_millis(1500));
    let volume = listing
        .items
        .iter()
        .find(|item| item.object == Object::Volume);
    assert_eq!(volume.map(|item| item.bytes), Some(0));
    assert!(!sizing.is_empty());
    listing.apply(&sizing.wait());
    let rows: Vec<(Object, u64)> = listing
        .items
        .iter()
        .map(|item| (item.object, item.bytes))
        .collect();
    assert_eq!(
        rows,
        [
            (Object::Volume, 300_000_000),
            (Object::Container, 1_000_000)
        ]
    );
}

#[test]
fn build_cache_falls_back_to_system_df() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let program = script(
        tmp.path(),
        "docker",
        r#"case "$1 $2" in
  "builder du") echo "unknown command" >&2; exit 1 ;;
  "system df") printf 'Build Cache\t4.2GB\n' ;;
esac"#,
    );
    let listing = list(&[program], TIMEOUT, 24);
    let rows: Vec<(Object, u64)> = listing
        .items
        .iter()
        .map(|item| (item.object, item.bytes))
        .collect();
    assert_eq!(rows, [(Object::BuildCache, 4_200_000_000)]);
}

#[test]
fn refusals_name_the_engine() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let program = script(
        tmp.path(),
        "podman",
        "echo 'Error: volume pgdata is being used' >&2\nexit 2",
    );
    let item = Item {
        engine: "podman".to_owned(),
        program,
        object: Object::Volume,
        id: "pgdata".to_owned(),
        label: "pgdata".to_owned(),
        bytes: 0,
        safety: Safety::Review,
        note: String::new(),
    };
    let error = remove(
        std::slice::from_ref(&item.program),
        &item.candidate(1),
        Mode::Permanent,
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "podman refused: Error: volume pgdata is being used"
    );
}

#[test]
fn reads_stop_times() {
    assert_eq!(hours_ago("Exited (0) 3 days ago"), Some(72));
    assert_eq!(hours_ago("Exited (137) 2 hours ago"), Some(2));
    assert_eq!(hours_ago("Exited (1) About an hour ago"), Some(1));
    assert_eq!(hours_ago("Exited (0) 45 seconds ago"), Some(0));
    assert_eq!(hours_ago("Exited (0) 2 weeks ago"), Some(336));
    assert_eq!(hours_ago("Created"), None);
}

#[test]
fn foreign_paths_are_not_removed() {
    for path in [
        "/home/me/x",
        "docker:image/",
        "docker:thing/abc",
        "docker:image/-f",
    ] {
        assert!(parse_path(Path::new(path)).is_none(), "{path}");
    }
}
