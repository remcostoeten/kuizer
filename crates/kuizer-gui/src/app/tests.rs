use gpui::{
    Entity, Keystroke, Modifiers, ScrollDelta, TestAppContext, TouchPhase, VisualTestContext, point,
};

use super::*;

pub(super) fn open(
    cx: &mut TestAppContext,
    recent: PathBuf,
) -> (Entity<Kuizer>, &mut VisualTestContext) {
    cx.add_window_view(|window, cx| {
        let mut app = Kuizer::new(
            Config::default(),
            Vec::new(),
            Recent::load(Some(recent.clone())),
            Snapshots::new(recent.parent().map(|dir| dir.join("scans"))),
            "Noto Sans".into(),
            window,
            cx,
        );
        app.session.options.caches = false;
        app.session.options.toolchains = false;
        app.session.options.docker = false;
        app.config_path = recent.parent().map(|dir| dir.join("config.toml"));
        app.reveal = |_| Err("no file manager in tests".to_owned());
        app
    })
}

pub(super) fn type_text(cx: &mut VisualTestContext, text: &str) {
    for ch in text.chars() {
        let keystroke = Keystroke {
            modifiers: Modifiers::default(),
            key: ch.to_string(),
            key_char: Some(ch.to_string()),
        };
        cx.update(|window, cx| {
            window.dispatch_keystroke(keystroke, cx);
        });
    }
    cx.run_until_parked();
}

