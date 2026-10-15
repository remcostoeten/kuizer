mod app;
mod view;
mod watch;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, TryRecvError, unbounded};
use ratatui::crossterm::event::{
    self, Event as TermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
};

use crate::config::Config;
use crate::delete::{self, Guard, Mode, Target};
use crate::git;
use crate::history::{self, Entry};
use crate::platform;
use crate::scan::{self, Cancel, Event};
use app::{App, Overlay, project_of};
pub use watch::watch;

const FRAME: Duration = Duration::from_millis(50);
const EVENTS_PER_FRAME: usize = 5_000;
const PAGE: isize = 20;
const DISK_REFRESH: Duration = Duration::from_secs(2);

type Job = (u64, Target, Mode);
type Outcome = (u64, Mode, Result<(), String>);
type Changes = (PathBuf, Option<usize>);

fn spawn_deleter(
    guard: Guard,
    jobs: Receiver<Job>,
    results: Sender<Outcome>,
    stop: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("kuizer-delete".to_owned())
        .spawn(move || {
            for (id, target, mode) in jobs {
                let result = if stop.load(Ordering::Relaxed) {
                    Err("cancelled".to_owned())
                } else {
                    delete::delete(&guard, &target, mode).map_err(|error| error.to_string())
                };
                if results.send((id, mode, result)).is_err() {
                    break;
                }
            }
        })
        .expect("spawn delete thread")
}

fn spawn_git(projects: Receiver<PathBuf>, results: Sender<Changes>) {
    thread::Builder::new()
        .name("kuizer-git".to_owned())
        .spawn(move || {
            for project in projects {
                let count = git::uncommitted_changes(&project);
                if results.send((project, count)).is_err() {
                    break;
                }
            }
        })
        .expect("spawn git thread");
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
    if app.filtering {
        handle_filter(app, key);
        return;
    }
    match (&app.overlay, key.code) {
        (Overlay::Confirm { .. }, KeyCode::Char('y') | KeyCode::Enter) => {
            for job in app.confirm() {
                let _ = jobs.send(job);
            }
        }
        (Overlay::Confirm { .. }, KeyCode::Char('t')) => app.toggle_confirm_mode(),
        (Overlay::Confirm { .. }, _) => app.overlay = Overlay::None,
        (Overlay::Help, _) => app.overlay = Overlay::None,
        (Overlay::None, code) => match code {
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
                for job in app.delete() {
                    let _ = jobs.send(job);
                }
            }
            KeyCode::Enter => {
                for job in app.delete_selected() {
                    let _ = jobs.send(job);
                }
            }
            KeyCode::Char('s') => app.cycle_sort(),
            KeyCode::Char('p') => app.toggle_grouping(),
            KeyCode::Char('P') => app.select_project(),
            KeyCode::Char('/') => app.filtering = true,
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
    let handle = scan::start(config, roots, Cancel::new()).map_err(std::io::Error::other)?;
    let (job_tx, job_rx) = unbounded::<Job>();
    let (result_tx, result_rx) = unbounded::<Outcome>();
    let stop = Arc::new(AtomicBool::new(false));
    let deleter = spawn_deleter(Guard::new(roots), job_rx, result_tx, Arc::clone(&stop));
    let mut app = App::new(config.delete.mode.into(), confirm);
    let started_at = history::now();
    let mut deleted = Vec::new();
    let filesystems = distinct_filesystems(roots);
    let mut measured: Option<Instant> = None;
    let (project_tx, project_rx) = unbounded::<PathBuf>();
    let (changes_tx, changes_rx) = unbounded::<Changes>();
    spawn_git(project_rx, changes_tx);
    let mut projects = HashSet::new();

    let mut terminal = ratatui::try_init()?;
    let outcome = (|| -> std::io::Result<()> {
        let mut scanning = true;
        while !app.quit {
            for _ in 0..EVENTS_PER_FRAME {
                if !scanning {
                    break;
                }
                match handle.events.try_recv() {
                    Ok(Event::Candidate(candidate)) => {
                        let project = project_of(&candidate);
                        if projects.insert(project.to_path_buf()) {
                            let _ = project_tx.send(project.to_path_buf());
                        }
                        app.push(candidate);
                    }
                    Ok(Event::Issue(_)) => app.issues += 1,
                    Ok(Event::Finished(summary)) => {
                        app.finished = Some(summary);
                        scanning = false;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => scanning = false,
                }
            }
            while let Ok((project, count)) = changes_rx.try_recv() {
                if let Some(count) = count {
                    app.changes.insert(project, count);
                }
            }
            let mut removed = false;
            while let Ok(outcome) = result_rx.try_recv() {
                record(&mut app, &mut deleted, outcome);
                removed = true;
            }
            if removed || measured.is_none_or(|at| at.elapsed() >= DISK_REFRESH) {
                app.free = available_space(&filesystems);
                measured = Some(Instant::now());
            }
            if scanning {
                app.progress = handle.progress();
            }
            app.refresh();
            terminal.draw(|frame| view::draw(frame, &app))?;
            if event::poll(FRAME)?
                && let TermEvent::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
            {
                handle_key(&mut app, key, &job_tx);
            }
        }
        Ok(())
    })();
    ratatui::restore();

    stop.store(true, Ordering::Relaxed);
    drop(job_tx);
    drop(project_tx);
    drop(handle);
    let _ = deleter.join();
    while let Ok(result) = result_rx.try_recv() {
        record(&mut app, &mut deleted, result);
    }
    save_history(started_at, roots, deleted);
    outcome
}

fn record(app: &mut App, deleted: &mut Vec<Entry>, (id, mode, result): Outcome) {
    if result.is_ok()
        && mode != Mode::DryRun
        && let Some(item) = app.item(id)
    {
        deleted.push(Entry::new(&item.candidate, mode));
    }
    app.finish(id, mode, result);
}

fn distinct_filesystems(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut devices = HashSet::new();
    roots
        .iter()
        .filter(|root| {
            std::fs::metadata(root)
                .is_ok_and(|metadata| devices.insert(platform::device(&metadata)))
        })
        .cloned()
        .collect()
}

fn available_space(filesystems: &[PathBuf]) -> Option<u64> {
    let sizes: Vec<u64> = filesystems
        .iter()
        .filter_map(|path| platform::available_space(path))
        .collect();
    (!sizes.is_empty()).then(|| sizes.iter().sum())
}

fn save_history(started_at: String, roots: &[PathBuf], deleted: Vec<Entry>) {
    if let Err(error) = history::save(started_at, roots, deleted) {
        eprintln!("kuizer: {error}");
    }
}
