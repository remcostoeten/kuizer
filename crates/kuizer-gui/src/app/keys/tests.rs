use gpui::{Entity, TestAppContext, VisualTestContext, size};

use super::*;
use crate::app::tests::{
    artifact, menu_target, open, project, scanned, type_text, wait_for_duplicates, wait_for_refresh,
};

#[test]
fn motions_stop_at_the_ends() {
    assert_eq!(step(Motion::Down, 0, 3, 10), 1);
    assert_eq!(step(Motion::Down, 2, 3, 10), 2);
    assert_eq!(step(Motion::Up, 0, 3, 10), 0);
    assert_eq!(step(Motion::Last, 0, 3, 10), 2);
    assert_eq!(step(Motion::First, 2, 3, 10), 0);
    assert_eq!(step(Motion::PageDown, 1, 30, 10), 11);
    assert_eq!(step(Motion::PageDown, 25, 30, 10), 29);
    assert_eq!(step(Motion::PageUp, 5, 30, 10), 0);
}

#[test]
fn filters_ignore_case_and_match_any_field() {
    assert!(found(None, &["anything"]));
    assert!(found(Some("node"), &["/x/Node_Modules"]));
    assert!(found(Some("cargo"), &["/x/target", "Cargo build output"]));
    assert!(!found(Some("pip"), &["/x/target", "cargo"]));
}

#[gpui::test]
fn the_palette_asks_the_marked_folders_not_the_disk(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (view, cx) = open(cx, dir.path().join("recent"));
    view.update(cx, |app, _| {
        let nowhere = dir.path().join("not-created");
        assert_eq!(
            app.blocked(&Command::Scan(nowhere)),
            None,
            "the interface thread never touches the disk for it"
        );
        let there = dir.path().to_path_buf();
        app.start.recent.mark_gone(vec![there.clone()]);
        assert_eq!(
            app.blocked(&Command::Scan(there)),
            Some("This folder is missing")
        );
    });
}

#[gpui::test]
fn a_list_is_filtered_again_only_when_its_filter_or_the_report_changes(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (view, cx) = open(cx, dir.path().join("recent"));
    view.update(cx, |app, cx| {
        app.session.roots = vec![PathBuf::from("/p")];
        let mut report = Report::empty(&app.session.roots, &app.session.options);
        report.caches = [("npm", "/c/npm"), ("pnpm", "/c/pnpm")]
            .into_iter()
            .zip(1..)
            .map(|((name, path), id)| {
                let cache = kuizer::caches::KnownCache {
                    name: name.to_owned(),
                    tool: name.to_owned(),
                    group: kuizer::catalog::Group::Developer,
                    category: kuizer::detect::Category::Cache,
                    kind: kuizer::detect::Kind::Dir,
                    path: PathBuf::from(path),
                    safety: Safety::Safe,
                    note: String::new(),
                    processes: Vec::new(),
                };
                (cache, artifact(id, Path::new(path)))
            })
            .collect();
        app.session.phase = Phase::Ready(Box::new(report));
        app.refresh_stats();
        assert_eq!(app.counts(Tab::Caches), (2, 2));
        assert!(
            Rc::ptr_eq(
                &app.listed(Tab::Caches).targets,
                &app.listed(Tab::Caches).targets
            ),
            "asking again reuses the rows"
        );

        app.list.filters.insert(Tab::Caches, "PNPM".to_owned());
        assert_eq!(app.counts(Tab::Caches), (1, 2));
        assert_eq!(app.targets(Tab::Caches).len(), 1);

        app.list.filters.remove(&Tab::Caches);
        app.list.safety = Some(Safety::Review);
        assert_eq!(
            app.counts(Tab::Caches),
            (0, 2),
            "the safety segments narrow the list too"
        );
        app.list.safety = None;
        assert_eq!(app.counts(Tab::Caches), (2, 2));

        app.take(vec![Update::Loaded(Loaded::Caches(Vec::new()))], cx);
        assert_eq!(
            app.counts(Tab::Caches),
            (0, 0),
            "a new report is listed anew"
        );
    });
}

/// A folder with `count` projects, each with its own node_modules folder.
fn projects(dir: &Path, count: usize) -> PathBuf {
    let root = dir.join("work");
    for index in 0..count {
        let project = root.join(format!("project-{index:02}"));
        let modules = project.join("node_modules").join("dep");
        std::fs::create_dir_all(&modules).unwrap();
        std::fs::write(project.join("package.json"), "{}").unwrap();
        std::fs::write(modules.join("index.js"), vec![b'x'; 4096 + index]).unwrap();
    }
    root
}

