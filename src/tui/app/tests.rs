use super::super::jobs::GitState;
use crate::delete::Confirmation;
use crate::detect::Safety;
use std::path::PathBuf;
use std::sync::Arc;

use super::*;
use crate::detect::{Category, Kind, RuleInfo};
use crate::scan::Usage;

fn candidate(id: u64, path: &str, bytes: u64, safety: Safety) -> Candidate {
    Candidate {
        id,
        path: PathBuf::from(path),
        kind: Kind::Dir,
        rule: Arc::new(RuleInfo {
            name: "node-modules".to_owned(),
            category: Category::Dependencies,
            safety,
            ecosystem: "node".to_owned(),
        }),
        project: None,
        usage: Usage {
            allocated: bytes,
            ..Usage::default()
        },
        newest: None,
        identity: None,
        project_last_active: None,
    }
}

fn app() -> App {
    app_with(Mode::Permanent, true)
}

fn app_with(mode: Mode, confirm: bool) -> App {
    let mut app = App::new(mode, confirm);
    app.push(candidate(1, "/p/a/node_modules", 10, Safety::Safe));
    app.push(candidate(2, "/p/b/node_modules", 30, Safety::Safe));
    app.push(candidate(3, "/p/c/dist", 20, Safety::Review));
    app.refresh();
    app
}

#[test]
fn sorts_by_size_and_keeps_cursor_on_item() {
    let mut app = app();
    let order: Vec<u64> = app
        .view
        .iter()
        .map(|&i| app.items[i].candidate.id)
        .collect();
    assert_eq!(order, [2, 3, 1]);
    app.move_by(2);
    app.push(candidate(4, "/p/d/node_modules", 100, Safety::Safe));
    app.refresh();
    assert_eq!(app.current().unwrap().candidate.id, 1);
}

#[test]
fn space_selects_multiple_and_moves_down() {
    let mut app = app();
    app.toggle();
    app.toggle();
    let ids: Vec<u64> = app.selected().map(|item| item.candidate.id).collect();
    assert_eq!(ids, [2, 3]);
    assert_eq!(app.selected_bytes(), 50);
}

#[test]
fn shift_motions_select_from_the_anchor_to_the_cursor() {
    let mut app = app();
    app.push(candidate(4, "/p/d/node_modules", 5, Safety::Safe));
    app.refresh();
    app.move_by(1);
    app.toggle();
    app.extend(-1);
    app.extend(-1);
    let ids = |app: &App| -> Vec<u64> {
        app.view
            .iter()
            .map(|&index| &app.items[index])
            .filter(|item| item.selected)
            .map(|item| item.candidate.id)
            .collect()
    };
    assert_eq!(ids(&app), [2, 3, 1]);
    app.extend(1);
    app.extend(1);
    assert_eq!(app.cursor, 2);
    assert_eq!(
        ids(&app),
        [3, 1],
        "moving back keeps only what was selected before"
    );
    app.end_range();
    app.extend(1);
    assert_eq!(ids(&app), [3, 1, 4]);
}

#[test]
fn enter_opens_confirm_only_with_a_selection() {
    let mut app = app();
    assert!(app.delete_selected().is_empty());
    assert!(matches!(app.overlay, Overlay::None));
    app.toggle();
    assert!(app.delete_selected().is_empty());
    assert!(matches!(&app.overlay, Overlay::Confirm { ids, .. } if ids == &[2]));
}

#[test]
fn deleting_skips_the_dialog_when_confirmation_is_off() {
    let mut app = app_with(Mode::Permanent, false);
    app.toggle();
    let jobs = app.delete_selected();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].1, Mode::Permanent);
    assert!(matches!(app.overlay, Overlay::None));
    assert_eq!(app.item(2).unwrap().status, Status::Deleting);
}

#[test]
fn review_items_always_ask() {
    let mut app = app_with(Mode::Permanent, false);
    app.move_to(1);
    assert!(app.delete().is_empty());
    assert!(matches!(&app.overlay, Overlay::Confirm { ids, .. } if ids == &[3]));
    assert_eq!(app.item(3).unwrap().status, Status::Ready);
}

