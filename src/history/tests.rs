use std::collections::HashSet;
use std::fs;
use std::thread;
use std::time::{Duration, Instant};

use super::*;

const FILE_NAME: &str = store::STATE_FILE_NAME;

fn entry(bytes: u64) -> Entry {
    Entry {
        deleted_at: now(),
        path: PathBuf::from("/tmp/a/node_modules"),
        rule: "node-modules".to_owned(),
        category: "dependencies".to_owned(),
        mode: Mode::Trash,
        bytes,
        size: size(bytes),
        restores: None,
    }
}

fn entry_at(path: &str, deleted_at: &str) -> Entry {
    Entry {
        path: PathBuf::from(path),
        deleted_at: deleted_at.to_owned(),
        ..entry(1)
    }
}

fn record(path: &str, deleted_at: Option<&str>) -> TrashRecord {
    TrashRecord {
        path: PathBuf::from(path),
        deleted_at: deleted_at.map(|time| time.parse().unwrap()),
    }
}

#[test]
fn append_accumulates_totals_across_runs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(FILE_NAME);
    append(
        &path,
        &path,
        Run::new(now(), &[], vec![entry(1 << 20), entry(1 << 20)]),
    )
    .expect("first");
    append(&path, &path, Run::new(now(), &[], vec![entry(1 << 30)])).expect("second");

    let history = load(&path).expect("load");
    assert_eq!(history.runs.len(), 2);
    assert_eq!(history.runs[0].total.items, 2);
    assert_eq!(history.runs[0].total.size, "2.0 MiB");
    assert_eq!(history.cumulative.runs, 2);
    assert_eq!(history.cumulative.items, 3);
    assert_eq!(history.cumulative.bytes, (1 << 30) + (2 << 20));

    let text = fs::read_to_string(&path).expect("read");
    assert_eq!(text.lines().count(), 2);
    for line in text.lines() {
        let value: serde_json::Value = serde_json::from_str(line).expect("jsonl line");
        assert_eq!(value["type"], "run");
    }
}

#[test]
fn recorder_writes_each_entry_into_one_run() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(FILE_NAME);
    let mut fresh = Recorder::at(path.clone(), path.clone(), now(), &[]);
    fresh.add(entry(7));
    fresh.add(entry(0));
    fresh.flush();
    assert_eq!(load(&path).expect("fresh").runs[0].total.items, 2);
    let mut recorder = Recorder::at(path.clone(), path.clone(), now(), &[]);

    recorder.add(entry(1));
    let partial = load(&path).expect("after one");
    assert_eq!(partial.runs.len(), 2);
    assert_eq!(partial.runs[1].total.items, 1);

    recorder.add(entry(2));
    assert_eq!(recorder.session(), Some(2));
    recorder.finish().expect("no errors");
    let history = load(&path).expect("after two");
    assert_eq!(history.runs.len(), 2);
    assert_eq!(history.runs[1].session(), "s2");
    assert_eq!(history.runs[1].total.bytes, 3);
    assert_eq!(history.cumulative.items, 4);
}

#[test]
fn an_unreadable_history_is_set_aside_and_recording_continues() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(FILE_NAME);
    fs::write(&path, "{\"runs\": [").expect("garbage");

    append(&path, &path, Run::new(now(), &[], vec![entry(1)])).expect("append");

    assert_eq!(load(&path).expect("fresh").runs.len(), 1);
    let aside: Vec<_> = fs::read_dir(dir.path())
        .expect("list")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().contains(".corrupt-"))
        .collect();
    assert_eq!(aside.len(), 1);
    assert_eq!(
        fs::read_to_string(aside[0].path()).expect("kept"),
        "{\"runs\": ["
    );

    let mut recorder = Recorder::at(path.clone(), path.clone(), now(), &[]);
    recorder.add(entry(2));
    recorder.finish().expect("no errors");
    assert_eq!(load(&path).expect("reload").runs.len(), 2);
}