fn wait_for_scan(view: &Entity<Kuizer>, cx: &mut VisualTestContext) {
    for _ in 0..1000 {
        cx.executor().advance_clock(POLL);
        cx.run_until_parked();
        if view.read_with(cx, |app, _| {
            !matches!(app.session.phase, Phase::Loading(_)) && app.session.cancel.is_none()
        }) {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the scan did not finish");
}

fn wait_until_shown(view: &Entity<Kuizer>, cx: &mut VisualTestContext) -> bool {
    for _ in 0..1000 {
        cx.executor().advance_clock(POLL);
        cx.run_until_parked();
        if let Some(refreshing) = view.read_with(cx, |app, _| {
            (!matches!(app.session.phase, Phase::Loading(_))).then(|| app.session.stale.is_some())
        }) {
            return refreshing;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("nothing was shown");
}

pub(super) fn wait_for_refresh(view: &Entity<Kuizer>, cx: &mut VisualTestContext) {
    for _ in 0..1000 {
        cx.executor().advance_clock(POLL);
        cx.run_until_parked();
        if view.read_with(cx, |app, _| {
            !matches!(app.session.phase, Phase::Loading(_)) && !app.refreshing()
        }) {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the refresh did not finish");
}

pub(super) fn wait_for_duplicates(view: &Entity<Kuizer>, cx: &mut VisualTestContext) {
    for _ in 0..1000 {
        cx.run_until_parked();
        if view.read_with(cx, |app, _| app.session.search.is_none()) {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the duplicate search did not finish");
}

/// Waits until the finished scan is written, which happens once its results are shown.
pub(super) fn wait_for_saved(dir: &Path) {
    for _ in 0..1000 {
        let saved = std::fs::read_dir(dir.join("scans")).is_ok_and(|mut entries| {
            entries.any(|entry| {
                entry.is_ok_and(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
            })
        });
        if saved {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the scan was not saved");
}

fn scan_again_from_the_start(view: &Entity<Kuizer>, cx: &mut VisualTestContext) {
    view.update(cx, |app, cx| app.new_scan(cx));
    cx.simulate_keystrokes("enter");
}

pub(super) fn project(dir: &Path) -> PathBuf {
    let root = dir.join("apps");
    let modules = root.join("site").join("node_modules").join("left-pad");
    std::fs::create_dir_all(&modules).unwrap();
    std::fs::write(root.join("site").join("package.json"), "{}").unwrap();
    std::fs::write(modules.join("index.js"), vec![b'x'; 64 * 1024]).unwrap();
    root
}

#[gpui::test]
fn opens_on_the_start_screen_without_a_path(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (view, cx) = open(cx, dir.path().join("recent"));
    view.read_with(cx, |app, _| {
        assert!(matches!(app.session.phase, Phase::Start));
        assert!(app.start.folder.is_empty());
    });
}

#[gpui::test]
fn a_missing_folder_stays_on_the_start_screen(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (view, cx) = open(cx, dir.path().join("recent"));
    type_text(cx, &dir.path().join("missing").display().to_string());
    cx.simulate_keystrokes("enter");
    view.read_with(cx, |app, _| {
        assert!(matches!(app.session.phase, Phase::Start));
        assert_eq!(
            app.start.folder_error.as_deref(),
            Some("This folder does not exist")
        );
    });
}

#[gpui::test]
fn ctrl_backspace_removes_the_last_folder(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (view, cx) = open(cx, dir.path().join("recent"));
    type_text(cx, "/home/me/apps");
    cx.simulate_keystrokes("ctrl-backspace");
    view.read_with(cx, |app, _| assert_eq!(app.start.folder, "/home/me/"));
    cx.simulate_keystrokes("backspace");
    view.read_with(cx, |app, _| assert_eq!(app.start.folder, "/home/me"));
}

#[gpui::test]
fn enter_scans_the_typed_folder_and_remembers_it(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = project(dir.path());
    let recent = dir.path().join("recent");
    let (view, cx) = open(cx, recent.clone());
    type_text(cx, &root.display().to_string());
    cx.simulate_keystrokes("enter");
    view.read_with(cx, |app, _| {
        assert!(matches!(
            app.session.phase,
            Phase::Loading(_) | Phase::Ready(_)
        ))
    });
    wait_for_scan(&view, cx);
    let canonical = std::fs::canonicalize(&root).unwrap();
    view.read_with(cx, |app, _| {
        let Phase::Ready(report) = &app.session.phase else {
            panic!("expected a report");
        };
        assert_eq!(report.roots, vec![canonical.clone()]);
        assert_eq!(report.candidates.len(), 1);
        assert!(report.caches.is_empty());
    });
    crate::persist::flush();
    let saved = std::fs::read_to_string(&recent).unwrap();
    assert_eq!(saved.trim(), canonical.display().to_string());

    view.update(cx, |app, cx| app.new_scan(cx));
    view.read_with(cx, |app, _| {
        assert!(matches!(app.session.phase, Phase::Start));
        assert_eq!(app.start.recent.paths, vec![canonical.clone()]);
    });
}

#[gpui::test]
fn escape_cancels_a_running_scan(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = project(dir.path());
    let (view, cx) = open(cx, dir.path().join("recent"));
    let escape = Keystroke::parse("escape").unwrap();
    let cancel = cx.update(|window, cx| {
        let cancel = view.update(cx, |app, cx| {
            app.session.roots = vec![root];
            app.rescan(cx);
            assert!(matches!(app.session.phase, Phase::Loading(_)));
            app.session.cancel.clone()
        });
        window.dispatch_keystroke(escape, cx);
        cancel
    });
    assert!(cancel.expect("the scan started").is_cancelled());
    cx.run_until_parked();
    view.read_with(cx, |app, _| {
        assert!(matches!(app.session.phase, Phase::Start));
        assert!(app.session.cancel.is_none());
        assert!(app.session.task.is_none());
    });
}

#[gpui::test]
fn the_whole_home_option_ignores_the_typed_folder(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (view, cx) = open(cx, dir.path().join("recent"));
    type_text(cx, "/does/not/matter");
    view.update(cx, |app, cx| {
        app.start.whole_home = true;
        app.session.options.duplicates = false;
        app.start_scan(cx);
    });
    cx.run_until_parked();
    view.update(cx, |app, cx| {
        assert!(matches!(
            app.session.phase,
            Phase::Loading(_) | Phase::Ready(_)
        ));
        assert_eq!(
            app.session.roots,
            scan::normalize_roots(&[platform::home_dir().unwrap()])
        );
        app.cancel_scan(cx);
    });
}

#[gpui::test]
fn the_scan_view_shows_each_step_and_what_it_touches(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (view, cx) = open(cx, dir.path().join("recent"));
    view.update(cx, |app, cx| {
        app.session.options.caches = true;
        let mut activity = Activity::default();
        activity.record(Path::new("/p/site/node_modules"), "node_modules", 3 << 30);
        activity.finish(
            Step::Folders,
            Duration::from_millis(1_500),
            "1 item found".to_owned(),
        );
        activity.begin(Step::Caches, Some(4));
        activity.advance(Step::Caches, Path::new("/home/me/.cargo/registry"));
        app.session.phase = Phase::Loading(Box::new(activity));
        cx.notify();
    });
    cx.update(|window, cx| window.draw(cx).clear());
    for selector in [
        "activity",
        "step-Folders",
        "step-Caches",
        "step-Docker",
        "current",
        "found-total",
        "Shown once every part of the scan is done",
    ] {
        assert!(
            cx.debug_bounds(selector).is_some(),
            "{selector} is not shown"
        );
    }
    let folders = cx.debug_bounds("step-Folders").unwrap();
    let caches = cx.debug_bounds("step-Caches").unwrap();
    assert!(
        caches.size.height > folders.size.height,
        "the running step shows its progress and current folder below it"
    );
}

#[gpui::test]
fn the_folder_field_skips_recent_folders_that_are_gone(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let kept = dir.path().join("kept");
    std::fs::create_dir_all(&kept).unwrap();
    let recent = dir.path().join("recent");
    std::fs::write(
        &recent,
        format!("/tmp/.mount_kuizerGone/usr\n{}\n", kept.display()),
    )
    .unwrap();
    let (view, cx) = open(cx, recent);
    cx.run_until_parked();
    view.read_with(cx, |app, _| {
        assert_eq!(app.start.folder, display_path(&kept));
    });
}

#[gpui::test]
fn a_scan_shows_its_progress_then_charts_with_tooltips(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = project(dir.path());
    let (view, cx) = open(cx, dir.path().join("recent"));
    type_text(cx, &root.display().to_string());
    cx.simulate_keystrokes("enter");
    cx.update(|window, cx| window.draw(cx).clear());
    // A fast runner can stream the first artifacts in before this frame, which leaves the loader.
    if view.read_with(cx, |app, _| matches!(app.session.phase, Phase::Loading(_))) {
        assert!(cx.debug_bounds("activity").is_some());
        assert!(cx.debug_bounds("loader").is_some());
    }
    wait_for_scan(&view, cx);
    view.read_with(cx, |app, _| assert!(!app.busy()));
    cx.update(|window, cx| window.draw(cx).clear());
    assert!(cx.debug_bounds("chart-ages").is_some());
    let segment = cx
        .debug_bounds("disk-0")
        .expect("the safe artifacts segment");
    cx.simulate_event(MouseMoveEvent {
        position: segment.center(),
        pressed_button: None,
        modifiers: Modifiers::default(),
    });
    cx.executor().advance_clock(Duration::from_millis(700));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    assert!(cx.debug_bounds("tooltip").is_some());
    let legend = cx
        .debug_bounds("disk-legend-2")
        .expect("the global caches legend item");
    cx.simulate_mouse_move(legend.center(), None, Modifiers::default());
    cx.executor().advance_clock(Duration::from_millis(700));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    let tip = cx.debug_bounds("tooltip").expect("the legend tooltip");
    assert!(tip.origin.y >= legend.bottom());
}

#[gpui::test]
fn the_titlebar_search_opens_the_palette(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (view, cx) = open(cx, dir.path().join("recent"));
    cx.update(|window, cx| window.draw(cx).clear());
    let search = cx
        .debug_bounds("search")
        .expect("the titlebar search field");
    cx.simulate_click(search.center(), Modifiers::default());
    view.read_with(cx, |app, _| assert!(app.overlays.palette.is_some()));
}

#[gpui::test]
fn the_sidebar_switches_sections_during_a_scan(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (view, cx) = open(cx, dir.path().join("recent"));
    view.update(cx, |app, cx| {
        app.session.roots = vec![dir.path().to_path_buf()];
        let mut activity = Activity::default();
        activity.begin(Step::Folders, None);
        activity.begin(Step::History, None);
        app.session.phase = Phase::Loading(Box::new(activity));
        cx.notify();
    });
    for (label, shown) in [
        ("Global caches", "Global caches are off for this scan"),
        ("History", "Reading the cleanup history"),
        ("Duplicates", "Searched on request once the scan is done"),
    ] {
        cx.update(|window, cx| window.draw(cx).clear());
        let bounds = cx.debug_bounds(label).expect("a sidebar item");
        cx.simulate_click(bounds.center(), Modifiers::default());
        view.read_with(cx, |app, _| {
            assert!(matches!(app.session.phase, Phase::Loading(_)));
            assert_eq!(app.list.tab.label(), label);
        });
        cx.update(|window, cx| window.draw(cx).clear());
        assert!(cx.debug_bounds(shown).is_some(), "{shown} is not shown");
    }
}

pub(super) fn artifact(id: u64, path: &Path) -> Candidate {
    Candidate {
        id,
        path: path.to_path_buf(),
        kind: kuizer::detect::Kind::Dir,
        rule: std::sync::Arc::new(kuizer::detect::RuleInfo {
            name: "node_modules".to_owned(),
            category: Category::Dependencies,
            safety: Safety::Safe,
            ecosystem: "node".to_owned(),
        }),
        project: path.parent().map(Path::to_path_buf),
        usage: kuizer::scan::Usage {
            allocated: id * 4096,
            apparent: id * 4096,
            entries: 1,
            partial: false,
        },
        newest: None,
        identity: None,
        project_last_active: None,
    }
}

#[gpui::test]
fn stopping_a_scan_keeps_what_it_found(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (view, cx) = open(cx, dir.path().join("recent"));
    let cancel = Cancel::new();
    view.update(cx, |app, cx| {
        app.session.roots = vec![PathBuf::from("/p")];
        app.session.cancel = Some(cancel.clone());
        app.session.pending = Part::expected(&app.session.options);
        app.session.phase = Phase::Loading(Box::default());
        app.take(
            vec![Update::Loaded(Loaded::History(
                Err("unreadable".to_owned()),
            ))],
            cx,
        );
        app.stop_scan(cx);
        let report = app
            .report()
            .expect("a stop before any artifact still shows results");
        assert!(report.candidates.is_empty());
        assert!(report.history.is_err(), "a finished part is kept");
        let scanned = report.scanned;

        app.session.cancel = Some(cancel.clone());
        app.session.stale = Some(scanned);
        app.session.streamed = true;
        app.take(
            vec![Update::Loaded(Loaded::Found(vec![artifact(
                2,
                Path::new("/p/a/node_modules"),
            )]))],
            cx,
        );
        app.stop_scan(cx);
        let report = app.report().expect("the results so far stay");
        assert_eq!(report.candidates.len(), 1);
        assert!(report.summary.cancelled);
        assert!(!app.refreshing());
    });
    assert!(cancel.is_cancelled());
}

#[gpui::test]
fn parts_fill_the_shown_scan_and_its_rows_can_be_deleted(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (view, cx) = open(cx, dir.path().join("recent"));
    let (a, b) = (
        Path::new("/p/a/node_modules"),
        Path::new("/p/b/node_modules"),
    );
    view.update(cx, |app, cx| {
        app.session.roots = vec![PathBuf::from("/p")];
        app.session.cancel = Some(Cancel::new());
        app.session.pending = Part::expected(&app.session.options);
        app.session.phase = Phase::Loading(Box::default());
        let history = Loaded::History(Err("unreadable".to_owned()));
        app.take(
            vec![
                Update::Loaded(history),
                Update::Loaded(Loaded::Found(vec![artifact(2, b)])),
            ],
            cx,
        );
        let report = app.report().expect("the first artifacts are shown");
        assert_eq!(report.candidates.len(), 1);
        assert!(report.history.is_err(), "an earlier part is kept");
        assert!(app.refreshing());
        assert!(!app.session.pending.contains(&Part::History));

        app.select((Source::Artifact, 2));
        app.take(
            vec![
                Update::Loaded(Loaded::Found(vec![artifact(5, a)])),
                Update::Loaded(Loaded::ArtifactsDone {
                    issues: Vec::new(),
                    summary: app.report().unwrap().summary,
                    disks: Vec::new(),
                }),
            ],
            cx,
        );
        let paths: Vec<&Path> = app
            .report()
            .unwrap()
            .candidates
            .iter()
            .map(|candidate| candidate.path.as_path())
            .collect();
        assert_eq!(paths, [a, b], "largest first");

        let mut saved = Report::empty(&app.session.roots, &app.session.options);
        saved.candidates = vec![artifact(9, Path::new("/p/old/node_modules"))];
        saved.history = Ok(HistoryView::new(
            kuizer::history::History::default(),
            kuizer::history::TrashSnapshot {
                known: true,
                ..Default::default()
            },
        ));
        app.take(vec![Update::Cached(Box::new(saved))], cx);
        let report = app.report().unwrap();
        assert_eq!(report.candidates.len(), 2, "streamed artifacts win");
        assert!(report.history.is_err(), "a fresh part is never replaced");
    });
    view.read_with(cx, |app, _| {
        assert_eq!(
            app.trash_lock(),
            None,
            "rows the running scan found can go before it finishes"
        );
    });

    view.update(cx, |app, cx| {
        let summary = app.report().unwrap().summary;
        let artifacts = Loaded::Artifacts {
            candidates: vec![artifact(5, a), artifact(2, b)],
            issues: Vec::new(),
            summary,
            disks: Vec::new(),
        };
        app.take(
            vec![
                Update::Loaded(artifacts),
                Update::Loaded(Loaded::Repos(Vec::new())),
                Update::Done(Ok(SystemTime::UNIX_EPOCH)),
            ],
            cx,
        );
        assert!(!app.refreshing());
        assert_eq!(app.list.selection, HashSet::from([(Source::Artifact, 2)]));
        assert_eq!(app.report().unwrap().scanned, SystemTime::UNIX_EPOCH);
        assert_eq!(app.trash_lock(), None);
    });
}

#[gpui::test]
fn an_item_deleted_mid_scan_stays_gone_when_the_scan_delivers_it(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (view, cx) = open(cx, dir.path().join("recent"));
    let (a, b) = (
        Path::new("/p/a/node_modules"),
        Path::new("/p/b/node_modules"),
    );
    view.update(cx, |app, cx| {
        app.session.roots = vec![PathBuf::from("/p")];
        app.session.cancel = Some(Cancel::new());
        app.session.pending = Part::expected(&app.session.options);
        app.session.phase = Phase::Loading(Box::default());
        let found = Loaded::Found(vec![artifact(1, a), artifact(2, b)]);
        app.take(vec![Update::Loaded(found)], cx);
        app.select((Source::Artifact, 1));
        app.select((Source::Artifact, 2));
    });
    view.update(cx, |app, cx| {
        let history = || {
            Ok(HistoryView::new(
                kuizer::history::History::default(),
                kuizer::history::TrashSnapshot {
                    known: true,
                    ..Default::default()
                },
            ))
        };
        let outcome = Outcome {
            removals: vec![(
                Source::Artifact,
                kuizer::delete::Removal {
                    candidate: artifact(2, b),
                    status: kuizer::delete::Status::Removed,
                    error: None,
                },
            )],
            mode: DeleteMode::Permanent,
            history_error: None,
        };
        app.finish(Some((outcome, history())), cx);
        assert_eq!(app.list.selection, HashSet::from([(Source::Artifact, 1)]));
        assert_eq!(
            app.report().unwrap().candidates.len(),
            2,
            "the deleted row fades out before it leaves"
        );
        let summary = app.report().unwrap().summary;
        let artifacts = Loaded::Artifacts {
            candidates: vec![artifact(1, a), artifact(2, b)],
            issues: Vec::new(),
            summary,
            disks: Vec::new(),
        };
        app.take(vec![Update::Loaded(artifacts)], cx);
    });
    cx.executor().advance_clock(motion::STEP);
    cx.run_until_parked();
    view.update(cx, |app, _| {
        let report = app.report().unwrap();
        assert_eq!(report.candidates.len(), 1, "it leaves once the fade ends");
        assert_eq!(report.candidates[0].path, a);
    });
    view.update(cx, |app, cx| {
        let summary = app.report().unwrap().summary;
        let artifacts = Loaded::Artifacts {
            candidates: vec![artifact(7, b), artifact(8, a)],
            issues: Vec::new(),
            summary,
            disks: Vec::new(),
        };
        app.take(
            vec![
                Update::Loaded(Loaded::History(Err("read before the deletion".to_owned()))),
                Update::Loaded(artifacts),
            ],
            cx,
        );
        let report = app.report().unwrap();
        assert_eq!(report.candidates.len(), 1, "the deleted folder is not back");
        assert_eq!(report.candidates[0].path, a);
        assert!(report.history.is_ok(), "the history read after it is kept");
    });
}

#[gpui::test]
fn a_batch_a_refresh_throws_away_changes_nothing_on_screen(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (view, cx) = open(cx, dir.path().join("recent"));
    view.update(cx, |app, cx| {
        app.session.roots = vec![PathBuf::from("/p")];
        let mut report = Report::empty(&app.session.roots, &app.session.options);
        report.candidates = vec![artifact(2, Path::new("/p/a/node_modules"))];
        app.session.phase = Phase::Ready(Box::new(report));
        app.session.cancel = Some(Cancel::new());
        app.refresh_stats();
        app.refresh_rows();
        app.select((Source::Artifact, 2));
        let (stats, rows) = (app.session.report_generation, app.list.rows_generation);

        let batch = vec![artifact(5, Path::new("/p/b/node_modules"))];
        app.take(vec![Update::Loaded(Loaded::Found(batch))], cx);
        assert_eq!(app.report().unwrap().candidates.len(), 1, "it is dropped");
        assert_eq!(
            (app.session.report_generation, app.list.rows_generation),
            (stats, rows),
            "and nothing is counted or listed again"
        );

        app.session.streamed = true;
        let batch = vec![artifact(5, Path::new("/p/b/node_modules"))];
        app.take(vec![Update::Loaded(Loaded::Found(batch))], cx);
        assert_eq!(app.report().unwrap().candidates.len(), 2);
        assert_eq!(app.list.rows.len(), 2);
        assert_eq!(app.session.report_stats.artifacts.all.count, 2);
        assert_eq!(app.list.selection, HashSet::from([(Source::Artifact, 2)]));
        assert_eq!(app.list.selection.stats().total().all.bytes, 2 * 4096);
    });
}

#[gpui::test]
fn a_failed_exclude_keeps_the_item_and_says_why(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (view, cx) = scanned(cx, dir.path(), Tab::Artifacts);
    let blocker = dir.path().join("blocker");
    std::fs::write(&blocker, "not a folder").unwrap();
    view.update(cx, |app, _| {
        app.config_path = Some(blocker.join("config.toml"));
    });
    right_click(cx, "artifact-row-0");
    click_item(cx, "menu-Always exclude");
    click_item(cx, "confirm-exclude");
    view.read_with(cx, |app, _| {
        assert_eq!(app.report().unwrap().candidates.len(), 1);
        assert!(app.session.excluded.is_empty());
        assert!(app.session.excluding.is_empty(), "the write is over");
    });
    toast_is(&view, cx, |notice| matches!(notice, Notice::Failed(_)));
}

#[gpui::test]
fn the_sidebar_collapses_to_icons_and_back(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = project(dir.path());
    let (view, cx) = open(cx, dir.path().join("recent"));
    type_text(cx, &root.display().to_string());
    cx.simulate_keystrokes("enter");
    wait_for_scan(&view, cx);
    cx.update(|window, cx| window.draw(cx).clear());
    let wide = cx.debug_bounds("sidebar").expect("the sidebar").size.width;
    cx.simulate_keystrokes("ctrl-b");
    view.read_with(cx, |app, _| assert!(app.chrome.collapsed));
    cx.update(|window, cx| window.draw(cx).clear());
    let narrow = cx.debug_bounds("sidebar").expect("the sidebar").size.width;
    assert!(narrow < wide / 2.);
    let history = cx.debug_bounds("History").expect("a sidebar icon");
    cx.simulate_click(history.center(), Modifiers::default());
    view.read_with(cx, |app, _| assert_eq!(app.list.tab, Tab::History));
    let toggle = cx
        .debug_bounds("sidebar-toggle")
        .expect("the collapse button");
    cx.simulate_click(toggle.center(), Modifiers::default());
    view.read_with(cx, |app, _| assert!(!app.chrome.collapsed));
}

#[gpui::test]
fn the_sidebar_settings_item_opens_the_settings(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = project(dir.path());
    let (view, cx) = open(cx, dir.path().join("recent"));
    type_text(cx, &root.display().to_string());
    cx.simulate_keystrokes("enter");
    wait_for_scan(&view, cx);
    cx.update(|window, cx| window.draw(cx).clear());
    let settings = cx
        .debug_bounds("sidebar-settings")
        .expect("the settings item");
    cx.simulate_click(settings.center(), Modifiers::default());
    view.read_with(cx, |app, _| assert!(app.overlays.settings.is_some()));
}

#[gpui::test]
fn the_sidebar_opens_collapsed_when_configured(cx: &mut TestAppContext) {
    let mut config = Config::default();
    config.appearance.sidebar = Sidebar::Collapsed;
    let (view, cx) = cx.add_window_view(|window, cx| {
        Kuizer::new(
            config,
            Vec::new(),
            Recent::load(None),
            Snapshots::new(None),
            "Noto Sans".into(),
            window,
            cx,
        )
    });
    view.read_with(cx, |app, _| assert!(app.chrome.collapsed));
}

#[gpui::test]
fn the_sidebar_shows_what_each_section_frees(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = project(dir.path());
    let (view, cx) = open(cx, dir.path().join("recent"));
    type_text(cx, &root.display().to_string());
    cx.simulate_keystrokes("enter");
    wait_for_scan(&view, cx);
    view.read_with(cx, |app, _| {
        let report = app.report().expect("a report");
        assert_eq!(
            app.tab_figure(Tab::Artifacts),
            Some(theme::bytes(report.total(None)))
        );
        assert_eq!(app.tab_figure(Tab::Overview), None);
        assert_eq!(app.tab_figure(Tab::Issues), None);
    });
}

#[gpui::test]
fn hovering_a_section_explains_it(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = project(dir.path());
    let (view, cx) = open(cx, dir.path().join("recent"));
    type_text(cx, &root.display().to_string());
    cx.simulate_keystrokes("enter");
    wait_for_scan(&view, cx);
    cx.update(|window, cx| window.draw(cx).clear());
    let history = cx.debug_bounds("History").expect("a sidebar item");
    let issues = cx.debug_bounds("Issues").expect("a sidebar item");
    cx.simulate_mouse_move(history.center(), None, Modifiers::default());
    cx.executor().advance_clock(Duration::from_millis(600));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    let tip = cx.debug_bounds("tooltip").expect("the History tooltip");
    assert!(tip.origin.y >= history.bottom());
    cx.simulate_mouse_move(issues.center(), None, Modifiers::default());
    cx.update(|window, cx| window.draw(cx).clear());
    let next = cx
        .debug_bounds("tooltip")
        .expect("the Issues tooltip opens at once");
    assert!(next.origin.y >= issues.bottom());
}

fn scroll(cx: &mut VisualTestContext, lines: f32, modifiers: Modifiers) {
    cx.simulate_event(ScrollWheelEvent {
        position: point(px(400.), px(300.)),
        delta: ScrollDelta::Lines(point(0., lines)),
        modifiers,
        touch_phase: TouchPhase::Moved,
    });
}

#[gpui::test]
fn ctrl_plus_minus_and_zero_change_the_zoom(cx: &mut TestAppContext) {
    cx.update(bind_keys);
    let dir = tempfile::tempdir().unwrap();
    let (view, cx) = open(cx, dir.path().join("recent"));
    cx.simulate_keystrokes("ctrl-= ctrl-shift-=");
    view.read_with(cx, |app, _| assert_eq!(app.config.appearance.zoom, 120));
    cx.update(|window, _| assert_eq!(window.rem_size(), px(19.2)));
    cx.simulate_keystrokes("ctrl-- ctrl-- ctrl-- ctrl--");
    view.read_with(cx, |app, _| {
        assert_eq!(app.config.appearance.zoom, 80);
        assert!(app.start.folder.is_empty());
    });
    cx.run_until_parked();
    let written = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
    assert!(written.contains("zoom = 80"), "{written}");
    cx.simulate_keystrokes("ctrl-0");
    view.read_with(cx, |app, _| assert_eq!(app.config.appearance.zoom, 100));
}

#[gpui::test]
fn ctrl_scroll_zooms_and_plain_scroll_does_not(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (view, cx) = open(cx, dir.path().join("recent"));
    scroll(cx, 1., Modifiers::control());
    scroll(cx, 1., Modifiers::control());
    view.read_with(cx, |app, _| assert_eq!(app.config.appearance.zoom, 120));
    scroll(cx, -1., Modifiers::control());
    view.read_with(cx, |app, _| assert_eq!(app.config.appearance.zoom, 110));
    scroll(cx, 1., Modifiers::default());
    view.read_with(cx, |app, _| assert_eq!(app.config.appearance.zoom, 110));
}

#[gpui::test]
fn a_known_folder_opens_with_the_last_scan_while_it_refreshes(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = project(dir.path());
    let (view, cx) = open(cx, dir.path().join("recent"));
    type_text(cx, &root.display().to_string());
    cx.simulate_keystrokes("enter");
    assert!(!wait_until_shown(&view, cx));
    wait_for_refresh(&view, cx);
    wait_for_saved(dir.path());

    let id = view.update(cx, |app, cx| {
        let report = app.report().unwrap();
        let saved = crate::snapshot::load(&app.session.snapshots, &report.roots, &report.options)
            .expect("the saved scan");
        app.set_phase(Phase::Loading(Box::default()), cx);
        app.session.cancel = Some(Cancel::new());
        app.session.pending = Part::expected(&app.session.options);
        app.take(vec![Update::Cached(Box::new(saved))], cx);
        let report = app.report().expect("the saved scan comes first");
        assert_eq!(report.candidates.len(), 1);
        report.candidates[0].id
    });
    cx.update(|window, cx| window.draw(cx).clear());
    assert!(cx.debug_bounds("stale").is_some());
    assert!(cx.debug_bounds("loader").is_some());

    view.update(cx, |app, cx| {
        app.toggle(Source::Artifact, id, cx);
        app.delete(None, cx);
        assert!(
            app.session.cleaning.is_none(),
            "stale items cannot be deleted"
        );
        assert_eq!(app.selected().len(), 1);
    });

    view.update(cx, |app, cx| app.rescan(cx));
    wait_for_refresh(&view, cx);
    view.read_with(cx, |app, _| {
        assert!(matches!(app.session.phase, Phase::Ready(_)));
        assert_eq!(app.selected().len(), 1);
        assert!(app.session.stale.is_none());
        assert_eq!(app.trash_lock(), None);
    });
}

#[gpui::test]
fn a_narrow_titlebar_never_overlaps_while_refreshing(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = project(dir.path());
    let (view, cx) = open(cx, dir.path().join("recent"));
    type_text(cx, &root.display().to_string());
    cx.simulate_keystrokes("enter");
    wait_for_refresh(&view, cx);
    view.update(cx, |app, cx| {
        app.session.stale = Some(SystemTime::now());
        cx.notify();
    });

    cx.simulate_resize(gpui::size(px(860.), px(520.)));
    cx.update(|window, cx| window.draw(cx).clear());
    let mut bounds = |selector: &'static str| {
        cx.debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} is painted"))
    };
    let brand = bounds("brand");
    let search = bounds("search");
    let stale = bounds("stale");
    let actions = bounds("titlebar-actions");
    let titlebar = bounds("titlebar");
    assert!(brand.right() <= search.left());
    assert!(search.right() <= stale.left());
    assert!(actions.right() <= titlebar.right());
}

#[gpui::test]
fn the_fresh_scan_keeps_the_selection_by_path(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = project(dir.path());
    let gone = root.join("old").join("node_modules");
    std::fs::create_dir_all(gone.join("x")).unwrap();
    std::fs::write(root.join("old").join("package.json"), "{}").unwrap();
    std::fs::write(gone.join("x").join("index.js"), vec![b'x'; 8 * 1024]).unwrap();
    let (view, cx) = open(cx, dir.path().join("recent"));
    type_text(cx, &root.display().to_string());
    cx.simulate_keystrokes("enter");
    wait_for_refresh(&view, cx);
    view.update(cx, |app, cx| {
        let Phase::Ready(report) = &mut app.session.phase else {
            panic!("expected a report");
        };
        assert_eq!(report.candidates.len(), 2);
        for candidate in &mut report.candidates {
            candidate.id += 100;
        }
        let keys: Vec<(Source, u64)> = report
            .candidates
            .iter()
            .map(|candidate| (Source::Artifact, candidate.id))
            .collect();
        app.refresh_stats();
        for key in keys {
            app.select(key);
        }
        app.refresh_rows();
        cx.notify();
    });
    std::fs::remove_dir_all(&gone).unwrap();

    view.update(cx, |app, cx| app.rescan(cx));
    view.read_with(cx, |app, _| {
        assert!(app.refreshing());
        assert!(matches!(app.session.phase, Phase::Ready(_)));
        assert_eq!(app.list.selection.len(), 2);
    });
    wait_for_refresh(&view, cx);
    view.read_with(cx, |app, _| {
        let report = app.report().expect("the fresh scan");
        assert_eq!(report.candidates.len(), 1);
        let kept = &report.candidates[0];
        assert!(kept.id < 100);
        assert!(kept.path.ends_with("site/node_modules"));
        assert_eq!(
            app.list.selection,
            HashSet::from([(Source::Artifact, kept.id)])
        );
    });
}

const ISOLATED: &str = "KUIZER_GUI_TEST_BASE";

/// Runs every ignored `*_inside` deletion test sequentially in one child process whose HOME and
/// XDG folders point into a temporary directory, avoiding spawning a fresh test binary per case.
#[test]
fn isolated_deletion_cases() {
    let dir = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(dir.path()).unwrap();
    let home = base.join("home");
    std::fs::create_dir_all(home.join(".local/share")).unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["_inside", "--ignored", "--test-threads=1"])
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env_remove("RUSTUP_HOME")
        .env_remove("NVM_DIR")
        .env_remove("FNM_DIR")
        .env_remove("VOLTA_HOME")
        .env_remove("PYENV_ROOT")
        .env_remove("UV_PYTHON_INSTALL_DIR")
        .env_remove("SDKMAN_DIR")
        .env(ISOLATED, &base)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let expected = if cfg!(unix) { 7 } else { 6 };
    assert!(
        output.status.success() && stdout.contains(&format!("{expected} passed")),
        "{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn isolated_base() -> Option<PathBuf> {
    let base = std::env::var_os(ISOLATED).map(PathBuf::from)?;
    for name in ["work", "recent", "bin", "home"] {
        let path = base.join(name);
        if path.is_dir() {
            let _ = std::fs::remove_dir_all(&path);
        } else {
            let _ = std::fs::remove_file(&path);
        }
    }
    std::fs::create_dir_all(base.join("home/.local/share")).unwrap();
    Some(base)
}

fn wait_until(
    view: &Entity<Kuizer>,
    cx: &mut VisualTestContext,
    what: &str,
    done: impl Fn(&Kuizer) -> bool,
) {
    for _ in 0..1000 {
        cx.executor().advance_clock(POLL);
        cx.run_until_parked();
        if view.read_with(cx, |app, _| done(app)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("{what} did not happen");
}

fn toast_is(view: &Entity<Kuizer>, cx: &mut VisualTestContext, check: fn(&Notice) -> bool) {
    let shown = view.read_with(cx, |app, _| {
        app.overlays
            .toast
            .as_ref()
            .is_some_and(|toast| check(&toast.notice))
    });
    assert!(shown, "unexpected toast");
}

#[gpui::test]
#[ignore = "runs inside isolated_deletion_cases"]
fn deleting_the_selection_removes_it_for_good_inside(cx: &mut TestAppContext) {
    let Some(base) = isolated_base() else {
        return;
    };
    cx.update(bind_keys);
    let root = project(&base.join("work"));
    let modules = std::fs::canonicalize(root.join("site").join("node_modules")).unwrap();
    let (view, cx) = open(cx, base.join("recent"));
    type_text(cx, &root.display().to_string());
    cx.simulate_keystrokes("enter");
    wait_for_refresh(&view, cx);
    let id = view.read_with(cx, |app, _| app.report().unwrap().candidates[0].id);
    view.update(cx, |app, cx| app.toggle(Source::Artifact, id, cx));
    cx.update(|window, cx| window.draw(cx).clear());
    assert!(cx.debug_bounds("permanent-warning").is_some());
    cx.simulate_keystrokes("enter");
    wait_until(&view, cx, "the removal", |app| {
        matches!(
            &app.overlays.toast,
            Some(Toast {
                notice: Notice::Info(_),
                ..
            })
        )
    });
    cx.executor().advance_clock(motion::STEP);
    cx.run_until_parked();
    assert!(!modules.exists());
    assert!(
        kuizer::history::trashed_paths().is_none_or(|trashed| !trashed.contains(&modules)),
        "nothing goes to the trash"
    );
    view.read_with(cx, |app, _| {
        assert!(
            app.overlays.dialog.is_none(),
            "a clean run needs no summary dialog"
        );
        assert!(app.report().unwrap().candidates.is_empty());
    });
    let path = kuizer::history::default_path().unwrap();
    let runs = kuizer::history::load(&path).unwrap().runs;
    assert_eq!(runs[0].deleted[0].mode, kuizer::delete::Mode::Permanent);
}

#[gpui::test]
#[ignore = "runs inside isolated_deletion_cases"]
fn trash_mode_moves_the_selection_to_the_trash_inside(cx: &mut TestAppContext) {
    let Some(base) = isolated_base() else {
        return;
    };
    cx.update(bind_keys);
    let root = project(&base.join("work"));
    let modules = std::fs::canonicalize(root.join("site").join("node_modules")).unwrap();
    let (view, cx) = open(cx, base.join("recent"));
    view.update(cx, |app, _| app.config.delete.mode = DeleteMode::Trash);
    type_text(cx, &root.display().to_string());
    cx.simulate_keystrokes("enter");
    wait_for_refresh(&view, cx);
    let id = view.read_with(cx, |app, _| app.report().unwrap().candidates[0].id);
    view.update(cx, |app, cx| app.toggle(Source::Artifact, id, cx));
    cx.update(|window, cx| window.draw(cx).clear());
    assert!(cx.debug_bounds("trash-note").is_some());
    assert!(cx.debug_bounds("permanent-warning").is_none());
    cx.simulate_keystrokes("enter");
    wait_until(&view, cx, "the removal", |app| {
        matches!(
            &app.overlays.toast,
            Some(Toast {
                notice: Notice::Info(text),
                ..
            }) if text.contains("moved to the trash")
        )
    });
    assert!(!modules.exists());
    assert!(
        kuizer::history::trashed_paths().is_none_or(|trashed| trashed.contains(&modules)),
        "the folder is in the trash"
    );
    let path = kuizer::history::default_path().unwrap();
    let runs = kuizer::history::load(&path).unwrap().runs;
    assert_eq!(runs[0].deleted[0].mode, kuizer::delete::Mode::Trash);
}

#[gpui::test]
#[ignore = "runs inside isolated_deletion_cases"]
fn a_clean_adds_its_run_to_the_history_on_screen_inside(cx: &mut TestAppContext) {
    let Some(base) = isolated_base() else {
        return;
    };
    cx.update(bind_keys);
    let root = project(&base.join("work"));
    let modules = std::fs::canonicalize(root.join("site").join("node_modules")).unwrap();
    let (view, cx) = open(cx, base.join("recent"));
    view.update(cx, |app, _| app.config.delete.mode = DeleteMode::Trash);
    type_text(cx, &root.display().to_string());
    cx.simulate_keystrokes("enter");
    wait_for_refresh(&view, cx);
    let shown_only = kuizer::history::Run {
        id: 41,
        ..kuizer::history::Run::new(
            kuizer::history::now(),
            &[],
            vec![kuizer::history::Entry {
                deleted_at: kuizer::history::now(),
                path: PathBuf::from("/p/old/node_modules"),
                rule: "node-modules".to_owned(),
                category: "dependencies".to_owned(),
                mode: kuizer::delete::Mode::Permanent,
                bytes: 7,
                size: "7 B".to_owned(),
                restores: None,
            }],
        )
    };
    view.update(cx, |app, cx| {
        let history = kuizer::history::History {
            next_session: 42,
            runs: vec![shown_only],
            ..Default::default()
        };
        let trash = kuizer::history::TrashSnapshot::read(&history);
        if let Phase::Ready(report) = &mut app.session.phase {
            report.history = Ok(HistoryView::new(history, trash));
        }
        let id = app.report().unwrap().candidates[0].id;
        app.toggle(Source::Artifact, id, cx);
    });
    cx.update(|window, cx| window.draw(cx).clear());
    cx.simulate_keystrokes("enter");
    wait_until(&view, cx, "the removal", |app| {
        app.session.cleaning.is_none()
    });

    let on_disk = kuizer::history::load(&kuizer::history::default_path().unwrap())
        .unwrap()
        .runs;
    assert_eq!(on_disk.len(), 1);
    view.read_with(cx, |app, _| {
        let view = app.report().unwrap().history.as_ref().unwrap();
        let runs = &view.history.runs;
        assert_eq!(runs.len(), 2, "the history was not read again");
        assert_eq!(runs[0].id, 41);
        assert_eq!(runs[1], on_disk[0]);
        assert_eq!(runs[1].deleted[0].path, modules);
        assert_eq!(view.history.cumulative.runs, 2);
        let bytes = runs[1].total.bytes;
        if view.trash.known {
            assert_eq!(
                view.trash_state(1),
                TrashState::Restorable { items: 1, bytes }
            );
            assert_eq!(view.stats.in_trash, bytes);
        }
        assert_eq!(view.stats.freed, 7);
    });
}

#[gpui::test]
fn trash_mode_still_says_delete_for_what_cannot_be_trashed(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (view, cx) = open(cx, dir.path().join("recent"));
    view.update(cx, |app, _| {
        app.config.delete.mode = DeleteMode::Trash;
        let toolchain = artifact(1, Path::new("/t/v18"));
        let modules = artifact(2, Path::new("/p/node_modules"));
        let stats = SelectionStats::default();
        assert_eq!(app.delete_label(&stats), "Move to trash");
        let stats = SelectionStats::of([(Source::Toolchain, &toolchain)].into_iter());
        assert_eq!(app.delete_label(&stats), "Delete");
        let stats = SelectionStats::of(
            [
                (Source::Toolchain, &toolchain),
                (Source::Artifact, &modules),
            ]
            .into_iter(),
        );
        assert_eq!(app.delete_label(&stats), "Move to trash");
        app.config.delete.mode = DeleteMode::Permanent;
        assert_eq!(app.delete_label(&stats), "Delete");
    });
}

#[cfg(unix)]
#[gpui::test]
#[ignore = "runs inside isolated_deletion_cases"]
fn docker_objects_are_removed_through_the_engine_inside(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt;

    let Some(base) = isolated_base() else {
        return;
    };
    cx.update(bind_keys);
    let bin = base.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let engine = bin.join("docker");
    std::fs::write(
        &engine,
        r#"#!/bin/sh
echo "$*" >> "$(dirname "$0")/calls"
case "$1 $2" in
  "image ls") printf 'sha256:aaaaaaaaaaaaaaaa\t1.5GB\t3 weeks ago\n' ;;
  "image inspect") echo 0 ;;
  "volume ls") echo pgdata ;;
  *) exit 0 ;;
esac
"#,
    )
    .unwrap();
    std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o755)).unwrap();
    let root = project(&base.join("work"));
    let (view, cx) = open(cx, base.join("recent"));
    view.update(cx, |app, _| {
        app.session.options.docker = true;
        app.config.docker.engines = vec![engine.display().to_string()];
    });
    type_text(cx, &root.display().to_string());
    cx.simulate_keystrokes("enter");
    wait_for_refresh(&view, cx);
    let image = view.read_with(cx, |app, _| {
        let docker = &app.report().unwrap().docker;
        assert_eq!(docker.len(), 2);
        assert_eq!(docker[0].1.rule.safety, Safety::Safe);
        assert_eq!(docker[1].0.object, kuizer::docker::Object::Volume);
        assert_eq!(docker[1].1.rule.safety, Safety::Review);
        docker[0].1.id
    });

    cx.simulate_keystrokes("shift-5");
    view.read_with(cx, |app, _| assert_eq!(app.list.tab, Tab::Docker));
    cx.update(|window, cx| window.draw(cx).clear());
    assert!(cx.debug_bounds("docker-row-1").is_some());
    let target = view.read_with(cx, |app, _| app.targets(Tab::Docker)[0].clone());
    let menu = view.read_with(cx, |app, _| app.menu_items(&target));
    let reveal = menu
        .iter()
        .find(|item| item.choice == Choice::Reveal)
        .unwrap();
    assert!(!reveal.enabled, "a Docker object has no folder to reveal");
    view.update(cx, |app, cx| app.toggle(Source::Docker, image, cx));
    cx.update(|window, cx| window.draw(cx).clear());
    assert!(cx.debug_bounds("permanent-warning").is_some());
    cx.simulate_keystrokes("enter");
    wait_until(&view, cx, "the removal", |app| {
        app.session.cleaning.is_none()
    });
    cx.executor().advance_clock(motion::STEP);
    cx.run_until_parked();
    let calls = std::fs::read_to_string(bin.join("calls")).unwrap();
    assert_eq!(calls.lines().last(), Some("image rm aaaaaaaaaaaaaaaa"));
    view.read_with(cx, |app, _| {
        assert!(
            app.overlays.dialog.is_none(),
            "a clean run needs no summary dialog"
        );
        assert_eq!(app.report().unwrap().docker.len(), 1);
    });
}

#[gpui::test]
#[ignore = "runs inside isolated_deletion_cases"]
fn toolchains_are_deleted_for_good_inside(cx: &mut TestAppContext) {
    let Some(base) = isolated_base() else {
        return;
    };
    cx.update(bind_keys);
    let versions = base.join("home/.nvm/versions/node");
    let old = versions.join("v18.17.0");
    std::fs::create_dir_all(old.join("bin")).unwrap();
    std::fs::write(old.join("bin/node"), vec![0u8; 4096]).unwrap();
    std::fs::create_dir_all(versions.join("v20.11.0/bin")).unwrap();
    let root = project(&base.join("work"));
    std::fs::write(root.join("site/.nvmrc"), "20\n").unwrap();
    let (view, cx) = open(cx, base.join("recent"));
    view.update(cx, |app, _| app.session.options.toolchains = true);
    type_text(cx, &root.display().to_string());
    cx.simulate_keystrokes("enter");
    wait_for_refresh(&view, cx);
    let (id, pinned) = view.read_with(cx, |app, _| {
        let toolchains = &app.report().unwrap().toolchains;
        assert_eq!(toolchains.len(), 2);
        let find = |version: &str| {
            toolchains
                .iter()
                .find(|(toolchain, _)| toolchain.version == version)
                .unwrap()
        };
        assert_eq!(find("v18.17.0").1.rule.safety, Safety::Safe);
        (find("v18.17.0").1.id, find("v20.11.0").0.clone())
    });
    assert_eq!(pinned.safety(), Safety::Review);
    assert!(pinned.note().contains(".nvmrc"), "{}", pinned.note());

    cx.simulate_keystrokes("shift-4");
    view.read_with(cx, |app, _| assert_eq!(app.list.tab, Tab::Toolchains));
    cx.update(|window, cx| window.draw(cx).clear());
    assert!(cx.debug_bounds("toolchain-row-1").is_some());
    view.update(cx, |app, cx| app.toggle(Source::Toolchain, id, cx));
    cx.update(|window, cx| window.draw(cx).clear());
    assert!(cx.debug_bounds("permanent-warning").is_some());
    cx.simulate_keystrokes("enter");
    wait_until(&view, cx, "the removal", |app| {
        app.session.cleaning.is_none()
    });
    cx.executor().advance_clock(motion::STEP);
    cx.run_until_parked();
    assert!(!old.exists());
    assert!(versions.join("v20.11.0").exists());
    view.read_with(cx, |app, _| {
        assert!(
            app.overlays.dialog.is_none(),
            "a clean run needs no summary dialog"
        );
        assert_eq!(app.report().unwrap().toolchains.len(), 1);
    });
    let path = kuizer::history::default_path().unwrap();
    let runs = kuizer::history::load(&path).unwrap().runs;
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].deleted[0].mode, kuizer::delete::Mode::Permanent);
}

#[gpui::test]
fn a_toast_hides_itself(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (view, cx) = open(cx, dir.path().join("recent"));
    view.update(cx, |app, cx| {
        app.show_toast(Notice::Info("2 items deleted".to_owned()), cx);
    });
    cx.update(|window, cx| window.draw(cx).clear());
    assert!(cx.debug_bounds("toast").is_some());
    cx.executor().advance_clock(TOAST_TIME - POLL * 2);
    cx.run_until_parked();
    view.read_with(cx, |app, _| assert!(app.overlays.toast.is_some()));
    cx.executor().advance_clock(POLL * 3);
    cx.run_until_parked();
    view.read_with(cx, |app, _| assert!(app.overlays.toast.is_none()));
}

pub(super) fn scanned<'a>(
    cx: &'a mut TestAppContext,
    dir: &Path,
    tab: Tab,
) -> (Entity<Kuizer>, &'a mut VisualTestContext) {
    let root = project(dir);
    let (view, cx) = open(cx, dir.join("recent"));
    type_text(cx, &root.display().to_string());
    cx.simulate_keystrokes("enter");
    wait_for_refresh(&view, cx);
    view.update(cx, |app, cx| {
        app.list.tab = tab;
        cx.notify();
    });
    cx.update(|window, cx| window.draw(cx).clear());
    (view, cx)
}

fn right_click(cx: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
    let bounds = cx.debug_bounds(selector).expect("the row is painted");
    cx.simulate_mouse_down(bounds.center(), MouseButton::Right, Modifiers::default());
    cx.simulate_mouse_up(bounds.center(), MouseButton::Right, Modifiers::default());
    cx.update(|window, cx| window.draw(cx).clear());
    bounds
}

fn click_item(cx: &mut VisualTestContext, selector: &'static str) {
    let item = cx.debug_bounds(selector).expect("the menu item is painted");
    cx.simulate_click(item.center(), Modifiers::default());
    cx.run_until_parked();
}

pub(super) fn menu_target(view: &Entity<Kuizer>, cx: &mut VisualTestContext) -> Option<Target> {
    view.read_with(cx, |app, _| {
        app.overlays.menu.as_ref().map(|menu| menu.target.clone())
    })
}

fn only_artifact(view: &Entity<Kuizer>, cx: &mut VisualTestContext) -> (u64, PathBuf) {
    view.read_with(cx, |app, _| {
        let candidates = &app.report().unwrap().candidates;
        assert_eq!(candidates.len(), 1);
        (candidates[0].id, candidates[0].path.clone())
    })
}

#[gpui::test]
fn right_clicking_a_row_opens_its_menu_there(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (view, cx) = scanned(cx, dir.path(), Tab::Artifacts);
    let (id, path) = only_artifact(&view, cx);
    let row = right_click(cx, "artifact-row-0");
    assert_eq!(
        menu_target(&view, cx),
        Some(Target::Item(Source::Artifact, id, path))
    );
    let menu = cx.debug_bounds("menu").expect("the menu is painted");
    assert!(menu.top() >= row.top() && menu.top() <= row.bottom());
    assert!((menu.left() - row.center().x).abs() < px(1.));
    view.read_with(cx, |app, _| {
        assert!(
            app.list.selection.is_empty(),
            "a right click does not select"
        );
        let labels: Vec<&str> = app
            .menu_items(&app.overlays.menu.as_ref().unwrap().target)
            .iter()
            .map(|item| item.label)
            .collect();
        assert_eq!(
            labels,
            [
                "Reveal in file manager",
                "Copy path",
                "Select",
                "Delete",
                "Always exclude"
            ]
        );
    });
}

#[gpui::test]
fn copy_path_puts_the_full_path_on_the_clipboard(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (view, cx) = scanned(cx, dir.path(), Tab::Artifacts);
    let (_, path) = only_artifact(&view, cx);
    right_click(cx, "artifact-row-0");
    click_item(cx, "menu-Copy path");
    assert!(menu_target(&view, cx).is_none(), "choosing closes the menu");
    let copied = cx.read_from_clipboard().and_then(|item| item.text());
    assert_eq!(copied.as_deref(), Some(path.to_str().unwrap()));
}

#[gpui::test]
fn always_exclude_writes_the_config_and_hides_the_item(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (view, cx) = scanned(cx, dir.path(), Tab::Artifacts);
    let (id, path) = only_artifact(&view, cx);
    let file = dir.path().join("config.toml");
    view.update(cx, |app, cx| app.toggle(Source::Artifact, id, cx));
    cx.update(|window, cx| window.draw(cx).clear());

    // Pressing x opens the inline confirmation with Cancel focused; Enter cancels safely.
    cx.simulate_keystrokes("x");
    view.read_with(cx, |app, _| {
        assert!(matches!(
            &app.overlays.dialog,
            Some(Dialog::Exclude { path: shown, cancel: true, .. }) if shown == &path
        ));
    });
    cx.simulate_keystrokes("enter");
    view.read_with(cx, |app, _| {
        assert!(app.overlays.dialog.is_none());
        assert_eq!(app.list.rows.len(), 1);
    });
    assert!(!file.exists(), "cancelling must not write the config");

    // Choosing Always exclude from the menu opens the inline confirmation; confirming writes it.
    right_click(cx, "artifact-row-0");
    click_item(cx, "menu-Always exclude");
    view.read_with(cx, |app, _| {
        assert!(app.overlays.menu.is_none());
        assert!(matches!(
            app.overlays.dialog,
            Some(Dialog::Exclude { cancel: true, .. })
        ));
    });
    click_item(cx, "confirm-exclude");

    let config = Config::load(Some(&file)).unwrap().config;
    let pattern = kuizer::config::exclude_pattern(&path);
    assert_eq!(config.scan.exclude, std::slice::from_ref(&pattern));
    view.read_with(cx, |app, _| {
        assert!(app.overlays.dialog.is_none());
        assert!(app.overlays.menu.is_none());
        assert!(app.report().unwrap().candidates.is_empty());
        assert!(app.list.rows.is_empty());
        assert!(
            app.list.selection.is_empty(),
            "the selection forgets it too"
        );
        assert_eq!(app.config.scan.exclude, std::slice::from_ref(&pattern));
    });
    toast_is(&view, cx, |notice| matches!(notice, Notice::Info(_)));

    view.update(cx, |app, cx| app.rescan(cx));
    wait_for_refresh(&view, cx);
    view.read_with(cx, |app, _| {
        assert!(
            app.report().unwrap().candidates.is_empty(),
            "a fresh scan skips it"
        );
    });
}

#[gpui::test]
fn escape_or_a_click_outside_closes_the_menu(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (view, cx) = scanned(cx, dir.path(), Tab::Artifacts);
    right_click(cx, "artifact-row-0");
    assert!(menu_target(&view, cx).is_some());
    cx.simulate_keystrokes("escape");
    assert!(menu_target(&view, cx).is_none());

    let row = right_click(cx, "artifact-row-0");
    assert!(menu_target(&view, cx).is_some());
    cx.simulate_click(
        point(row.center().x, row.bottom() + px(200.)),
        Modifiers::default(),
    );
    view.read_with(cx, |app, _| {
        assert!(app.overlays.menu.is_none());
        assert!(
            app.list.selection.is_empty(),
            "the click only closed the menu"
        );
    });
}

#[gpui::test]
fn the_keyboard_opens_the_menu_and_chooses_an_item(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (view, cx) = scanned(cx, dir.path(), Tab::Artifacts);
    let (id, path) = only_artifact(&view, cx);
    cx.simulate_keystrokes("shift-f10");
    view.read_with(cx, |app, _| {
        let menu = app
            .overlays
            .menu
            .as_ref()
            .expect("Shift+F10 opens the menu");
        assert_eq!(
            menu.target,
            Target::Item(Source::Artifact, id, path.clone())
        );
        assert_eq!(menu.active, Some(0));
    });
    cx.update(|window, cx| window.draw(cx).clear());
    let row = cx.debug_bounds("artifact-row-0").unwrap();
    let menu = cx.debug_bounds("menu").unwrap();
    assert!(
        menu.top() >= row.bottom() - px(1.),
        "it opens below the row"
    );

    let active = |cx: &mut VisualTestContext| {
        view.read_with(cx, |app, _| {
            app.overlays.menu.as_ref().and_then(|menu| menu.active)
        })
    };
    cx.simulate_keystrokes("down");
    assert_eq!(active(cx), Some(1));
    cx.simulate_keystrokes("j j");
    assert_eq!(active(cx), Some(3));
    cx.simulate_keystrokes("k");
    assert_eq!(active(cx), Some(2));
    cx.simulate_keystrokes("up up up");
    assert_eq!(active(cx), Some(4), "it wraps around");
    cx.simulate_keystrokes("down down down enter");
    view.read_with(cx, |app, _| {
        assert!(app.overlays.menu.is_none());
        assert!(app.list.selection.contains(&(Source::Artifact, id)));
    });

    cx.simulate_keystrokes("menu");
    assert!(
        menu_target(&view, cx).is_some(),
        "the Menu key opens it too"
    );
    cx.simulate_keystrokes("s");
    view.read_with(cx, |app, _| {
        assert!(app.overlays.menu.is_none());
        assert!(app.list.selection.is_empty(), "S deselects");
    });
}

#[gpui::test]
fn a_right_click_on_the_titlebar_opens_no_item_menu(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (view, cx) = scanned(cx, dir.path(), Tab::Artifacts);
    right_click(cx, "titlebar");
    assert!(menu_target(&view, cx).is_none());
}

#[gpui::test]
#[ignore = "runs inside isolated_deletion_cases"]
fn delete_from_the_menu_removes_that_item_only_inside(cx: &mut TestAppContext) {
    let Some(base) = isolated_base() else {
        return;
    };
    let root = project(&base.join("work"));
    let other = root.join("web").join("node_modules").join("x");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(root.join("web").join("package.json"), "{}").unwrap();
    std::fs::write(other.join("index.js"), vec![b'y'; 8 * 1024]).unwrap();
    let (view, cx) = open(cx, base.join("recent"));
    type_text(cx, &root.display().to_string());
    cx.simulate_keystrokes("enter");
    wait_for_refresh(&view, cx);
    let selected = view.update(cx, |app, cx| {
        app.list.tab = Tab::Artifacts;
        let first = &app.report().unwrap().candidates[0];
        let (id, path) = (first.id, first.path.clone());
        app.toggle(Source::Artifact, id, cx);
        path
    });
    cx.update(|window, cx| window.draw(cx).clear());

    view.update(cx, |app, _| app.session.stale = Some(SystemTime::now()));
    right_click(cx, "artifact-row-1");
    view.read_with(cx, |app, _| {
        let items = app.menu_items(&app.overlays.menu.as_ref().unwrap().target);
        let trash = items
            .iter()
            .find(|item| item.choice == Choice::Trash)
            .unwrap();
        assert!(!trash.enabled, "not while the results are refreshing");
        assert_eq!(trash.tip, app.trash_lock());
    });
    cx.simulate_keystrokes("delete");
    view.read_with(cx, |app, _| assert!(app.session.cleaning.is_none()));
    cx.simulate_keystrokes("escape");
    view.update(cx, |app, _| app.session.stale = None);

    let second = right_click(cx, "artifact-row-1");
    let Some(Target::Item(_, _, path)) = menu_target(&view, cx) else {
        panic!("the menu is open on the second row");
    };
    assert!(second.size.height > px(0.));
    cx.simulate_keystrokes("delete");
    view.read_with(cx, |app, _| {
        assert!(app.overlays.dialog.is_none(), "the menu never asks")
    });
    wait_until(&view, cx, "the removal", |app| {
        app.session.cleaning.is_none()
    });
    assert!(!path.exists());
    assert!(selected.exists(), "the selection is untouched");
    view.read_with(cx, |app, _| assert_eq!(app.selected().len(), 1));
}

#[gpui::test]
#[ignore = "runs inside isolated_deletion_cases"]
fn enter_deletes_the_selection_at_once_but_a_held_enter_never_does_inside(cx: &mut TestAppContext) {
    let Some(base) = isolated_base() else {
        return;
    };
    cx.update(bind_keys);
    let root = project(&base.join("work"));
    let other = root.join("web").join("node_modules").join("x");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(root.join("web").join("package.json"), "{}").unwrap();
    std::fs::write(other.join("index.js"), vec![b'y'; 8 * 1024]).unwrap();
    let (view, cx) = open(cx, base.join("recent"));
    type_text(cx, &root.display().to_string());
    cx.simulate_keystrokes("enter");
    wait_for_refresh(&view, cx);
    let select = |view: &Entity<Kuizer>, cx: &mut VisualTestContext| {
        view.update(cx, |app, cx| {
            let first = &app.report().unwrap().candidates[0];
            let (id, path) = (first.id, first.path.clone());
            app.toggle(Source::Artifact, id, cx);
            path
        })
    };

    let first = select(&view, cx);
    cx.update(|window, cx| window.draw(cx).clear());
    assert!(cx.debug_bounds("summary-bar").is_some());
    cx.simulate_keystrokes("enter");
    view.read_with(cx, |app, _| {
        assert!(app.overlays.dialog.is_none(), "Enter never asks")
    });
    wait_until(&view, cx, "the removal", |app| {
        app.session.cleaning.is_none()
    });
    cx.executor().advance_clock(motion::STEP);
    cx.run_until_parked();
    assert!(!first.exists());

    let second = select(&view, cx);
    cx.simulate_event(KeyDownEvent {
        keystroke: Keystroke::parse("enter").unwrap(),
        is_held: true,
    });
    view.read_with(cx, |app, _| {
        assert!(
            app.session.cleaning.is_none(),
            "a held Enter deletes nothing"
        );
        assert_eq!(app.selected().len(), 1);
    });
    assert!(second.exists());
}

#[gpui::test]
fn a_failed_reveal_explains_why(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (view, cx) = scanned(cx, dir.path(), Tab::Artifacts);
    right_click(cx, "artifact-row-0");
    click_item(cx, "menu-Reveal in file manager");
    toast_is(&view, cx, |notice| {
        matches!(notice, Notice::Failed(text)
            if text == "Could not open the file manager: no file manager in tests")
    });
}

#[gpui::test]
fn duplicates_can_be_excluded_from_their_menu(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = project(dir.path());
    for name in ["a", "b"] {
        let folder = root.join("photos").join(name);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("same.bin"), vec![b'z'; 16 * 1024]).unwrap();
    }
    let (view, cx) = open(cx, dir.path().join("recent"));
    view.update(cx, |app, _| app.session.options.duplicates = true);
    type_text(cx, &root.display().to_string());
    cx.simulate_keystrokes("enter");
    wait_for_refresh(&view, cx);
    wait_for_duplicates(&view, cx);
    view.update(cx, |app, cx| {
        app.list.tab = Tab::Duplicates;
        cx.notify();
    });
    cx.update(|window, cx| window.draw(cx).clear());
    let groups = |cx: &mut VisualTestContext| {
        view.read_with(cx, |app, _| {
            app.report()
                .unwrap()
                .duplicates
                .as_ref()
                .unwrap()
                .groups
                .len()
        })
    };
    assert_eq!(groups(cx), 1);
    right_click(cx, "duplicate-0-0");
    let Some(Target::Duplicate(path)) = menu_target(&view, cx) else {
        panic!("a duplicate menu");
    };
    view.read_with(cx, |app, _| {
        let items = app.menu_items(&app.overlays.menu.as_ref().unwrap().target);
        let choices: Vec<Choice> = items.iter().map(|item| item.choice).collect();
        assert_eq!(choices, [Choice::Reveal, Choice::CopyPath, Choice::Exclude]);
    });
    cx.simulate_keystrokes("x");
    cx.simulate_keystrokes("tab enter");
    assert_eq!(groups(cx), 0, "one copy left is no longer a duplicate");
    let written = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
    assert!(written.contains(&kuizer::config::exclude_pattern(&path)));
}

#[gpui::test]
fn escape_cancels_a_duplicate_search_and_a_new_one_can_start(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = project(dir.path());
    for name in ["a", "b"] {
        let folder = root.join("photos").join(name);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("same.bin"), vec![b'z'; 16 * 1024]).unwrap();
    }
    let (view, cx) = open(cx, dir.path().join("recent"));
    view.update(cx, |app, _| app.session.options.duplicates = false);
    type_text(cx, &root.display().to_string());
    cx.simulate_keystrokes("enter");
    wait_for_refresh(&view, cx);
    let escape = Keystroke::parse("escape").unwrap();
    let cancel = cx.update(|window, cx| {
        let cancel = view.update(cx, |app, cx| {
            app.run(Command::FindDuplicates, window, cx);
            app.session
                .search
                .as_ref()
                .map(|search| search.cancel.clone())
        });
        window.dispatch_keystroke(escape, cx);
        cancel
    });
    let cancel = cancel.expect("the search started");
    assert!(cancel.is_cancelled());
    let duplicates = |cx: &mut VisualTestContext| {
        view.read_with(cx, |app, _| {
            app.report()
                .unwrap()
                .duplicates
                .as_ref()
                .map(|found| found.groups.len())
        })
    };
    cx.run_until_parked();
    view.read_with(cx, |app, _| {
        assert!(app.session.search.is_none());
        assert!(app.session.search_error.is_none());
    });
    assert_eq!(duplicates(cx), None, "a cancelled search shows nothing");

    cx.simulate_keystrokes("D");
    wait_for_duplicates(&view, cx);
    assert_eq!(duplicates(cx), Some(1));
}