#[test]
fn projects_with_uncommitted_changes_always_ask() {
    let mut app = app_with(Mode::Permanent, false);
    app.changes.insert(PathBuf::from("/p/a"), 2);
    app.select_safe();
    assert!(app.delete_selected().is_empty());
    assert!(matches!(&app.overlay, Overlay::Confirm { ids, .. } if ids.len() == 2));
    assert_eq!(app.confirm().len(), 2);
}

#[test]
fn a_project_still_being_checked_for_changes_asks() {
    let mut app = app_with(Mode::Permanent, false);
    app.expect_status(PathBuf::from("/p/b"));
    app.toggle();
    assert!(app.delete_selected().is_empty());
    assert_eq!(
        app.confirm_stats.as_ref().map(|stats| stats.checking),
        Some(1)
    );
    app.set_statuses([(PathBuf::from("/p/b"), GitState::Changes(0))]);
    assert_eq!(
        app.confirm_stats.as_ref().map(|stats| stats.checking),
        Some(0)
    );
    app.overlay = Overlay::None;
    assert_eq!(app.delete_selected().len(), 1);
}

#[test]
fn a_project_whose_git_check_failed_asks() {
    let mut app = app_with(Mode::Permanent, false);
    app.expect_status(PathBuf::from("/p/b"));
    app.set_statuses([(PathBuf::from("/p/b"), GitState::Unknown)]);
    app.toggle();
    assert!(app.delete_selected().is_empty());
    assert_eq!(app.pending_confirmation(), Confirmation::YesNo);
    assert_eq!(
        app.confirm_stats.as_ref().map(|stats| stats.unknown),
        Some(true)
    );
}

#[test]
fn running_counters_match_a_recount() {
    let mut app = app();
    app.push(candidate(4, "/p/d/node_modules", 5, Safety::Safe));
    app.refresh();
    let counters = |app: &App| {
        (
            app.selected_count(),
            app.selected_bytes(),
            app.deleting(),
            app.total_bytes(),
        )
    };
    let recount = |app: &App| {
        let bytes = |item: &Item| item.candidate.usage.allocated;
        (
            app.selected().count(),
            app.selected().map(bytes).sum::<u64>(),
            app.items
                .iter()
                .filter(|item| item.status == Status::Deleting)
                .count(),
            app.items
                .iter()
                .filter(|item| item.status != Status::Deleted)
                .map(bytes)
                .sum::<u64>(),
        )
    };
    app.select_safe();
    assert_eq!(counters(&app), recount(&app));
    app.move_to(0);
    app.extend(1);
    app.extend(1);
    app.extend(-1);
    app.toggle();
    assert_eq!(counters(&app), recount(&app));
    app.open_confirm();
    app.confirm();
    assert_eq!(counters(&app), recount(&app));
    app.finish(2, Mode::Permanent, Ok(()));
    app.finish(1, Mode::Permanent, Err("refused".to_owned()));
    assert_eq!(counters(&app), recount(&app));
    app.select_project();
    app.toggle();
    assert_eq!(counters(&app), recount(&app));
    app.clear_selection();
    assert_eq!(counters(&app), recount(&app));
}

#[test]
fn select_safe_skips_review_items() {
    let mut app = app();
    app.select_safe();
    let mut ids: Vec<u64> = app.selected().map(|item| item.candidate.id).collect();
    ids.sort_unstable();
    assert_eq!(ids, [1, 2]);
}

#[test]
fn delete_without_selection_targets_cursor() {
    let mut app = app();
    app.open_confirm();
    assert_eq!(
        app.overlay,
        Overlay::Confirm {
            ids: vec![2],
            mode: Mode::Permanent
        }
    );
    let jobs = app.confirm();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].1, Mode::Permanent);
    assert_eq!(app.item(2).unwrap().status, Status::Deleting);
    app.finish(2, Mode::Permanent, Ok(()));
    assert_eq!(app.freed, 30);
    assert_eq!(app.item(2).unwrap().status, Status::Deleted);
}