#[test]
fn recorder_without_entries_writes_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(FILE_NAME);
    let recorder = Recorder::at(path.clone(), path.clone(), now(), &[]);
    assert_eq!(recorder.session(), None);
    recorder.finish().expect("no errors");
    assert!(!path.exists());
}

fn recorded_items(path: &Path) -> usize {
    load(path).map_or(0, |history| history.cumulative.items)
}

#[test]
fn queued_entries_reach_the_file_without_finish() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(FILE_NAME);
    let mut recorder = Recorder::at(path.clone(), path.clone(), now(), &[]);
    recorder.add(entry(1));
    assert_eq!(recorded_items(&path), 1);
    recorder.add(entry(2));
    recorder.add(entry(3));
    drop(recorder);
    assert_eq!(recorded_items(&path), 3);

    let other = dir.path().join("interrupted.json");
    let mut abandoned = Recorder::at(other.clone(), other.clone(), now(), &[]);
    abandoned.add(entry(1));
    abandoned.add(entry(2));
    abandoned.add(entry(3));
    std::mem::forget(abandoned);
    let deadline = Instant::now() + Duration::from_secs(10);
    while recorded_items(&other) < 3 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(recorded_items(&other), 3);
}

#[test]
fn many_queued_entries_land_in_one_run_and_keep_other_runs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(FILE_NAME);
    let mut recorder = Recorder::at(path.clone(), path.clone(), now(), &[]);
    recorder.add(entry(1));
    append(&path, &path, Run::new(now(), &[], vec![entry(10)])).expect("other process");
    for _ in 0..128 {
        recorder.add(entry(1));
    }
    recorder.finish().expect("no errors");
    let history = load(&path).expect("load");
    assert_eq!(history.runs.len(), 2);
    assert_eq!(history.runs[0].deleted.len(), 129);
    assert_eq!(history.runs[0].total.items, 129);
    assert_eq!(history.cumulative.items, 130);
}

#[test]
fn concurrent_appends_keep_every_run() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(FILE_NAME);
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                for _ in 0..5 {
                    append(&path, &path, Run::new(now(), &[], vec![entry(1)])).expect("append");
                }
            });
        }
    });
    let history = load(&path).expect("load");
    assert_eq!(history.runs.len(), 40);
    let ids: HashSet<u64> = history.runs.iter().map(|run| run.id).collect();
    assert_eq!(ids.len(), 40);
    let leftovers: Vec<_> = fs::read_dir(dir.path())
        .expect("list")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty());
}

#[test]
fn missing_file_is_empty_history() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert_eq!(
        load(&dir.path().join("none.json")).expect("load"),
        History::default()
    );
}

#[test]
fn reads_legacy_history_and_writes_the_new_file() {
    let home = tempfile::tempdir().expect("tempdir");
    let legacy_sweep = home.path().join(store::LEGACY_SWEEP_FILE);
    let legacy_kuizer = home.path().join(store::LEGACY_KUIZER_FILE);
    let current = store::state_path_in(home.path());
    assert_eq!(readable_path_in(home.path()), current);

    append(
        &legacy_sweep,
        &legacy_sweep,
        Run::new(now(), &[], vec![entry(1)]),
    )
    .expect("legacy sweep");
    assert_eq!(readable_path_in(home.path()), legacy_sweep);

    append(
        &legacy_kuizer,
        &legacy_kuizer,
        Run::new(now(), &[], vec![entry(2)]),
    )
    .expect("legacy kuizer");
    assert_eq!(readable_path_in(home.path()), legacy_kuizer);

    store::append_with_cap(
        &readable_path_in(home.path()),
        &current,
        Run::new(now(), &[], vec![entry(3)]),
        DEFAULT_MAX_BYTES,
        true,
    )
    .expect("migrate");
    assert_eq!(readable_path_in(home.path()), current);
    assert_eq!(load(&current).expect("current").runs.len(), 2);
    assert_eq!(load(&legacy_kuizer).expect("legacy kept").runs.len(), 1);
}