#[gpui::test]
fn a_broken_saved_scan_falls_back_to_a_normal_scan(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = project(dir.path());
    let (view, cx) = open(cx, dir.path().join("recent"));
    type_text(cx, &root.display().to_string());
    cx.simulate_keystrokes("enter");
    wait_for_refresh(&view, cx);
    wait_for_saved(dir.path());
    let scans = dir.path().join("scans");
    let files: Vec<PathBuf> = std::fs::read_dir(&scans)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(files.len(), 1);
    std::fs::write(&files[0], b"\0garbage").unwrap();

    scan_again_from_the_start(&view, cx);
    cx.update(|window, cx| window.draw(cx).clear());
    // A fast runner can stream the first artifacts in before this frame, which leaves the loader.
    if view.read_with(cx, |app, _| matches!(app.session.phase, Phase::Loading(_))) {
        assert!(cx.debug_bounds("activity").is_some());
    } else {
        view.read_with(cx, |app, _| {
            assert!(matches!(app.session.phase, Phase::Ready(_)));
            assert!(app.session.stale.is_none(), "no saved scan is shown");
        });
    }
    assert!(!wait_until_shown(&view, cx), "no saved scan is shown");
    wait_for_refresh(&view, cx);
    let (roots, options) = view.read_with(cx, |app, _| {
        let report = app.report().expect("the fresh scan");
        assert_eq!(report.candidates.len(), 1);
        (report.roots.clone(), report.options.clone())
    });
    let saved = (0..1000).any(|_| {
        let saved = Snapshots::new(Some(scans.clone())).load(&roots, &options);
        saved.is_some() || {
            std::thread::sleep(Duration::from_millis(5));
            false
        }
    });
    assert!(saved, "the fresh scan replaces the broken file");
}

