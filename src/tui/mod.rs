mod app;
mod jobs;
mod palette;
mod view;
mod watch;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crossbeam_channel::{Sender, TryRecvError, unbounded};
use ratatui::crossterm::event::{
    self, Event as TermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
};

use crate::config::Config;
use crate::delete::{Mode, Target};
use crate::scan::{self, Cancel, Event};
use app::{App, Overlay, project_of};
use jobs::{Changes, Job, Outcome};
pub use watch::watch;

const FRAME: Duration = Duration::from_millis(50);
const IDLE: Duration = Duration::from_millis(250);
const REFRESH: Duration = Duration::from_millis(100);
const EVENTS_PER_FRAME: usize = 5_000;
const PAGE: isize = 20;

fn dispatch(app: &mut App, jobs: &Sender<Job>, batch: Vec<(u64, Target, Mode)>) {
    for (id, target, mode) in batch {
        let Some(item) = app.item(id) else {
            continue;
        };
        let job = Job {
            id,
            target,
            mode,
            candidate: item.candidate.clone(),
        };
        if jobs.send(job).is_err() {
            app.restore_deleting();
            return;
        }
    }
}

fn notify(app: &mut App, problem: String) {
    app.message = Some(match app.message.take() {
        Some(existing) => format!("{existing}; {problem}"),
        None => problem,
    });
}

fn handle_quit_prompt(app: &mut App, key: KeyEvent) {
    if key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
    {
        return;
    }
    let held_enter = key.code == KeyCode::Enter && app.enter_repeats(Instant::now());
    match key.code {
        KeyCode::Char('y') => app.quit = true,
        KeyCode::Enter if !held_enter => app.quit = true,
        KeyCode::Enter | KeyCode::Char('q') => {}
        _ => app.overlay = Overlay::None,
    }
}

fn handle_filter(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.filtering = false;
            app.set_filter(String::new());
        }
        KeyCode::Enter => app.filtering = false,
        KeyCode::Backspace => {
            let mut filter = app.filter.clone();
            filter.pop();
            app.set_filter(filter);
        }
        KeyCode::Char(c) => {
            let mut filter = app.filter.clone();
            filter.push(c);
            app.set_filter(filter);
        }
        _ => {}
    }
}

fn handle_chord(app: &mut App, key: KeyEvent) {
    app.message = None;
    app.end_range();
    let plain_control =
        key.modifiers.contains(KeyModifiers::CONTROL) && !key.modifiers.contains(KeyModifiers::ALT);
    if app.filtering || app.overlay != Overlay::None || !plain_control {
        return;
    }
    match key.code {
        KeyCode::Char('d') => app.move_by(PAGE / 2),
        KeyCode::Char('u') => app.move_by(-PAGE / 2),
        _ => {}
    }
}