fn scan_root<'a>(
    cx: &'a mut TestAppContext,
    dir: &Path,
    root: &Path,
    tab: Tab,
) -> (Entity<Kuizer>, &'a mut VisualTestContext) {
    cx.update(bind_keys);
    let (view, cx) = open(cx, dir.join("recent"));
    view.update(cx, |app, _| {
        app.session.options.duplicates = tab == Tab::Duplicates
    });
    type_text(cx, &root.display().to_string());
    cx.simulate_keystrokes("enter");
    wait_for_refresh(&view, cx);
    wait_for_duplicates(&view, cx);
    view.update(cx, |app, cx| app.go_to(tab, cx));
    cx.update(|window, cx| window.draw(cx).clear());
    (view, cx)
}

fn cursor_row(view: &Entity<Kuizer>, cx: &mut VisualTestContext) -> usize {
    view.read_with(cx, |app, _| {
        let cursor = app.cursor_target().expect("a cursor row");
        app.targets(app.list.tab)
            .iter()
            .position(|target| *target == cursor)
            .unwrap()
    })
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear());
}

/// `debug_bounds` takes a `&'static str`; a test can afford to leak a few selectors.
fn painted(cx: &mut VisualTestContext, selector: String) -> bool {
    cx.debug_bounds(Box::leak(selector.into_boxed_str()))
        .is_some()
}

fn inside(cx: &mut VisualTestContext, row: &'static str, list: &'static str) {
    let row = cx.debug_bounds(row).expect("the row is painted");
    let list = cx.debug_bounds(list).expect("the list is painted");
    assert!(
        row.top() >= list.top() - px(1.) && row.bottom() <= list.bottom() + px(1.),
        "{row:?} is outside {list:?}"
    );
}

#[gpui::test]
fn ctrl_k_opens_the_palette_and_enter_runs_a_command(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    cx.update(bind_keys);
    let (view, cx) = scanned(cx, dir.path(), Tab::Overview);
    cx.simulate_keystrokes("ctrl-k");
    view.read_with(cx, |app, _| assert!(app.overlays.palette.is_some()));
    draw(cx);
    let panel = cx.debug_bounds("palette").expect("the palette is painted");
    let window = cx.update(|window, _| window.viewport_size());
    assert!(
        (panel.center().x - window.width / 2.).abs() < px(1.),
        "centered"
    );
    assert!(panel.top() > px(0.) && panel.bottom() < window.height);
    let rescan = cx.debug_bounds("palette-Rescan").expect("a command row");
    assert!(panel.contains(&rescan.center()));
    let everything = view.read_with(cx, |app, _| app.palette_entries().len());

    type_text(cx, "go hist");
    view.read_with(cx, |app, _| {
        let entries = app.palette_entries();
        assert!(entries.len() < everything, "typing narrows the list");
        assert_eq!(entries[0].spec.command, Command::GoTo(Tab::History));
        assert_eq!(app.overlays.palette.as_ref().unwrap().query, "go hist");
    });
    cx.simulate_keystrokes("enter");
    view.read_with(cx, |app, _| {
        assert!(
            app.overlays.palette.is_none(),
            "running a command closes the palette"
        );
        assert_eq!(app.list.tab, Tab::History);
    });

    cx.simulate_keystrokes("ctrl-k");
    type_text(cx, "zoom");
    cx.simulate_keystrokes("down ctrl-n ctrl-p");
    view.read_with(cx, |app, _| {
        let palette = app.overlays.palette.as_ref().unwrap();
        assert_eq!(palette.active, 1);
        assert_eq!(
            app.palette_entries()[palette.active].spec.command,
            Command::ZoomOut
        );
    });
    cx.simulate_keystrokes("enter");
    view.read_with(cx, |app, _| assert_eq!(app.config.appearance.zoom, 90));
}

#[gpui::test]
fn a_blocked_command_is_grey_and_does_nothing(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    cx.update(bind_keys);
    let (view, cx) = scanned(cx, dir.path(), Tab::Artifacts);
    view.update(cx, |app, cx| {
        let id = app.report().unwrap().candidates[0].id;
        app.toggle(Source::Artifact, id, cx);
        app.session.stale = Some(SystemTime::now());
    });
    cx.simulate_keystrokes("ctrl-k");
    type_text(cx, "delete selected");
    view.read_with(cx, |app, _| {
        let entry = &app.palette_entries()[0];
        assert_eq!(entry.spec.command, Command::Trash);
        assert_eq!(entry.blocked, app.trash_lock());
        assert!(entry.blocked.is_some());
    });
    draw(cx);
    assert!(cx.debug_bounds("palette-Delete selected").is_some());
    cx.simulate_keystrokes("enter");
    view.read_with(cx, |app, _| {
        assert!(
            app.session.cleaning.is_none(),
            "a blocked command does not run"
        );
        assert!(app.overlays.palette.is_some(), "the palette stays open");
    });
    view.update(cx, |app, _| app.session.stale = None);
    view.read_with(cx, |app, _| {
        assert_eq!(
            app.palette_entries()[0].blocked,
            None,
            "the same query is checked again once the state changes"
        );
    });
}