#[test]
fn trashed_items_do_not_count_as_freed_space() {
    let mut app = app_with(Mode::Trash, true);
    app.free = Some(100);
    assert_eq!(app.free_after(10, Mode::Permanent), Some(110));
    assert_eq!(app.free_after(10, Mode::Trash), Some(100));
    app.open_confirm();
    let jobs = app.confirm();
    assert_eq!(jobs[0].1, Mode::Trash);
    app.finish(2, Mode::Trash, Ok(()));
    assert_eq!(app.trashed, 30);
    assert_eq!(app.freed, 0);
}

#[test]
fn project_grouping_orders_by_project_and_selects_whole_project() {
    let mut app = App::new(Mode::Permanent, true);
    let mut cache = candidate(4, "/p/a/.cache", 50, Safety::Review);
    cache.project = Some(PathBuf::from("/p/a"));
    let mut modules = candidate(1, "/p/a/node_modules", 10, Safety::Safe);
    modules.project = Some(PathBuf::from("/p/a"));
    app.push(modules);
    app.push(candidate(2, "/p/b/node_modules", 30, Safety::Safe));
    app.push(cache);
    app.toggle_grouping();
    app.refresh();
    let order: Vec<u64> = app
        .view
        .iter()
        .map(|&index| app.items[index].candidate.id)
        .collect();
    assert_eq!(order, [4, 1, 2]);

    app.move_to(1);
    let selected =
        |app: &App| -> Vec<u64> { app.selected().map(|item| item.candidate.id).collect() };
    app.select_project();
    assert_eq!(selected(&app), [1]);
    app.select_project();
    assert_eq!(selected(&app), [1, 4]);
    assert_eq!(app.selected_bytes(), 60);
    assert_eq!(app.selected_review(), 1);
    assert!(app.message.clone().unwrap_or_default().contains("widened"));
    app.select_project();
    assert!(selected(&app).is_empty());
    assert_eq!(app.selected_review(), 0);
}

#[test]
fn a_review_only_project_warns_before_selecting() {
    let mut app = app();
    app.move_to(1);
    app.select_project();
    assert_eq!(app.selected_count(), 0);
    assert!(app.message.clone().unwrap_or_default().contains("review"));
    app.disarm_widen();
    app.select_project();
    assert_eq!(app.selected_count(), 0);
    app.select_project();
    assert_eq!(app.selected_review(), 1);
}

#[test]
fn reports_only_nonzero_uncommitted_changes() {
    let mut app = app();
    app.changes.insert(PathBuf::from("/p/a"), 3);
    app.changes.insert(PathBuf::from("/p/b"), 0);
    assert_eq!(app.changes_in(&app.items[0].candidate), Some(3));
    assert_eq!(app.changes_in(&app.items[1].candidate), None);
}