#[gpui::test]
fn the_mode_follows_whatever_takes_the_next_key(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (view, cx) = open(cx, dir.path().join("recent"));
    view.update(cx, |app, _| {
        assert_eq!(app.mode(), keys::Mode::Normal);
        app.list.visual = true;
        app.list.filtering = true;
        assert_eq!(app.mode(), keys::Mode::Filter);
        app.overlays.palette = Some(Palette::default());
        assert_eq!(app.mode(), keys::Mode::Palette);
        app.overlays.dialog = Some(Dialog::Branches {
            scope: GitScope::All,
            cancel: false,
        });
        assert_eq!(app.mode(), keys::Mode::Confirm);
        app.overlays.dialog = None;
        app.overlays.palette = None;
        app.list.filtering = false;
        assert_eq!(app.mode(), keys::Mode::Visual);
    });
}

#[test]
fn a_long_path_gives_way_in_its_folder_part_first() {
    let fit = |room: f32| fit_path("~/apps/kuizer/site/", "node_modules", room, 10.);
    assert_eq!(
        fit(400.),
        ("~/apps/kuizer/site/".to_owned(), "node_modules".to_owned())
    );
    assert_eq!(
        fit(200.),
        ("~/apps/…".to_owned(), "node_modules".to_owned())
    );
    assert_eq!(fit(80.), (String::new(), "node_mo…".to_owned()));
}