#[gpui::test]
fn disabled_route_shortcuts_leave_the_section_alone(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    cx.update(bind_keys);
    let (view, cx) = scanned(cx, dir.path(), Tab::Artifacts);
    view.update(cx, |app, _| app.config.route_shortcuts.enabled = false);
    cx.simulate_keystrokes("shift-3");
    view.read_with(cx, |app, _| assert_eq!(app.list.tab, Tab::Artifacts));
    view.update(cx, |app, _| app.config.route_shortcuts.enabled = true);
    cx.simulate_keystrokes("shift-3");
    view.read_with(cx, |app, _| assert_eq!(app.list.tab, Tab::Caches));
}

#[gpui::test]
fn a_switch_in_the_settings_popover_is_written_to_the_config(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    cx.update(bind_keys);
    let (view, cx) = scanned(cx, dir.path(), Tab::Artifacts);
    let file = dir.path().join("config.toml");
    view.update(cx, |app, _| app.config_path = Some(file.clone()));
    cx.simulate_keystrokes("ctrl-,");
    view.read_with(cx, |app, _| assert!(app.overlays.settings.is_some()));
    draw(cx);
    assert!(cx.debug_bounds("settings").is_some());
    cx.simulate_keystrokes("tab space");
    view.read_with(cx, |app, _| {
        assert_eq!(app.config.delete.mode, kuizer::config::DeleteMode::Trash);
    });
    cx.run_until_parked();
    let written = std::fs::read_to_string(&file).unwrap();
    assert!(written.contains("mode = \"trash\""), "{written}");
    cx.simulate_keystrokes("ctrl-,");
    view.read_with(cx, |app, _| {
        assert!(app.overlays.settings.is_none(), "it toggles")
    });
    cx.simulate_keystrokes("ctrl-, escape");
    view.read_with(cx, |app, _| assert!(app.overlays.settings.is_none()));
}

#[gpui::test]
fn the_theme_row_picks_a_theme_and_writes_it_to_the_config(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    cx.update(bind_keys);
    let (view, cx) = scanned(cx, dir.path(), Tab::Artifacts);
    let file = dir.path().join("config.toml");
    view.update(cx, |app, _| app.config_path = Some(file.clone()));
    cx.simulate_keystrokes("ctrl-, right");
    view.read_with(cx, |app, _| {
        assert_eq!(app.config.appearance.theme, kuizer::config::Theme::Light);
    });
    cx.run_until_parked();
    let written = std::fs::read_to_string(&file).unwrap();
    assert!(written.contains("theme = \"light\""), "{written}");
    draw(cx);
    assert!(cx.debug_bounds("theme-card-Dark").is_some());
    cx.simulate_keystrokes("left");
    view.read_with(cx, |app, _| {
        assert_eq!(app.config.appearance.theme, kuizer::config::Theme::Dark);
    });
}

#[gpui::test]
fn escape_closes_only_the_palette(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    cx.update(bind_keys);
    let (view, cx) = scanned(cx, dir.path(), Tab::Artifacts);
    view.update(cx, |app, cx| {
        app.show_toast(Notice::Info("hello".to_owned()), cx)
    });
    cx.simulate_keystrokes("ctrl-k");
    type_text(cx, "re");
    cx.simulate_keystrokes("escape");
    view.read_with(cx, |app, _| {
        assert!(app.overlays.palette.is_none());
        assert!(
            app.overlays.toast.is_some(),
            "Escape went to the palette only"
        );
    });
    cx.simulate_keystrokes("ctrl-k");
    view.read_with(cx, |app, _| {
        assert_eq!(
            app.overlays.palette.as_ref().unwrap().query,
            "",
            "it opens empty"
        );
    });
    cx.simulate_keystrokes("ctrl-k");
    view.read_with(cx, |app, _| {
        assert!(app.overlays.palette.is_none(), "Ctrl+K toggles")
    });
    cx.simulate_keystrokes("escape");
    view.read_with(cx, |app, _| assert!(app.overlays.toast.is_none()));
}

#[gpui::test]
fn recent_folders_are_in_the_palette(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    cx.update(bind_keys);
    let first = projects(&dir.path().join("a"), 1);
    let second = projects(&dir.path().join("b"), 2);
    let (view, cx) = scan_root(cx, dir.path(), &first, Tab::Artifacts);
    view.update(cx, |app, _| {
        app.start
            .recent
            .remember(&[std::fs::canonicalize(&second).unwrap()]);
    });
    cx.simulate_keystrokes("ctrl-k");
    type_text(cx, &format!("scan {}", second.display()));
    view.read_with(cx, |app, _| {
        assert!(matches!(
            &app.palette_entries()[0].spec.command,
            Command::Scan(path) if path.ends_with("b/work")
        ));
    });
    cx.simulate_keystrokes("enter");
    wait_for_refresh(&view, cx);
    view.read_with(cx, |app, _| {
        assert!(app.session.roots[0].ends_with("b/work"));
        assert_eq!(app.report().unwrap().candidates.len(), 2);
    });
}