#[test]
fn ordering_matches_a_full_sort_after_every_batch_for_every_sort_and_grouping() {
    let rules: Vec<Arc<RuleInfo>> = (0..4)
        .map(|index| {
            Arc::new(RuleInfo {
                name: format!("rule-{}", index % 3),
                category: Category::ALL[index % Category::ALL.len()],
                safety: if index == 0 {
                    Safety::Review
                } else {
                    Safety::Safe
                },
                ecosystem: "x".to_owned(),
            })
        })
        .collect();
    let build = |id: u64| {
        let mut made = candidate(
            id,
            &format!("/p/{}/{id}/out", id * 7 % 13),
            id * 37 % 11 * 100,
            Safety::Safe,
        );
        made.rule = Arc::clone(&rules[id as usize % rules.len()]);
        made.project = Some(PathBuf::from(format!("/p/proj{}", id % 9)));
        made
    };
    let expected = |app: &App| -> Vec<u64> {
        let key = |candidate: &Candidate| match app.grouping {
            Grouping::Rule => (Some(candidate.rule.category), candidate.rule.name.clone()),
            Grouping::Project => (None, project_of(candidate).to_string_lossy().into_owned()),
        };
        let shown: Vec<&Candidate> = app
            .items
            .iter()
            .map(|item| &item.candidate)
            .filter(|candidate| !app.safe_only || candidate.rule.safety == Safety::Safe)
            .collect();
        let mut bytes: HashMap<_, u64> = HashMap::new();
        for candidate in &shown {
            *bytes.entry(key(candidate)).or_default() += candidate.usage.allocated;
        }
        let mut ordered = shown.clone();
        ordered.sort_by(|a, b| {
            let (left, right) = (key(a), key(b));
            let group = match app.sort {
                SortKey::Size | SortKey::Idle => {
                    bytes[&right].cmp(&bytes[&left]).then(left.cmp(&right))
                }
                SortKey::Path | SortKey::Category => left.cmp(&right),
            };
            group.then_with(|| match app.sort {
                SortKey::Path => a.path.cmp(&b.path),
                _ => b
                    .usage
                    .allocated
                    .cmp(&a.usage.allocated)
                    .then_with(|| a.path.cmp(&b.path)),
            })
        });
        ordered.iter().map(|candidate| candidate.id).collect()
    };
    let mut app = App::new(Mode::Permanent, true);
    for batch in [1..41u64, 41..61, 61..64] {
        for id in batch {
            app.push(build(id));
        }
        for safe_only in [false, true] {
            app.safe_only = safe_only;
            for sort in [
                SortKey::Size,
                SortKey::Path,
                SortKey::Category,
                SortKey::Size,
            ] {
                for grouping in [Grouping::Rule, Grouping::Project] {
                    app.sort = sort;
                    app.grouping = grouping;
                    app.dirty = true;
                    app.refresh();
                    let order: Vec<u64> = app
                        .view
                        .iter()
                        .map(|&index| app.items[index].candidate.id)
                        .collect();
                    assert_eq!(
                        order,
                        expected(&app),
                        "{sort:?} {grouping:?} safe={safe_only}"
                    );
                    let positions: Vec<Option<usize>> =
                        order.iter().map(|&id| app.position(id)).collect();
                    assert_eq!(positions, (0..order.len()).map(Some).collect::<Vec<_>>());
                }
            }
        }
    }
}

#[test]
fn deleted_items_cannot_be_selected_again() {
    let mut app = app();
    app.open_confirm();
    app.confirm();
    app.finish(2, Mode::Permanent, Err("refused".to_owned()));
    assert_eq!(app.freed, 0);
    app.move_to(0);
    app.toggle();
    assert_eq!(app.selected().count(), 0);
    app.move_to(0);
    app.open_confirm();
    assert_eq!(app.overlay, Overlay::None);
}

#[test]
fn category_and_safe_filters_narrow_view() {
    let mut app = app();
    app.toggle_safe_only();
    app.refresh();
    assert_eq!(app.view.len(), 2);
    app.cycle_category();
    app.refresh();
    assert_eq!(app.view.len(), 2);
    app.cycle_category();
    app.refresh();
    assert_eq!(app.view.len(), 0);
    app.reset_filters();
    app.refresh();
    assert_eq!(app.view.len(), 3);
}

#[test]
fn filter_narrows_view() {
    let mut app = app();
    app.set_filter("dist".to_owned());
    app.refresh();
    assert_eq!(app.view.len(), 1);
    app.select_safe();
    assert_eq!(app.selected().count(), 0);
}

#[test]
fn duplicate_rows_follow_the_search_and_keep_the_cursor_in_range() {
    let group = |paths: [&str; 2]| crate::dupes::Group {
        size: 10,
        allocated: 10,
        hash: String::new(),
        verified: false,
        members: paths
            .iter()
            .map(|path| crate::dupes::Member {
                path: PathBuf::from(path),
                links: Vec::new(),
                modified: None,
            })
            .collect(),
    };
    let mut app = app();
    app.switch(Tab::Duplicates);
    app.set_duplicates(Duplicates {
        groups: vec![group(["/a/x", "/a/y"]), group(["/b/x", "/b/Photo"])],
        ..Duplicates::default()
    });
    app.move_to(usize::MAX);
    assert_eq!(app.duplicates.cursor, 5);
    app.set_filter("photo".to_owned());
    app.refresh();
    assert_eq!(
        app.duplicates.rows,
        [Spot::Group(1), Spot::Member(1, 0), Spot::Member(1, 1)]
    );
    assert_eq!(app.duplicates.cursor, 2);
}
