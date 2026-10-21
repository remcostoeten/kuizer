mod app;
mod jobs;
mod view;
mod watch;

use std::collections::HashSet;
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

fn dispatch(app: &App, jobs: &Sender<Job>, batch: Vec<(u64, Target, Mode)>) {
    for (id, target, mode) in batch {
        let Some(item) = app.item(id) else {
            continue;
        };
        let _ = jobs.send(Job {
            id,
            target,
            mode,
            candidate: item.candidate.clone(),
        });
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

fn handle_key(app: &mut App, key: KeyEvent, jobs: &Sender<Job>) {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.quit = true;
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
    match (&app.overlay, key.code) {
        (Overlay::Confirm { .. }, KeyCode::Char('y') | KeyCode::Enter) => {
            let batch = app.confirm();
            dispatch(app, jobs, batch);
        }
        (Overlay::Confirm { .. }, KeyCode::Char('t')) => app.toggle_confirm_mode(),
        (Overlay::Confirm { .. }, _) => app.overlay = Overlay::None,
        (Overlay::Help, _) => app.overlay = Overlay::None,
        (Overlay::None, code) => match code {
            KeyCode::Char('J') => app.extend(1),
            KeyCode::Char('K') => app.extend(-1),
            KeyCode::Down if shift => app.extend(1),
            KeyCode::Up if shift => app.extend(-1),
            KeyCode::Char('q') | KeyCode::Esc => app.quit = true,
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
    let mut app = App::new(config.delete.mode.into(), confirm);
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
    let deleter = jobs::spawn_deleters(roots.to_vec(), job_rx, result_tx, Arc::clone(&stop));
    let (project_tx, project_rx) = unbounded::<PathBuf>();
    let (changes_tx, changes_rx) = unbounded::<Changes>();
    jobs::spawn_status(project_rx, changes_tx);
    let (nudge_tx, nudge_rx) = unbounded::<()>();
    let (free_tx, free_rx) = unbounded::<Option<u64>>();
    jobs::spawn_disk(roots.to_vec(), nudge_rx, free_tx);
    let mut projects = HashSet::new();

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
                        if projects.insert(project.to_path_buf()) {
                            app.expect_status(project.to_path_buf());
                            let _ = project_tx.send(project.to_path_buf());
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
            while let Ok((project, count)) = changes_rx.try_recv() {
                app.set_status(project, count);
                redraw = true;
            }
            let mut removed = false;
            while let Ok((id, mode, result)) = result_rx.try_recv() {
                app.finish(id, mode, result);
                removed = true;
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
                if app.quit || !event::poll(Duration::ZERO)? {
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
    if let Ok(Some(error)) = deleter.join() {
        eprintln!("kuizer: {error}");
    }
    outcome
}