#[gpui::test]
fn shift_motions_select_from_the_anchor_to_the_cursor(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = projects(dir.path(), 6);
    let (view, cx) = scan_root(cx, dir.path(), &root, Tab::Artifacts);
    let selected = |view: &Entity<Kuizer>, cx: &mut VisualTestContext| {
        view.read_with(cx, |app, _| {
            let report = app.report().unwrap();
            app.list
                .rows
                .iter()
                .enumerate()
                .filter(|(_, index)| {
                    app.list
                        .selection
                        .contains(&(Source::Artifact, report.candidates[**index].id))
                })
                .map(|(row, _)| row)
                .collect::<Vec<_>>()
        })
    };
    cx.simulate_keystrokes("j j space");
    assert_eq!(
        cursor_row(&view, cx),
        3,
        "Space moves on to the next unselected row"
    );
    cx.simulate_keystrokes("K K K");
    assert_eq!(selected(&view, cx), vec![0, 1, 2, 3]);
    cx.simulate_keystrokes("shift-down shift-down");
    assert_eq!(cursor_row(&view, cx), 2);
    assert_eq!(
        selected(&view, cx),
        vec![2, 3],
        "moving back keeps only what was selected before"
    );
    cx.simulate_keystrokes("j j J shift-down");
    assert_eq!(selected(&view, cx), vec![2, 3, 4, 5]);
}

#[gpui::test]
fn keys_do_not_fire_while_typing(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = projects(dir.path(), 3);
    let (view, cx) = scan_root(cx, dir.path(), &root, Tab::Artifacts);
    cx.simulate_keystrokes("ctrl-k");
    type_text(cx, "2j ");
    view.read_with(cx, |app, _| {
        assert_eq!(app.list.tab, Tab::Artifacts);
        assert_eq!(app.overlays.palette.as_ref().unwrap().query, "2j ");
        assert!(app.list.selection.is_empty());
    });
    cx.simulate_keystrokes("escape");

    cx.simulate_keystrokes("/");
    view.read_with(cx, |app, _| assert!(app.list.filtering));
    type_text(cx, "1");
    cx.simulate_keystrokes("j k space ? ctrl-a");
    view.read_with(cx, |app, _| {
        assert_eq!(app.list.tab, Tab::Artifacts);
        assert_eq!(app.list.filters.get(&Tab::Artifacts).unwrap(), "1jk ?");
        assert!(app.list.selection.is_empty());
        assert!(!app.overlays.shortcuts);
        assert!(app.list.cursors.is_empty(), "the cursor never moved");
    });
}

/// The rows of the artifact list that are fully scrolled into view.
fn visible_rows(view: &Entity<Kuizer>, cx: &mut VisualTestContext) -> (usize, usize) {
    view.read_with(cx, |app, _| {
        let state = app.list.artifact_scroll.0.borrow();
        let top = -state.base_handle.offset().y;
        let height = state.base_handle.bounds().size.height;
        let content = state.base_handle.max_offset().height + height;
        #[allow(clippy::cast_precision_loss, reason = "a handful of rows")]
        let row = content / app.list.rows.len() as f32;
        let first = (top / row).ceil();
        let last = ((top + height) / row).floor() - 1.;
        (first as usize, last as usize)
    })
}

#[gpui::test]
fn j_and_k_move_the_cursor_and_the_list_follows(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = projects(dir.path(), 40);
    let (view, cx) = scan_root(cx, dir.path(), &root, Tab::Artifacts);
    cx.simulate_resize(size(px(900.), px(520.)));
    draw(cx);
    assert_eq!(cursor_row(&view, cx), 0);
    cx.simulate_keystrokes("j j down");
    assert_eq!(cursor_row(&view, cx), 3);
    cx.simulate_keystrokes("k");
    assert_eq!(cursor_row(&view, cx), 2);
    draw(cx);
    assert!(
        cx.debug_bounds("cursor").is_some(),
        "the cursor row is marked"
    );

    let (first, last) = visible_rows(&view, cx);
    assert_eq!(first, 0);
    assert!(last < 39, "the list is longer than the window");
    cx.simulate_keystrokes("shift-g");
    assert_eq!(cursor_row(&view, cx), 39);
    draw(cx);
    inside(cx, "artifact-row-39", "artifact-list");
    assert_eq!(visible_rows(&view, cx).1, 39);
    for _ in 0..30 {
        cx.simulate_keystrokes("k");
    }
    assert_eq!(cursor_row(&view, cx), 9);
    draw(cx);
    let (first, last) = visible_rows(&view, cx);
    assert!(
        first <= 9 && 9 <= last,
        "row 9 is in view: {first}..={last}"
    );
    assert!(first > 0, "the list only scrolled as far as needed");
    cx.simulate_keystrokes("g");
    assert_eq!(cursor_row(&view, cx), 0);
    cx.simulate_keystrokes("pagedown");
    let page = cursor_row(&view, cx);
    assert!(page > 3 && page < 20, "a page is what fits, here {page}");
    cx.simulate_keystrokes("pageup end home");
    assert_eq!(cursor_row(&view, cx), 0);
}