#[test]
fn old_files_get_ids_in_order_and_keep_them() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(FILE_NAME);
    let old = r#"{"runs":[
        {"started_at":"a","finished_at":"a","command":"kuizer clean","roots":[],"deleted":[],"total":{"items":0,"bytes":0,"size":"0 B"}},
        {"started_at":"b","finished_at":"b","command":"kuizer clean","roots":[],"deleted":[],"total":{"items":0,"bytes":0,"size":"0 B"}}
    ]}"#;
    fs::write(&path, old).expect("write");

    let first = load(&path).expect("load");
    assert_eq!(
        first.runs.iter().map(Run::session).collect::<Vec<_>>(),
        ["s1", "s2"]
    );
    assert_eq!(first.next_session, 3);

    append(&path, &path, Run::new(now(), &[], vec![entry(1)])).expect("append");
    let second = load(&path).expect("reload");
    assert_eq!(
        second.runs.iter().map(Run::session).collect::<Vec<_>>(),
        ["s1", "s2", "s3"]
    );
    assert_eq!(load(&path).expect("again"), second);
}

#[test]
fn finds_sessions_and_items_by_id() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(FILE_NAME);
    append(&path, &path, Run::new(now(), &[], vec![entry(1), entry(2)])).expect("append");
    let history = load(&path).expect("load");

    assert_eq!(history.find("s1"), Some(vec![(0, 0), (0, 1)]));
    assert_eq!(history.find("s1.2"), Some(vec![(0, 1)]));
    assert_eq!(history.item_id(0, 1), "s1.2");
    for missing in ["s1.3", "s1.0", "s2", "1", "s", "sx.1"] {
        assert_eq!(history.find(missing), None, "{missing}");
    }
}

#[test]
fn a_failed_write_keeps_its_entries_in_one_session() {
    let dir = tempfile::tempdir().expect("tempdir");
    let folder = dir.path().join("later");
    let path = folder.join(FILE_NAME);
    let mut recorder = Recorder::at(path.clone(), path.clone(), now(), &[]);
    recorder.add(entry(1));
    assert!(recorder.session().is_none());
    fs::create_dir(&folder).expect("folder");
    recorder.add(entry(2));
    recorder.finish().expect("recovers");
    let history = load(&path).expect("load");
    assert_eq!(history.runs.len(), 1);
    assert_eq!(history.runs[0].deleted.len(), 2);
}

#[test]
fn a_non_utf8_path_survives_the_history_file() {
    use std::os::unix::ffi::OsStringExt;
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(FILE_NAME);
    let odd = PathBuf::from(std::ffi::OsString::from_vec(
        b"/tmp/caf\xE9/target".to_vec(),
    ));
    let mut item = entry(1);
    item.path = odd.clone();
    item.mode = Mode::Trash;
    let mut recorder = Recorder::at(
        path.clone(),
        path.clone(),
        now(),
        std::slice::from_ref(&odd),
    );
    recorder.add(item);
    recorder.finish().expect("writes");
    let history = load(&path).expect("load");
    assert_eq!(history.runs[0].roots, std::slice::from_ref(&odd));
    assert_eq!(history.runs[0].deleted[0].path, odd);
    assert_eq!(still_in_trash(&history, &[odd]).len(), 1);
}

#[test]
fn restores_subtract_from_the_totals() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(FILE_NAME);
    append(
        &path,
        &path,
        Run::new(now(), &[], vec![entry(100), entry(50)]),
    )
    .expect("delete");
    let deleted = load(&path).expect("load").runs[0].deleted[1].clone();
    append(
        &path,
        &path,
        Run::restore(now(), vec![Entry::restored(&deleted, "s1.2".to_owned())]),
    )
    .expect("restore");

    let history = load(&path).expect("reload");
    assert_eq!(history.cumulative.bytes, 100);
    assert_eq!(history.cumulative.items, 1);
    assert_eq!(history.cumulative.restored_bytes, 50);
    assert_eq!(history.restored_ids(), HashSet::from(["s1.2"]));
    let kept = still_in_trash(&history, &[PathBuf::from("/tmp/a/node_modules")]);
    assert_eq!(kept, HashSet::from([(0, 1)]));
}