fn handle_key(app: &mut App, key: KeyEvent, jobs: &Sender<Job>) {
    if app.overlay == Overlay::Quit {
        handle_quit_prompt(app, key);
        return;
    }
    let chord = key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
    if chord || key.code != KeyCode::Char('P') {
        app.disarm_widen();
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c' | 'q'))
    {
        app.request_quit();
        return;
    }
    if chord {
        handle_chord(app, key);
        return;
    }
    if key.code == KeyCode::Char('P')
        && !app.filtering
        && app.overlay == Overlay::None
        && app.project_key_repeats(Instant::now())
    {
        return;
    }
    app.message = None;
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let extending = matches!(key.code, KeyCode::Char('J' | 'K'))
        || (shift && matches!(key.code, KeyCode::Up | KeyCode::Down));
    if !extending || app.filtering {
        app.end_range();
    }
    if app.filtering {
        handle_filter(app, key);
        return;
    }
    let held_enter = key.code == KeyCode::Enter && app.enter_repeats(Instant::now());
    match (&app.overlay, key.code) {
        (Overlay::Confirm { .. }, KeyCode::Enter) if held_enter => {}
        (Overlay::Confirm { .. }, KeyCode::Char('y') | KeyCode::Enter) => {
            let batch = app.confirm();
            dispatch(app, jobs, batch);
        }
        (Overlay::Confirm { .. }, _) => app.overlay = Overlay::None,
        (Overlay::Help, _) => app.overlay = Overlay::None,
        (Overlay::Quit, _) => {}
        (Overlay::None, code) => match code {
            KeyCode::Char('J') => app.extend(1),
            KeyCode::Char('K') => app.extend(-1),
            KeyCode::Down if shift => app.extend(1),
            KeyCode::Up if shift => app.extend(-1),
            KeyCode::Char('q') => app.request_quit(),
            KeyCode::Esc => app.back_out(),
            KeyCode::Char('j') | KeyCode::Down => app.move_by(1),
            KeyCode::Char('k') | KeyCode::Up => app.move_by(-1),
            KeyCode::PageDown => app.move_by(PAGE),
            KeyCode::PageUp => app.move_by(-PAGE),
            KeyCode::Char('g') | KeyCode::Home => app.move_to(0),
            KeyCode::Char('G') | KeyCode::End => app.move_to(usize::MAX),
            KeyCode::Char(' ') => app.toggle(),
            KeyCode::Char('a') => app.select_safe(),
            KeyCode::Char('u') => app.clear_selection(),
            KeyCode::Char('d') | KeyCode::Delete => {
                let batch = app.delete();
                dispatch(app, jobs, batch);
            }
            KeyCode::Enter => {
                let batch = app.delete_selected();
                dispatch(app, jobs, batch);
            }
            KeyCode::Char('s') => app.cycle_sort(),
            KeyCode::Char('p') => app.toggle_grouping(),
            KeyCode::Char('P') => app.select_project(),
            KeyCode::Char('/') => app.filtering = true,
            KeyCode::Char('c') => app.cycle_category(),
            KeyCode::Char('f') => app.toggle_safe_only(),
            KeyCode::Char('x') => app.reset_filters(),
            KeyCode::Char('?') => app.overlay = Overlay::Help,
            _ => {}
        },
    }
}

/// @name run
/// @description Runs the interactive interface: streams scan results into a selectable list
/// and deletes confirmed selections on a background thread through the delete guard.
///
/// @example
/// tui::run(&config, &roots, false)?;
pub fn run(config: &Config, roots: &[PathBuf], confirm: bool) -> std::io::Result<()> {
    run_with_notice(config, roots, confirm, None)
}

/// @name run_with_notice
/// @description Like `run`, and shows `notice` in the footer until the first key press, for
/// problems found before the interface opened such as ignored config settings.
///
/// @example
/// tui::run_with_notice(&config, &roots, false, Some("config: bad key".to_owned()))?;
pub fn run_with_notice(
    config: &Config,
    roots: &[PathBuf],
    confirm: bool,
    notice: Option<String>,
) -> std::io::Result<()> {
    let mut app = App::new(config.delete.mode.into(), confirm);
    app.message = notice;
    let mut terminal = ratatui::try_init()?;
    let started = terminal
        .draw(|frame| view::draw(frame, &app))
        .map(drop)
        .and_then(|()| scan::start(config, roots, Cancel::new()).map_err(std::io::Error::other));
    let handle = match started {
        Ok(handle) => handle,
        Err(error) => {
            ratatui::restore();
            return Err(error);
        }
    };
    let (job_tx, job_rx) = unbounded::<Job>();
    let (result_tx, result_rx) = unbounded::<Outcome>();
    let stop = Arc::new(AtomicBool::new(false));
    let deleter = match jobs::spawn_deleters(roots.to_vec(), job_rx, result_tx, Arc::clone(&stop)) {
        Ok(deleter) => Some(deleter),
        Err(error) => {
            notify(
                &mut app,
                format!("could not start the delete worker: {error}"),
            );
            None
        }
    };
    let (project_tx, project_rx) = unbounded::<PathBuf>();
    let (changes_tx, changes_rx) = unbounded::<Changes>();
    if let Err(error) = jobs::spawn_status(project_rx, changes_tx) {
        app.set_changes_unknown();
        notify(&mut app, format!("could not start the git check: {error}"));
    }
    let (nudge_tx, nudge_rx) = unbounded::<()>();
    let (free_tx, free_rx) = unbounded::<Option<u64>>();
    if let Err(error) = jobs::spawn_disk(roots.to_vec(), nudge_rx, free_tx) {
        notify(&mut app, format!("could not start the disk check: {error}"));
    }

    let outcome = (|| -> std::io::Result<()> {
        let mut scanning = true;
        let mut redraw = true;
        let mut refreshed: Option<Instant> = None;
        while !app.quit {
            for _ in 0..EVENTS_PER_FRAME {
                if !scanning {
                    break;
                }
                match handle.events.try_recv() {
                    Ok(Event::Candidate(candidate)) => {
                        let project = project_of(&candidate);
                        if !app.knows_project(project) {
                            let project = project.to_path_buf();
                            app.expect_status(project.clone());
                            let _ = project_tx.send(project);
                        }
                        app.push(candidate);
                    }
                    Ok(Event::Issue(_)) => app.issues += 1,
                    Ok(Event::Finished(summary)) => {
                        app.finished = Some(summary);
                        scanning = false;
                        redraw = true;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => scanning = false,
                }
            }
            let updates: Vec<_> = changes_rx.try_iter().collect();
            if !updates.is_empty() {
                app.set_statuses(updates);
                redraw = true;
            }
            let mut removed = false;
            loop {
                match result_rx.try_recv() {
                    Ok((id, mode, result)) => {
                        app.finish(id, mode, result);
                        removed = true;
                    }
                    Err(TryRecvError::Disconnected) if app.deleting() > 0 => {
                        app.restore_deleting();
                        redraw = true;
                        break;
                    }
                    Err(_) => break,
                }
            }
            if removed {
                let _ = nudge_tx.send(());
                redraw = true;
            }
            while let Ok(free) = free_rx.try_recv() {
                redraw |= app.free != free;
                app.free = free;
            }
            if scanning {
                app.progress = handle.progress();
                app.current = handle.current();
                redraw = true;
            }
            let due = !scanning || refreshed.is_none_or(|at| at.elapsed() >= REFRESH);
            if app.changed() || (app.arrivals() && due) {
                app.refresh();
                refreshed = Some(Instant::now());
                redraw = true;
            }
            if redraw {
                terminal.draw(|frame| view::draw(frame, &app))?;
                redraw = false;
            }
            let busy = scanning || app.deleting() > 0 || app.awaiting_status() > 0;
            if !event::poll(if busy { FRAME } else { IDLE })? {
                continue;
            }
            loop {
                if let TermEvent::Key(key) = event::read()?
                    && key.kind == KeyEventKind::Press
                {
                    if app.changed() && !app.filtering {
                        app.refresh();
                    }
                    handle_key(&mut app, key, &job_tx);
                }
                redraw = true;
                let confirming = matches!(app.overlay, Overlay::Confirm { .. } | Overlay::Quit);
                if app.quit || confirming || !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }
        Ok(())
    })();
    ratatui::restore();

    stop.store(true, Ordering::Relaxed);
    drop(job_tx);
    drop(project_tx);
    drop(nudge_tx);
    drop(handle);
    if let Some(deleter) = deleter
        && let Ok(Some(error)) = deleter.join()
    {
        eprintln!("kuizer: {error}");
    }
    outcome
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::detect::{Category, Kind, RuleInfo, Safety};
    use crate::scan::{Candidate, Usage};
    use app::Status;

    fn candidate(id: u64, path: &str) -> Candidate {
        Candidate {
            id,
            path: PathBuf::from(path),
            kind: Kind::Dir,
            rule: Arc::new(RuleInfo {
                name: "node-modules".to_owned(),
                category: Category::Dependencies,
                safety: Safety::Safe,
                ecosystem: "node".to_owned(),
            }),
            project: None,
            usage: Usage {
                allocated: 10 * id,
                ..Usage::default()
            },
            newest: None,
            identity: None,
            project_last_active: None,
        }
    }

    fn app() -> App {
        let mut app = App::new(Mode::Permanent, false);
        for id in 1..=30 {
            app.push(candidate(id, &format!("/p/{id}/node_modules")));
        }
        app.refresh();
        app
    }

    fn press(app: &mut App, jobs: &Sender<Job>, code: KeyCode, modifiers: KeyModifiers) {
        handle_key(app, KeyEvent::new(code, modifiers), jobs);
    }

    #[test]
    fn holding_enter_never_confirms_a_deletion() {
        let (jobs, queued) = unbounded::<Job>();
        let mut app = app();
        press(&mut app, &jobs, KeyCode::Char(' '), KeyModifiers::NONE);
        app.confirm_before_delete = true;
        press(&mut app, &jobs, KeyCode::Enter, KeyModifiers::NONE);
        assert!(matches!(app.overlay, Overlay::Confirm { .. }));
        press(&mut app, &jobs, KeyCode::Enter, KeyModifiers::NONE);
        assert!(matches!(app.overlay, Overlay::Confirm { .. }));
        assert!(queued.try_recv().is_err());
    }

    #[test]
    fn esc_backs_out_one_level_and_never_quits() {
        let (jobs, _queued) = unbounded::<Job>();
        let mut app = app();
        press(&mut app, &jobs, KeyCode::Char(' '), KeyModifiers::NONE);
        app.set_filter("node".to_owned());
        press(&mut app, &jobs, KeyCode::Esc, KeyModifiers::NONE);
        assert!(!app.filtered());
        assert_eq!(app.selected_count(), 1);
        press(&mut app, &jobs, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.selected_count(), 0);
        press(&mut app, &jobs, KeyCode::Esc, KeyModifiers::NONE);
        assert!(!app.quit);
    }

    #[test]
    fn quitting_during_a_deletion_asks_and_a_held_key_never_confirms() {
        let (jobs, _queued) = unbounded::<Job>();
        let mut app = app();
        press(&mut app, &jobs, KeyCode::Char('d'), KeyModifiers::NONE);
        assert_eq!(app.deleting(), 1);
        press(&mut app, &jobs, KeyCode::Enter, KeyModifiers::NONE);
        press(&mut app, &jobs, KeyCode::Char('q'), KeyModifiers::NONE);
        assert_eq!(app.overlay, Overlay::Quit);
        press(&mut app, &jobs, KeyCode::Enter, KeyModifiers::NONE);
        press(&mut app, &jobs, KeyCode::Char('q'), KeyModifiers::NONE);
        press(&mut app, &jobs, KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(app.overlay, Overlay::Quit);
        assert!(!app.quit);
        press(&mut app, &jobs, KeyCode::Char('n'), KeyModifiers::NONE);
        assert_eq!(app.overlay, Overlay::None);
        assert!(!app.quit);
        press(&mut app, &jobs, KeyCode::Char('q'), KeyModifiers::NONE);
        press(&mut app, &jobs, KeyCode::Char('y'), KeyModifiers::NONE);
        assert!(app.quit);
    }

    #[test]
    fn holding_p_never_widens_to_review_items() {
        let (jobs, _queued) = unbounded::<Job>();
        let mut app = App::new(Mode::Permanent, false);
        app.push(candidate(1, "/p/x/node_modules"));
        let mut review = candidate(2, "/p/x/dist");
        review.rule = Arc::new(RuleInfo {
            name: "dist".to_owned(),
            category: Category::Build,
            safety: Safety::Review,
            ecosystem: "node".to_owned(),
        });
        app.push(review);
        app.refresh();
        press(&mut app, &jobs, KeyCode::Char('P'), KeyModifiers::SHIFT);
        press(&mut app, &jobs, KeyCode::Char('P'), KeyModifiers::SHIFT);
        assert_eq!(app.selected_count(), 1);
        assert_eq!(app.selected_review(), 0);
    }

    #[test]
    fn a_dead_delete_worker_keeps_the_queue_selected() {
        let (jobs, queued) = unbounded::<Job>();
        drop(queued);
        let mut app = app();
        press(&mut app, &jobs, KeyCode::Char(' '), KeyModifiers::NONE);
        press(&mut app, &jobs, KeyCode::Char(' '), KeyModifiers::NONE);
        press(&mut app, &jobs, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.deleting(), 0);
        assert_eq!(app.selected_count(), 2);
        assert!(app.items.iter().all(|item| item.status == Status::Ready));
        assert!(app.message.is_some());
    }

    #[test]
    fn control_and_alt_chords_never_run_the_plain_key_actions() {
        let (jobs, queued) = unbounded::<Job>();
        let mut app = app();
        press(&mut app, &jobs, KeyCode::Char('d'), KeyModifiers::CONTROL);
        assert_eq!(app.cursor, (PAGE / 2) as usize);
        press(&mut app, &jobs, KeyCode::Char('u'), KeyModifiers::CONTROL);
        assert_eq!(app.cursor, 0);
        for (code, modifiers) in [
            (KeyCode::Char('d'), KeyModifiers::ALT),
            (
                KeyCode::Char('d'),
                KeyModifiers::CONTROL | KeyModifiers::ALT,
            ),
            (KeyCode::Char('a'), KeyModifiers::CONTROL),
            (KeyCode::Char('y'), KeyModifiers::CONTROL),
            (KeyCode::Char('s'), KeyModifiers::ALT),
        ] {
            press(&mut app, &jobs, code, modifiers);
        }
        assert!(queued.is_empty());
        assert_eq!(app.selected_count(), 0);
        assert!(app.items.iter().all(|item| item.status == Status::Ready));
        assert_eq!(app.overlay, Overlay::None);
        assert_eq!(app.cursor, 0);
        assert!(!app.quit);

        press(&mut app, &jobs, KeyCode::Char('d'), KeyModifiers::NONE);
        assert_eq!(queued.len(), 1);
    }

    #[test]
    fn chords_do_not_confirm_a_dialog_or_type_into_the_filter() {
        let (jobs, queued) = unbounded::<Job>();
        let mut app = app();
        app.open_confirm();
        press(&mut app, &jobs, KeyCode::Char('y'), KeyModifiers::CONTROL);
        press(&mut app, &jobs, KeyCode::Char('y'), KeyModifiers::ALT);
        assert!(matches!(app.overlay, Overlay::Confirm { .. }));
        assert!(queued.is_empty());
        app.overlay = Overlay::None;
        app.filtering = true;
        press(&mut app, &jobs, KeyCode::Char('d'), KeyModifiers::CONTROL);
        press(&mut app, &jobs, KeyCode::Char('D'), KeyModifiers::SHIFT);
        assert_eq!(app.filter, "D");
    }
}