#[gpui::test]
fn a_long_duplicate_list_scrolls_to_the_cursor(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("photos");
    for group in 0..30 {
        for copy in ["a", "b"] {
            let folder = root.join(copy);
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(
                folder.join(format!("{group}.bin")),
                vec![u8::try_from(group).unwrap(); 8192 + group],
            )
            .unwrap();
        }
    }
    let (view, cx) = scan_root(cx, dir.path(), &root, Tab::Duplicates);
    cx.simulate_resize(size(px(900.), px(520.)));
    draw(cx);
    let last = view.read_with(cx, |app, _| app.targets(Tab::Duplicates).len() - 1);
    assert_eq!(last, 59);
    cx.simulate_keystrokes("end");
    assert_eq!(cursor_row(&view, cx), last);
    draw(cx);
    draw(cx);
    inside(cx, "duplicate-29-1", "duplicate-list");
    cx.simulate_keystrokes("home");
    draw(cx);
    draw(cx);
    inside(cx, "duplicate-0-0", "duplicate-list");
}

#[gpui::test]
fn a_long_cache_list_scrolls_to_the_cursor(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    cx.update(bind_keys);
    let (view, cx) = open(cx, dir.path().join("recent"));
    view.update(cx, |app, cx| {
        app.session.roots = vec![PathBuf::from("/p")];
        let mut report = Report::empty(&app.session.roots, &app.session.options);
        report.caches = (1..=60u64)
            .map(|id| {
                let path = PathBuf::from(format!("/c/cache-{id}"));
                let cache = kuizer::caches::KnownCache {
                    name: "cache".to_owned(),
                    tool: "tool".to_owned(),
                    group: kuizer::catalog::Group::Developer,
                    category: kuizer::detect::Category::Cache,
                    kind: kuizer::detect::Kind::Dir,
                    path: path.clone(),
                    safety: Safety::Safe,
                    note: String::new(),
                    processes: Vec::new(),
                };
                (cache, artifact(id, &path))
            })
            .collect();
        app.session.phase = Phase::Ready(Box::new(report));
        app.refresh_stats();
        app.go_to(Tab::Caches, cx);
    });
    cx.simulate_resize(size(px(900.), px(520.)));
    draw(cx);
    cx.simulate_keystrokes("end");
    assert_eq!(cursor_row(&view, cx), 59);
    draw(cx);
    draw(cx);
    inside(cx, "cache-row-59", "cache-list");
    cx.simulate_keystrokes("home");
    draw(cx);
    draw(cx);
    inside(cx, "cache-row-0", "cache-list");
}

fn enter(is_held: bool) -> KeyDownEvent {
    KeyDownEvent {
        keystroke: Keystroke::parse("enter").unwrap(),
        is_held,
    }
}

#[test]
fn a_key_that_was_not_released_is_a_repeat() {
    let mut held = Held::default();
    let now = Instant::now();
    assert!(held.press(&enter(true), now), "the platform said so");
    held.release("enter");
    assert!(!held.press(&enter(false), now));
    assert!(held.press(&enter(false), now), "X11 sends no release");
    held.release("enter");
    assert!(!held.press(&enter(false), now));
    assert!(!held.press(&enter(false), now + HELD_FOR), "a lost release");
}

#[gpui::test]
fn shift_click_selects_a_range_and_ctrl_click_takes_one_out(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = projects(dir.path(), 4);
    let (view, cx) = scan_root(cx, dir.path(), &root, Tab::Artifacts);
    let mut click = |row: &'static str, modifiers: gpui::Modifiers| {
        draw(cx);
        let bounds = cx.debug_bounds(row).expect("the row is painted");
        cx.simulate_click(bounds.center(), modifiers);
        view.read_with(cx, |app, _| app.list.selection.len())
    };
    assert_eq!(click("artifact-row-0", gpui::Modifiers::default()), 1);
    assert_eq!(click("artifact-row-2", gpui::Modifiers::shift()), 3);
    assert_eq!(click("artifact-row-1", gpui::Modifiers::control()), 2);
    assert_eq!(click("artifact-row-3", gpui::Modifiers::shift()), 4);
}