#[test]
fn counts_trashed_bytes_as_freed_only_once_they_leave_the_trash() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(FILE_NAME);
    let mut permanent = entry(1000);
    permanent.mode = Mode::Permanent;
    append(
        &path,
        &path,
        Run::new(now(), &[], vec![entry(100), entry(10), permanent]),
    )
    .expect("delete");
    let restored = load(&path).expect("load").runs[0].deleted[1].clone();
    append(
        &path,
        &path,
        Run::restore(now(), vec![Entry::restored(&restored, "s1.2".to_owned())]),
    )
    .expect("restore");
    let history = load(&path).expect("reload");

    assert_eq!(history.cumulative.bytes, 1100);
    assert_eq!(history.cumulative.permanent_bytes, 1000);
    assert_eq!(history.cumulative.trashed_bytes, 100);
    let trash = |kept| TrashSnapshot::new(&history, kept);
    assert_eq!(freed_bytes(&history, &trash(None)), 1000);
    assert_eq!(
        freed_bytes(&history, &trash(Some(HashSet::from([(0, 0)])))),
        1000
    );
    assert_eq!(freed_bytes(&history, &trash(Some(HashSet::new()))), 1100);
}

#[test]
fn matches_the_trash_item_closest_in_time() {
    let history = History {
        next_session: 2,
        runs: vec![Run::new(
            now(),
            &[],
            vec![
                entry_at("/w/a", "2026-10-15T12:00:00+00:00"),
                entry_at("/w/a", "2026-10-15T14:00:00+00:00"),
                entry_at("/w/b", "2026-10-15T12:00:00+00:00"),
                entry_at("/w/c", "2026-10-15T12:00:00+00:00"),
            ],
        )],
        cumulative: Cumulative::default(),
    };
    let trash = [
        record("/w/a", Some("2026-10-15T14:00:03")),
        record("/w/a", Some("2026-10-15T12:00:01")),
        record("/w/b", Some("2026-10-15T13:00:00")),
        record("/w/c", None),
    ];
    let targets = [(0, 0), (0, 1), (0, 2), (0, 3)];

    let matched = match_trash(&history, &targets, &trash);

    assert_eq!(matched, [Some(1), Some(0), None, Some(3)]);
}

#[test]
fn matches_by_wall_clock_time_after_a_time_zone_change() {
    let history = History {
        next_session: 2,
        runs: vec![Run::new(
            now(),
            &[],
            vec![
                entry_at("/w/a", "2026-10-15T12:00:00+02:00"),
                entry_at("/w/b", "2026-03-29T01:59:00+01:00"),
            ],
        )],
        cumulative: Cumulative::default(),
    };
    let trash = [
        record("/w/a", Some("2026-10-15T12:00:02")),
        record("/w/b", Some("2026-03-29T01:59:00")),
    ];

    let matched = match_trash(&history, &[(0, 0), (0, 1)], &trash);

    assert_eq!(matched, [Some(0), Some(1)]);
}

#[test]
fn trash_record_keeps_the_wall_clock_time() {
    let zone = jiff::tz::TimeZone::system();
    let wall: jiff::civil::DateTime = "2026-10-15T12:00:00".parse().unwrap();
    let unix = wall.to_zoned(zone).unwrap().timestamp().as_second();

    let record = TrashRecord::new(PathBuf::from("/w/a"), unix);

    assert_eq!(record.deleted_at, Some(wall));
    assert_eq!(TrashRecord::new(PathBuf::from("/w/a"), -1).deleted_at, None);
}