#[gpui::test]
fn delete_keys_without_a_selection_remove_nothing(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = project(dir.path());
    let (view, cx) = open(cx, dir.path().join("recent"));
    type_text(cx, &root.display().to_string());
    cx.simulate_keystrokes("enter");
    wait_for_refresh(&view, cx);
    for tab in [Tab::Artifacts, Tab::Caches, Tab::Toolchains, Tab::Docker] {
        view.update(cx, |app, cx| {
            app.list.tab = tab;
            cx.notify();
        });
        for key in ["enter", "delete", "d"] {
            cx.simulate_keystrokes(key);
        }
        cx.run_until_parked();
    }
    assert!(root.join("site").join("node_modules").exists());
}

#[gpui::test]
fn paths_excluded_while_saved_results_load_stay_out(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (view, cx) = open(cx, dir.path().join("recent"));
    let (kept, late) = (
        Path::new("/p/a/node_modules"),
        Path::new("/p/b/node_modules"),
    );
    let member = |path: &Path| dupes::Member {
        path: path.join("file"),
        links: Vec::new(),
        modified: None,
    };
    view.update(cx, |app, cx| {
        app.session.roots = vec![PathBuf::from("/p")];
        app.session.cancel = Some(Cancel::new());
        app.session.pending = Part::expected(&app.session.options);
        app.session.phase = Phase::Loading(Box::default());
        app.session.excluded = vec![PathBuf::from("/p/old")];
        app.session.cached_excluded = app.session.excluded.len();
        app.session.excluded.push(late.to_path_buf());

        let mut saved = Report::empty(&app.session.roots, &app.session.options);
        saved.candidates = vec![artifact(1, kept), artifact(2, late)];
        app.take(vec![Update::Cached(Box::new(saved))], cx);
        let ids: Vec<u64> = app
            .report()
            .unwrap()
            .candidates
            .iter()
            .map(|candidate| candidate.id)
            .collect();
        assert_eq!(
            ids,
            [1],
            "a path excluded after the scan started is dropped"
        );

        let found = Duplicates {
            groups: vec![dupes::Group {
                size: 1,
                allocated: 4096,
                hash: String::new(),
                verified: true,
                members: vec![member(kept), member(late), member(Path::new("/p/c"))],
            }],
            issues: Vec::new(),
            files: 3,
            duration: Duration::ZERO,
        };
        let roots = app.session.roots.clone();
        app.show_duplicates(&roots, found, 1, cx);
        let groups = &app.report().unwrap().duplicates.as_ref().unwrap().groups;
        assert_eq!(groups[0].members.len(), 2);
        assert!(
            groups[0]
                .members
                .iter()
                .all(|member| !member.path.starts_with(late))
        );
    });
}