#[gpui::test]
fn a_count_repeats_a_motion_and_visual_mode_selects_along_it(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = projects(dir.path(), 6);
    let (view, cx) = scan_root(cx, dir.path(), &root, Tab::Artifacts);
    let selected = |view: &Entity<Kuizer>, cx: &mut VisualTestContext| {
        view.read_with(cx, |app, _| app.list.selection.len())
    };
    cx.simulate_keystrokes("3 j");
    assert_eq!(cursor_row(&view, cx), 3);
    assert_eq!(selected(&view, cx), 0, "a plain motion selects nothing");
    cx.simulate_keystrokes("2 G");
    assert_eq!(cursor_row(&view, cx), 1, "a count before G is a row number");

    cx.simulate_keystrokes("v 3 j");
    assert_eq!(cursor_row(&view, cx), 4);
    assert_eq!(selected(&view, cx), 4, "the start row and three more");
    cx.simulate_keystrokes("k");
    assert_eq!(selected(&view, cx), 3, "moving back gives a row up again");
    cx.simulate_keystrokes("escape j");
    view.read_with(cx, |app, _| assert!(!app.list.visual));
    assert_eq!(selected(&view, cx), 3, "the selection outlives the mode");

    cx.simulate_keystrokes("9 9 K");
    assert_eq!(
        cursor_row(&view, cx),
        0,
        "a count stops at the end of the list"
    );
}

#[gpui::test]
fn space_toggles_the_cursor_item(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = projects(dir.path(), 2);
    let (view, cx) = scan_root(cx, dir.path(), &root, Tab::Artifacts);
    cx.simulate_keystrokes("j space");
    view.read_with(cx, |app, _| {
        let Some(Target::Item(source, id, _)) = app.cursor_target() else {
            panic!("an item");
        };
        assert_eq!(app.list.selection, HashSet::from([(source, id)]));
    });
    cx.simulate_keystrokes("space");
    view.read_with(cx, |app, _| assert!(app.list.selection.is_empty()));
}

#[gpui::test]
fn the_filter_narrows_rows_and_select_all_follows_it(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = projects(dir.path(), 12);
    let (view, cx) = scan_root(cx, dir.path(), &root, Tab::Artifacts);
    assert_eq!(
        view.read_with(cx, |app, _| app.counts(Tab::Artifacts)),
        (12, 12)
    );
    cx.simulate_keystrokes("/");
    type_text(cx, "PROJECT-1");
    view.read_with(cx, |app, _| {
        assert_eq!(
            app.counts(Tab::Artifacts),
            (2, 12),
            "project-10 and project-11"
        );
        assert!(app.list.filtering);
    });
    draw(cx);
    assert!(cx.debug_bounds("tally").is_some());
    cx.simulate_keystrokes("enter");
    view.read_with(cx, |app, _| {
        assert!(!app.list.filtering, "Enter keeps the filter")
    });
    cx.simulate_keystrokes("ctrl-a");
    view.read_with(cx, |app, _| {
        assert_eq!(app.list.selection.len(), 2, "only the visible rows");
        assert!(
            app.selected()
                .iter()
                .all(|(_, candidate)| { candidate.path.to_string_lossy().contains("project-1") })
        );
    });
    cx.simulate_keystrokes("ctrl-shift-a");
    view.read_with(cx, |app, _| assert!(app.list.selection.is_empty()));

    cx.simulate_keystrokes("/");
    cx.simulate_keystrokes("escape");
    view.read_with(cx, |app, _| {
        assert!(!app.list.filtering, "Escape leaves the field");
        assert!(
            !app.list.filters.contains_key(&Tab::Artifacts),
            "and clears it"
        );
        assert_eq!(app.counts(Tab::Artifacts), (12, 12));
    });

    cx.simulate_keystrokes("/");
    type_text(cx, "nothing-like-this");
    cx.simulate_keystrokes("enter escape");
    view.read_with(cx, |app, _| {
        assert_eq!(
            app.list.rows.len(),
            12,
            "Escape outside the field clears it too"
        );
    });
}