#[test]
fn append_only_keeps_file_inode_and_appends_records() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(FILE_NAME);
    append(&path, &path, Run::new(now(), &[], vec![entry(1)])).expect("first");
    #[cfg(unix)]
    let inode_before = std::os::unix::fs::MetadataExt::ino(&fs::metadata(&path).expect("meta"));
    let len_before = fs::metadata(&path).expect("meta").len();

    let mut recorder = Recorder::at(path.clone(), path.clone(), now(), &[]);
    recorder.add(entry(2));
    recorder.add(entry(3));
    recorder.flush();
    recorder.add(entry(4));
    recorder.finish().expect("finish");

    #[cfg(unix)]
    let inode_after = std::os::unix::fs::MetadataExt::ino(&fs::metadata(&path).expect("meta"));
    let len_after = fs::metadata(&path).expect("meta").len();
    #[cfg(unix)]
    assert_eq!(inode_before, inode_after);
    assert!(len_after > len_before);

    let text = fs::read_to_string(&path).expect("read");
    assert!(text.lines().count() >= 3);
    let history = load(&path).expect("load");
    assert_eq!(history.runs.len(), 2);
    assert_eq!(history.runs[1].deleted.len(), 3);
    assert_eq!(history.runs[1].total.bytes, 9);
}

#[test]
fn automatic_compaction_caps_file_size_and_keeps_newest_run() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(FILE_NAME);
    let cap = 900;
    for idx in 1..=6u64 {
        let mut item = entry(idx * 10);
        item.path = PathBuf::from(format!("/tmp/project-{idx}/node_modules"));
        store::append_with_cap(&path, &path, Run::new(now(), &[], vec![item]), cap, false)
            .expect("append");
    }
    let len = fs::metadata(&path).expect("meta").len();
    assert!(len <= cap, "expected compacted file <= {cap}, got {len}");
    let history = load(&path).expect("load");
    assert!(!history.runs.is_empty());
    assert!(history.runs.len() < 6);
    assert_eq!(history.runs.last().expect("newest").id, 6);
    assert_eq!(history.next_session, 7);
}

#[test]
fn prune_trims_by_keep_age_and_size_without_reusing_ids() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(FILE_NAME);

    let mut old_run = Run::new(
        "2020-01-01T00:00:00Z".to_owned(),
        &[],
        vec![entry_at("/tmp/old", "2020-01-01T00:00:00Z")],
    );
    old_run.finished_at = "2020-01-01T00:00:05Z".to_owned();
    append(&path, &path, old_run).expect("r1");
    append(&path, &path, Run::new(now(), &[], vec![entry(20)])).expect("r2");
    append(&path, &path, Run::new(now(), &[], vec![entry(30)])).expect("r3");

    let dry = prune(
        &path,
        &path,
        &PruneOptions {
            older_than: Some(Duration::from_secs(30 * 86_400)),
            dry_run: true,
            ..PruneOptions::default()
        },
    )
    .expect("dry prune");
    assert!(dry.dry_run);
    assert_eq!(dry.pruned_runs, 1);
    assert_eq!(dry.kept_runs, 2);
    assert_eq!(load(&path).expect("unchanged").runs.len(), 3);

    let report = prune(
        &path,
        &path,
        &PruneOptions {
            older_than: Some(Duration::from_secs(30 * 86_400)),
            keep: Some(1),
            ..PruneOptions::default()
        },
    )
    .expect("prune");
    assert_eq!(report.pruned_runs, 2);
    assert_eq!(report.kept_runs, 1);
    let kept = load(&path).expect("load");
    assert_eq!(kept.runs.len(), 1);
    assert_eq!(kept.runs[0].id, 3);
    assert_eq!(kept.next_session, 4);

    let empty = prune(
        &path,
        &path,
        &PruneOptions {
            keep: Some(0),
            ..PruneOptions::default()
        },
    )
    .expect("prune all");
    assert_eq!(empty.pruned_runs, 1);
    assert_eq!(empty.kept_runs, 0);
    let after = load(&path).expect("load empty");
    assert!(after.runs.is_empty());
    assert_eq!(after.next_session, 4);

    append(&path, &path, Run::new(now(), &[], vec![entry(40)])).expect("r4");
    let resumed = load(&path).expect("resumed");
    assert_eq!(resumed.runs.len(), 1);
    assert_eq!(resumed.runs[0].session(), "s4");
}