#[gpui::test]
fn rescan_shows_project_growth_since_previous_scan(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = project(dir.path());
    let (view, cx) = open(cx, dir.path().join("recent"));
    type_text(cx, &root.display().to_string());
    cx.simulate_keystrokes("enter");
    wait_for_scan(&view, cx);
    cx.update(|window, cx| window.draw(cx).clear());

    view.read_with(cx, |app, _| {
        assert!(
            app.session.growth.is_none(),
            "first scan of a folder has no previous scan to compare against"
        );
    });
    assert!(cx.debug_bounds("overview-growth").is_none());

    // Grow the project's artifact and rescan in place.
    std::fs::write(
        root.join("site")
            .join("node_modules")
            .join("left-pad")
            .join("extra.js"),
        vec![b'y'; 128 * 1024],
    )
    .unwrap();
    view.update(cx, |app, cx| app.rescan(cx));
    wait_for_refresh(&view, cx);
    cx.update(|window, cx| window.draw(cx).clear());

    let canonical_site = std::fs::canonicalize(root.join("site")).unwrap();
    view.read_with(cx, |app, _| {
        let (_, diff) = app
            .session
            .growth
            .as_ref()
            .expect("rescan should compute growth against previous scan");
        let grown: Vec<_> = diff.grown_projects().collect();
        assert_eq!(grown.len(), 1);
        assert_eq!(grown[0].project, canonical_site);
        assert!(
            grown[0].delta_bytes >= 64 * 1024,
            "expected substantial growth, got {}",
            grown[0].delta_bytes
        );
    });
    assert!(
        cx.debug_bounds("overview-growth").is_some(),
        "overview should render the Grew since last scan card"
    );
}