#[gpui::test]
fn the_cursor_keeps_its_item_while_the_filter_changes(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = projects(dir.path(), 12);
    let (view, cx) = scan_root(cx, dir.path(), &root, Tab::Artifacts);
    let cursor_id = |view: &Entity<Kuizer>, cx: &mut VisualTestContext| {
        view.read_with(cx, |app, _| match app.cursor_target() {
            Some(Target::Item(_, id, _)) => id,
            other => panic!("expected an item cursor, got {other:?}"),
        })
    };
    cx.simulate_keystrokes("/");
    type_text(cx, "project-1");
    cx.simulate_keystrokes("enter j");
    assert_eq!(cursor_row(&view, cx), 1);
    let id = cursor_id(&view, cx);

    cx.simulate_keystrokes("escape");
    let row = cursor_row(&view, cx);
    assert_eq!(cursor_id(&view, cx), id, "the cursor stays on its item");
    cx.simulate_keystrokes("j");
    assert_eq!(cursor_row(&view, cx), (row + 1).min(11));

    cx.simulate_keystrokes("/");
    type_text(cx, "project-1");
    cx.simulate_keystrokes("backspace");
    view.read_with(cx, |app, _| {
        assert_eq!(
            app.counts(Tab::Artifacts),
            (12, 12),
            "a shorter filter widens again"
        );
    });
}

#[gpui::test]
fn the_menu_key_opens_on_the_cursor_row(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = projects(dir.path(), 3);
    let (view, cx) = scan_root(cx, dir.path(), &root, Tab::Artifacts);
    cx.simulate_keystrokes("j j");
    let cursor = view.read_with(cx, |app, _| app.cursor_target());
    draw(cx);
    cx.simulate_keystrokes("menu");
    assert_eq!(menu_target(&view, cx), cursor);
    draw(cx);
    let row = cx.debug_bounds("artifact-row-2").unwrap();
    let menu = cx.debug_bounds("menu").unwrap();
    assert!(
        menu.top() >= row.bottom() - px(1.),
        "it opens below the row"
    );
    cx.simulate_keystrokes("escape shift-f10");
    assert_eq!(menu_target(&view, cx), cursor);
}

#[gpui::test]
fn number_keys_and_ctrl_tab_switch_sections(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = project(dir.path());
    let (view, cx) = scan_root(cx, dir.path(), &root, Tab::Overview);
    let tab = |cx: &mut VisualTestContext| view.read_with(cx, |app, _| app.list.tab);
    cx.simulate_keystrokes("shift-3");
    assert_eq!(tab(cx), Tab::Caches);
    cx.simulate_keystrokes("ctrl-tab");
    assert_eq!(tab(cx), Tab::Toolchains);
    cx.simulate_keystrokes("ctrl-shift-tab ctrl-shift-tab ctrl-shift-tab");
    assert_eq!(tab(cx), Tab::Overview);
    cx.simulate_keystrokes("ctrl-shift-tab");
    assert_eq!(tab(cx), Tab::Issues, "it wraps around");
    cx.simulate_keystrokes("shift-1");
    assert_eq!(tab(cx), Tab::Overview);
}

#[gpui::test]
fn the_shortcuts_overlay_lists_the_command_table(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = project(dir.path());
    let (view, cx) = scan_root(cx, dir.path(), &root, Tab::Artifacts);
    cx.simulate_keystrokes("?");
    view.read_with(cx, |app, _| assert!(app.overlays.shortcuts));
    draw(cx);
    assert!(cx.debug_bounds("shortcuts").is_some());
    for spec in commands::table() {
        assert!(
            painted(cx, format!("shortcut-{}", spec.label)),
            "{} is listed",
            spec.label
        );
    }
    for (label, _, _) in commands::HINTS.iter().chain(commands::CONTROLS) {
        assert!(
            painted(cx, format!("shortcut-{label}")),
            "{label} is listed"
        );
    }
    cx.simulate_keystrokes("j");
    view.read_with(cx, |app, _| {
        assert!(app.overlays.shortcuts, "a motion keeps it open")
    });
    cx.simulate_keystrokes("escape");
    view.read_with(cx, |app, _| assert!(!app.overlays.shortcuts));

    cx.simulate_keystrokes("ctrl-k");
    type_text(cx, "keyboard");
    cx.simulate_keystrokes("enter");
    view.read_with(cx, |app, _| {
        assert!(app.overlays.shortcuts, "the palette opens it too");
        assert!(app.overlays.palette.is_none());
    });
}

#[gpui::test]
fn the_shortcuts_overlay_scrolls_from_the_keyboard(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = project(dir.path());
    let (view, cx) = scan_root(cx, dir.path(), &root, Tab::Artifacts);
    cx.simulate_resize(size(px(860.), px(520.)));
    cx.simulate_keystrokes("?");
    draw(cx);
    let read = |view: &Entity<Kuizer>, cx: &mut VisualTestContext| {
        view.read_with(cx, |app, _| {
            (
                -app.overlays.shortcuts_scroll.offset().y,
                app.overlays.shortcuts_scroll.max_offset().height,
            )
        })
    };
    let (top, end) = read(&view, cx);
    assert_eq!(top, px(0.));
    assert!(end > px(0.), "the overlay is taller than the window");
    cx.simulate_keystrokes("j");
    assert!(read(&view, cx).0 > px(0.));
    cx.simulate_keystrokes("shift-g");
    assert_eq!(read(&view, cx).0, end, "G reaches the last group");
    cx.simulate_keystrokes("ctrl-u");
    assert!(read(&view, cx).0 < end);
    cx.simulate_keystrokes("g");
    assert_eq!(read(&view, cx).0, px(0.));
    view.read_with(cx, |app, _| assert!(app.overlays.shortcuts));
}

fn graded_list(view: &Entity<Kuizer>, cx: &mut VisualTestContext, grades: &[Safety]) {
    view.update(cx, |app, cx| {
        app.session.roots = vec![PathBuf::from("/p")];
        let mut report = Report::empty(&app.session.roots, &app.session.options);
        report.candidates = grades
            .iter()
            .zip(1u64..)
            .map(|(&safety, id)| {
                let mut candidate = artifact(id, &PathBuf::from(format!("/p/item-{id}")));
                let rule = kuizer::detect::RuleInfo {
                    safety,
                    ..(*candidate.rule).clone()
                };
                candidate.rule = std::sync::Arc::new(rule);
                candidate
            })
            .collect();
        app.list.selection.clear();
        app.session.phase = Phase::Ready(Box::new(report));
        app.refresh_stats();
        app.refresh_rows();
        app.go_to(Tab::Artifacts, cx);
    });
    draw(cx);
}

#[gpui::test]
fn select_all_widens_to_review_items_one_press_at_a_time(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (view, cx) = open(cx, dir.path().join("recent"));
    let counts = |view: &Entity<Kuizer>, cx: &mut VisualTestContext| {
        view.read_with(cx, |app, _| {
            let total = app.list.selection.stats().total();
            (total.safe.count, total.review.count)
        })
    };
    let asking = |view: &Entity<Kuizer>, cx: &mut VisualTestContext| {
        view.read_with(cx, |app, _| app.asking())
    };

    graded_list(&view, cx, &[Safety::Safe, Safety::Review, Safety::Safe]);
    cx.simulate_keystrokes("ctrl-a");
    assert_eq!(counts(&view, cx), (2, 0), "safe items come first");
    let held = KeyDownEvent {
        keystroke: Keystroke::parse("ctrl-a").unwrap(),
        is_held: true,
    };
    cx.update(|window, cx| view.update(cx, |app, cx| app.on_key(&held, window, cx)));
    assert_eq!(counts(&view, cx), (2, 0), "a held key never widens");
    cx.simulate_keystrokes("ctrl-a");
    assert_eq!(counts(&view, cx), (2, 1), "the next press takes the rest");
    draw(cx);
    assert!(cx.debug_bounds("mode-review").is_some());
    cx.simulate_keystrokes("ctrl-a");
    assert_eq!(counts(&view, cx), (0, 0), "a full list clears");

    graded_list(&view, cx, &[Safety::Review, Safety::Review]);
    cx.simulate_keystrokes("ctrl-a");
    assert_eq!(counts(&view, cx), (0, 0), "review items are never silent");
    assert_eq!(asking(&view, cx), Some(2));
    draw(cx);
    assert!(cx.debug_bounds("mode-asking").is_some());
    cx.simulate_keystrokes("j ctrl-a");
    assert_eq!(counts(&view, cx), (0, 0), "another key asks again");
    assert_eq!(asking(&view, cx), Some(2));
    cx.simulate_keystrokes("ctrl-a");
    assert_eq!(counts(&view, cx), (0, 2));
    assert_eq!(asking(&view, cx), None);
    cx.simulate_keystrokes("ctrl-a");
    assert_eq!(counts(&view, cx), (0, 0));

    draw(cx);
    let checkbox = cx.debug_bounds("select-visible").expect("the checkbox");
    cx.simulate_click(checkbox.center(), gpui::Modifiers::default());
    assert_eq!(counts(&view, cx), (0, 0), "the checkbox asks first too");
    assert_eq!(asking(&view, cx), Some(2));
}

#[gpui::test]
fn vim_motions_move_the_branch_dialog_focus_and_never_confirm(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (view, cx) = scanned(cx, dir.path(), Tab::Repos);
    view.update(cx, |app, cx| {
        app.overlays.dialog = Some(Dialog::Branches {
            scope: GitScope::All,
            cancel: true,
        });
        cx.notify();
    });
    for (key, cancel) in [("h", false), ("j", true), ("k", false), ("l", true)] {
        cx.simulate_keystrokes(key);
        let focus = view.read_with(cx, |app, _| match app.overlays.dialog {
            Some(Dialog::Branches { cancel, .. }) => Some(cancel),
            _ => None,
        });
        assert_eq!(focus, Some(cancel), "{key} moves focus, confirms nothing");
    }
}