#[gpui::test]
fn rescanning_saved_folder_from_start_shows_project_growth(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = project(dir.path());
    let (view, cx) = open(cx, dir.path().join("recent"));
    type_text(cx, &root.display().to_string());
    cx.simulate_keystrokes("enter");
    wait_for_scan(&view, cx);
    wait_for_saved(dir.path());

    std::fs::write(
        root.join("site")
            .join("node_modules")
            .join("left-pad")
            .join("bundle.js"),
        vec![b'z'; 128 * 1024],
    )
    .unwrap();

    scan_again_from_the_start(&view, cx);
    wait_for_scan(&view, cx);
    cx.update(|window, cx| window.draw(cx).clear());

    let canonical_site = std::fs::canonicalize(root.join("site")).unwrap();
    view.read_with(cx, |app, _| {
        let (_, diff) = app
            .session
            .growth
            .as_ref()
            .expect("scanning a folder with a saved snapshot should compute growth");
        let grown: Vec<_> = diff.grown_projects().collect();
        assert_eq!(grown.len(), 1);
        assert_eq!(grown[0].project, canonical_site);
        assert!(grown[0].delta_bytes > 0);
    });
    assert!(cx.debug_bounds("overview-growth").is_some());
}

#[gpui::test]
fn project_view_cleans_only_safe_items_of_that_project(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("workspace");

    // Project alpha has one Safe artifact (node_modules) and one Review artifact (.venv).
    let alpha = root.join("alpha");
    let alpha_modules = alpha.join("node_modules").join("pkg");
    let alpha_venv = alpha.join(".venv");
    std::fs::create_dir_all(&alpha_modules).unwrap();
    std::fs::create_dir_all(&alpha_venv).unwrap();
    std::fs::write(alpha.join("package.json"), "{}").unwrap();
    std::fs::write(alpha_modules.join("index.js"), vec![b'a'; 64 * 1024]).unwrap();
    std::fs::write(alpha_venv.join("pyvenv.cfg"), "home = /usr/bin\n").unwrap();
    std::fs::write(alpha_venv.join("lib.bin"), vec![b'r'; 64 * 1024]).unwrap();

    // Project beta has its own Safe artifact (node_modules) that must remain untouched.
    let beta = root.join("beta");
    let beta_modules = beta.join("node_modules").join("other");
    std::fs::create_dir_all(&beta_modules).unwrap();
    std::fs::write(beta.join("package.json"), "{}").unwrap();
    std::fs::write(beta_modules.join("index.js"), vec![b'b'; 64 * 1024]).unwrap();

    let (view, cx) = open(cx, dir.path().join("recent"));
    type_text(cx, &root.display().to_string());
    cx.simulate_keystrokes("enter");
    wait_for_scan(&view, cx);

    // Switch Overview to the Projects view and draw it.
    view.update(cx, |app, cx| {
        app.list.projects_view = true;
        cx.notify();
    });
    cx.update(|window, cx| window.draw(cx).clear());
    assert!(cx.debug_bounds("projects-view").is_some());
    assert!(cx.debug_bounds("project-card-0").is_some());
    assert!(cx.debug_bounds("project-card-1").is_some());
    assert!(cx.debug_bounds("clean-project-0").is_some());

    // Clean only alpha's safe items.
    let canonical_alpha = std::fs::canonicalize(&alpha).unwrap();
    view.update(cx, |app, cx| {
        app.clean_project_safe(&canonical_alpha, cx);
    });
    for _ in 0..1000 {
        cx.executor().advance_clock(POLL);
        cx.run_until_parked();
        if view.read_with(cx, |app, _| app.session.cleaning.is_none()) {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }

    assert!(
        !alpha.join("node_modules").exists(),
        "alpha's safe artifact should be removed"
    );
    assert!(
        alpha.join(".venv").exists(),
        "alpha's needs-review artifact must NOT be removed by clean safe"
    );
    assert!(
        beta.join("node_modules").exists(),
        "beta's safe artifact must NOT be touched when cleaning alpha"
    );
}
