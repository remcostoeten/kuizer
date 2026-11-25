mod app;
mod jobs;
mod keys;
mod palette;
mod sources;
mod view;
mod watch;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, TryRecvError, unbounded};
use ratatui::crossterm::event::{self, Event as TermEvent, KeyCode, KeyEvent, KeyEventKind};

use crate::config::Config;
use crate::delete::{Confirmation, Mode};
use crate::history::HistoryError;
use crate::keymap::{Action, Keymap, Scope};
use crate::repos::{self, Walked};
use crate::scan::{Cancel, Event};
use crate::source::Source;
use app::{App, GitWork, Load, Overlay, Tab, project_of};
use jobs::{Changes, Job, Outcome};
use sources::{Found, ListWork};
pub use watch::watch;

const FRAME: Duration = Duration::from_millis(50);
const IDLE: Duration = Duration::from_millis(250);
const REFRESH: Duration = Duration::from_millis(100);
const EVENTS_PER_FRAME: usize = 5_000;
const PAGE: isize = 20;

const LISTED: [Source; 3] = [Source::Cache, Source::Toolchain, Source::Docker];

struct Queues {
    jobs: HashMap<Source, Sender<Job>>,
    git: Sender<GitWork>,
}

fn dispatch(app: &mut App, queues: &Queues, batch: Vec<(u64, Mode)>) {
    for (id, mode) in batch {
        let Some(item) = app.item(id) else {
            continue;
        };
        let source = item.source;
        let Some(queue) = queues.jobs.get(&source) else {
            app.restore_deleting(source);
            return;
        };
        let job = Job {
            source,
            mode,
            candidate: item.candidate.clone(),
        };
        if queue.send(job).is_err() {
            app.restore_deleting(source);
            return;
        }
    }
}

fn dispatch_git(app: &mut App, queues: &Queues, work: Option<GitWork>) {
    let Some(work) = work else {
        return;
    };
    let roots: Vec<PathBuf> = work.1.iter().map(|found| found.root.clone()).collect();
    if queues.git.send(work).is_err() {
        app.git_unavailable(&roots);
    }
}

fn list_only(action: Action) -> bool {
    matches!(
        action,
        Action::ExtendDown
            | Action::ExtendUp
            | Action::Toggle
            | Action::SelectAll
            | Action::SelectNone
            | Action::SelectProject
            | Action::Delete
            | Action::DeleteCursor
            | Action::CycleSort
            | Action::Group
            | Action::Category
            | Action::SafeOnly
    )
}

fn view_of(action: Action) -> Option<Tab> {
    Tab::ALL.into_iter().find(|tab| tab.action() == action)
}

fn receive(app: &mut App, found: Found) {
    match found {
        Found::Item {
            source,
            candidate,
            note,
            label,
        } => app.push_listed(source, candidate, note, label),
        Found::Listed(source, issues) => {
            let feed = app.feed_mut(source);
            feed.load = Load::Done;
            if !issues.is_empty() {
                let problem = issues.join("; ");
                feed.problem = Some(problem.clone());
                notify(app, problem);
            }
        }
        Found::ListFailed(source, problem) => {
            let feed = app.feed_mut(source);
            feed.load = Load::Done;
            feed.problem = Some(problem.clone());
            notify(app, problem);
        }
        Found::Stage(stage) => app.duplicates.stage = Some(stage),
        Found::Duplicates(found) => app.set_duplicates(found),
        Found::DuplicatesFailed(problem) => app.duplicates_failed(problem),
        Found::Repos(inspected) => app.set_repos(inspected),
        Found::Git(done) => app.git_done(done),
    }
}

fn drain(app: &mut App, results: &Receiver<Outcome>, source: Source) -> bool {
    let mut removed = false;
    loop {
        match results.try_recv() {
            Ok((id, mode, result)) => {
                app.finish(id, mode, result);
                removed = true;
            }
            Err(TryRecvError::Disconnected) if app.deleting_in(source) > 0 => {
                app.restore_deleting(source);
                return true;
            }
            Err(_) => return removed,
        }
    }
}

fn notify(app: &mut App, problem: String) {
    app.message = Some(match app.message.take() {
        Some(existing) => format!("{existing}; {problem}"),
        None => problem,
    });
}

fn handle_filter(app: &mut App, key: KeyEvent) {
    match keys::action(&app.keymap, Scope::TuiFilter, key) {
        Some(Action::Back) => {
            app.filtering = false;
            app.set_filter(String::new());
        }
        Some(Action::ApplyFilter) => app.filtering = false,
        Some(Action::FieldBackspace) => {
            let mut filter = app.filter.clone();
            filter.pop();
            app.set_filter(filter);
        }
        _ if !keys::modifiers(key).modified() => {
            if let KeyCode::Char(c) = key.code {
                let mut filter = app.filter.clone();
                filter.push(c);
                app.set_filter(filter);
            }
        }
        _ => {}
    }
}

fn handle_confirm(app: &mut App, key: KeyEvent, queues: &Queues, held: bool) {
    let typing = app.pending_confirmation() == Confirmation::TypeCount;
    let modified = keys::modifiers(key).modified();
    match keys::action(&app.keymap, Scope::TuiConfirm, key) {
        Some(Action::Confirm) if !held => {
            let batch = if typing {
                app.submit_count()
            } else {
                app.confirm()
            };
            dispatch(app, queues, batch);
        }
        Some(Action::Confirm) => {}
        Some(Action::Cancel) => app.close_overlay(),
        Some(Action::FieldBackspace) if typing => app.erase_digit(),
        _ if typing => {
            if let KeyCode::Char(digit) = key.code
                && !modified
            {
                app.type_digit(digit);
            }
        }
        _ if !modified => app.close_overlay(),
        _ => {}
    }
}

fn handle_branches(app: &mut App, key: KeyEvent, queues: &Queues, held: bool) {
    match keys::action(&app.keymap, Scope::TuiConfirm, key) {
        Some(Action::DialogToggle) => app.toggle_branch_focus(),
        Some(Action::Confirm) if !held => {
            let work = app.answer_branches();
            dispatch_git(app, queues, work);
        }
        Some(Action::Cancel) => app.close_overlay(),
        _ => {}
    }
}

fn handle_repos(app: &mut App, key: KeyEvent, queues: &Queues, held: bool) -> bool {
    match keys::action(&app.keymap, Scope::TuiRepos, key) {
        Some(Action::Gc | Action::GcAll | Action::Branches | Action::BranchesAll) if held => {}
        Some(Action::Gc) => {
            let work = app.collect_garbage(false);
            dispatch_git(app, queues, work);
        }
        Some(Action::GcAll) => {
            let work = app.collect_garbage(true);
            dispatch_git(app, queues, work);
        }
        Some(Action::Branches) => app.ask_branches(false),
        Some(Action::BranchesAll) => app.ask_branches(true),
        _ => return false,
    }
    true
}

fn handle_key(app: &mut App, key: KeyEvent, queues: &Queues) {
    let now = Instant::now();
    let held_enter = key.code == KeyCode::Enter && app.enter_repeats(now);
    let held_key = app.action_key_repeats(key, now);
    let held = held_enter || held_key;
    let action = keys::action(&app.keymap, Scope::Tui, key);
    let modified = keys::modifiers(key).modified();
    if app.overlay == Overlay::Quit {
        match keys::action(&app.keymap, Scope::TuiQuit, key) {
            Some(Action::QuitAccept) if !held => app.quit = true,
            Some(Action::QuitAccept) => {}
            Some(Action::QuitCancel) => app.overlay = Overlay::None,
            _ if !modified && action != Some(Action::Quit) => app.overlay = Overlay::None,
            _ => {}
        }
        return;
    }
    if action != Some(Action::SelectProject) || app.filtering || app.overlay != Overlay::None {
        app.disarm_widen();
    }
    if action == Some(Action::Quit) && (!app.filtering && app.overlay == Overlay::None || modified)
    {
        app.request_quit();
        return;
    }
    app.message = None;
    if app.filtering || !matches!(action, Some(Action::ExtendDown | Action::ExtendUp)) {
        app.end_range();
    }
    if app.filtering {
        handle_filter(app, key);
        return;
    }
    if app.overlay == Overlay::None
        && let Some(tab) = action.and_then(view_of)
    {
        app.switch(tab);
        return;
    }
    if app.overlay == Overlay::None
        && app.tab == Tab::Repos
        && handle_repos(app, key, queues, held_key)
    {
        return;
    }
    if app.overlay == Overlay::None && app.tab.source().is_none() && action.is_some_and(list_only) {
        app.message = Some(match app.tab {
            Tab::Repos => {
                "git: pack with git gc or remove merged branches, see ? for keys".to_owned()
            }
            tab => format!(
                "{} are a report, nothing here is selected or deleted",
                tab.label()
            ),
        });
        return;
    }
    match app.overlay {
        Overlay::Confirm { .. } => handle_confirm(app, key, queues, held),
        Overlay::Branches { .. } => handle_branches(app, key, queues, held),
        Overlay::Help => {
            if !modified {
                app.overlay = Overlay::None;
            }
        }
        Overlay::Quit => {}
        Overlay::None => match action {
            Some(Action::ExtendDown) => app.extend(1),
            Some(Action::ExtendUp) => app.extend(-1),
            Some(Action::Back) => app.back_out(),
            Some(Action::Down) => app.move_by(1),
            Some(Action::Up) => app.move_by(-1),
            Some(Action::PageDown) => app.move_by(PAGE),
            Some(Action::PageUp) => app.move_by(-PAGE),
            Some(Action::HalfPageDown) => app.move_by(PAGE / 2),
            Some(Action::HalfPageUp) => app.move_by(-PAGE / 2),
            Some(Action::First) => app.move_to(0),
            Some(Action::Last) => app.move_to(usize::MAX),
            Some(Action::Toggle) => app.toggle(),
            Some(Action::SelectAll) => app.select_safe(),
            Some(Action::SelectNone) => app.clear_selection(),
            Some(Action::DeleteCursor) if !held_key => {
                let batch = app.delete();
                dispatch(app, queues, batch);
            }
            Some(Action::Delete) if !held_key => {
                let batch = app.delete_selected();
                dispatch(app, queues, batch);
            }
            Some(Action::CycleSort) => app.cycle_sort(),
            Some(Action::Group) => app.toggle_grouping(),
            Some(Action::SelectProject) if !app.project_key_repeats(now) => app.select_project(),
            Some(Action::FilterFocus) => app.filtering = true,
            Some(Action::Category) => app.cycle_category(),
            Some(Action::SafeOnly) => app.toggle_safe_only(),
            Some(Action::ResetFilters) => app.reset_filters(),
            Some(Action::Help) => app.overlay = Overlay::Help,
            _ => {}
        },
    }
}

type Pending = (Receiver<Job>, Sender<Outcome>);

struct Background {
    config: Config,
    roots: Vec<PathBuf>,
    cancel: Cancel,
    stop: Arc<AtomicBool>,
    found: Sender<Found>,
    pending: HashMap<Source, Pending>,
    lists: Vec<JoinHandle<Option<HistoryError>>>,
    duplicates: Option<JoinHandle<()>>,
    repositories: Option<Vec<PathBuf>>,
    repos: Option<JoinHandle<()>>,
}

impl Background {
    fn start(&mut self, app: &mut App) {
        if let Some(source) = app.tab.source()
            && app.feed(source).load == Load::Idle
            && let Some((jobs, results)) = self.pending.remove(&source)
        {
            app.feed_mut(source).load = Load::Loading;
            let work = ListWork {
                config: self.config.clone(),
                roots: self.roots.clone(),
                jobs,
                results,
                stop: Arc::clone(&self.stop),
                cancel: self.cancel.clone(),
                found: self.found.clone(),
            };
            match sources::spawn_list(source, work) {
                Ok(handle) => self.lists.push(handle),
                Err(error) => {
                    let problem = format!("could not list {}: {error}", app.tab.label());
                    let feed = app.feed_mut(source);
                    feed.load = Load::Done;
                    feed.problem = Some(problem);
                }
            }
        }
        if app.tab == Tab::Duplicates && app.duplicates.load == Load::Idle {
            app.duplicates.load = Load::Loading;
            match sources::spawn_duplicates(
                self.config.clone(),
                self.roots.clone(),
                self.cancel.clone(),
                self.found.clone(),
            ) {
                Ok(handle) => self.duplicates = Some(handle),
                Err(error) => {
                    app.duplicates_failed(format!("could not look for duplicates: {error}"));
                }
            }
        }
        if app.tab == Tab::Repos
            && app.repos.load == Load::Idle
            && let Some(roots) = self.repositories.take()
        {
            app.repos.load = Load::Loading;
            app.repos.checking = roots.len();
            match sources::spawn_repos(
                roots,
                self.config.git.clone(),
                self.cancel.clone(),
                self.found.clone(),
            ) {
                Ok(handle) => self.repos = Some(handle),
                Err(error) => {
                    app.repos.load = Load::Done;
                    app.repos.problem = Some(format!("could not read repositories: {error}"));
                }
            }
        }
    }

    fn walked(&mut self, app: &mut App, walked: Result<Walked, TryRecvError>) -> bool {
        match walked {
            Ok(walked) => {
                self.repositories = Some(walked.repositories);
                self.start(app);
                true
            }
            Err(TryRecvError::Disconnected) => {
                if app.repos.load == Load::Idle {
                    app.repos.load = Load::Done;
                    app.repos.problem = Some("the scan did not list repositories".to_owned());
                }
                true
            }
            Err(TryRecvError::Empty) => false,
        }
    }

    fn finish(self) {
        self.cancel.cancel();
        if let Some(handle) = self.duplicates {
            let _ = handle.join();
        }
        if let Some(handle) = self.repos {
            let _ = handle.join();
        }
        for handle in self.lists {
            if let Ok(Some(error)) = handle.join() {
                eprintln!("kuizer: {error}");
            }
        }
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
    app.keymap = Keymap::resolve(&config.keys).0;
    app.min_gain = config.git.min_gain.0;
    app.message = notice;
    let mut terminal = ratatui::try_init()?;
    let started = terminal
        .draw(|frame| view::draw(frame, &app))
        .map(drop)
        .and_then(|()| {
            repos::start_scan(config, roots, Cancel::new(), None, false)
                .map_err(std::io::Error::other)
        });
    let (handle, walked_rx) = match started {
        Ok(started) => started,
        Err(error) => {
            ratatui::restore();
            return Err(error);
        }
    };
    let (job_tx, job_rx) = unbounded::<Job>();
    let (result_tx, result_rx) = unbounded::<Outcome>();
    let mut job_queues = HashMap::from([(Source::Artifact, job_tx)]);
    let mut results = vec![(Source::Artifact, result_rx)];
    let mut pending = HashMap::new();
    for source in LISTED {
        let (jobs_tx, jobs_rx) = unbounded::<Job>();
        let (outcome_tx, outcome_rx) = unbounded::<Outcome>();
        job_queues.insert(source, jobs_tx);
        results.push((source, outcome_rx));
        pending.insert(source, (jobs_rx, outcome_tx));
    }
    let (git_tx, git_rx) = unbounded::<GitWork>();
    let queues = Queues {
        jobs: job_queues,
        git: git_tx,
    };
    let stop = Arc::new(AtomicBool::new(false));
    let (found_tx, found_rx) = unbounded::<Found>();
    let git = match sources::spawn_git(
        config.git.clone(),
        git_rx,
        Arc::clone(&stop),
        found_tx.clone(),
    ) {
        Ok(git) => Some(git),
        Err(error) => {
            notify(&mut app, format!("could not start git: {error}"));
            None
        }
    };
    let mut background = Background {
        config: config.clone(),
        roots: roots.to_vec(),
        cancel: Cancel::new(),
        stop: Arc::clone(&stop),
        found: found_tx,
        pending,
        lists: Vec::new(),
        duplicates: None,
        repositories: None,
        repos: None,
    };
    let deleter = match jobs::spawn_deleters(
        roots.to_vec(),
        config.delete.clone(),
        job_rx,
        result_tx,
        Arc::clone(&stop),
    ) {
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
        let mut walking = true;
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
                    Ok(Event::Issue(issue)) => app.push_issue(issue),
                    Ok(Event::Finished(summary)) => {
                        app.progress = summary.progress;
                        app.finished = Some(summary);
                        scanning = false;
                        redraw = true;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => scanning = false,
                }
            }
            if walking && background.walked(&mut app, walked_rx.try_recv()) {
                walking = false;
                redraw = true;
            }
            let updates: Vec<_> = changes_rx.try_iter().collect();
            if !updates.is_empty() {
                app.set_statuses(updates);
                redraw = true;
            }
            for found in found_rx.try_iter() {
                receive(&mut app, found);
                redraw = true;
            }
            let mut removed = false;
            for (source, outcomes) in &results {
                removed |= drain(&mut app, outcomes, *source);
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
            let busy = scanning || app.deleting() > 0 || app.awaiting_status() > 0 || app.loading();
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
                    handle_key(&mut app, key, &queues);
                    background.start(&mut app);
                }
                redraw = true;
                let confirming = matches!(
                    app.overlay,
                    Overlay::Confirm { .. } | Overlay::Branches { .. } | Overlay::Quit
                );
                if app.quit || confirming || !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }
        Ok(())
    })();
    ratatui::restore();

    stop.store(true, Ordering::Relaxed);
    drop(queues);
    drop(project_tx);
    drop(nudge_tx);
    drop(handle);
    if let Some(deleter) = deleter
        && let Ok(Some(error)) = deleter.join()
    {
        eprintln!("kuizer: {error}");
    }
    background.finish();
    if let Some(git) = git {
        let _ = git.join();
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

    use ratatui::crossterm::event::KeyModifiers;

    fn queues() -> (Queues, Receiver<Job>) {
        let (jobs, queued) = unbounded::<Job>();
        let (git, _) = unbounded::<GitWork>();
        (
            Queues {
                jobs: Source::ALL
                    .into_iter()
                    .map(|source| (source, jobs.clone()))
                    .collect(),
                git,
            },
            queued,
        )
    }

    fn split_queues() -> (Queues, HashMap<Source, Receiver<Job>>, Receiver<GitWork>) {
        let mut senders = HashMap::new();
        let mut receivers = HashMap::new();
        for source in Source::ALL {
            let (jobs, queued) = unbounded::<Job>();
            senders.insert(source, jobs);
            receivers.insert(source, queued);
        }
        let (git, git_work) = unbounded::<GitWork>();
        (Queues { jobs: senders, git }, receivers, git_work)
    }

    fn press(app: &mut App, jobs: &Queues, code: KeyCode, modifiers: KeyModifiers) {
        handle_key(app, KeyEvent::new(code, modifiers), jobs);
    }

    #[test]
    fn holding_enter_never_confirms_a_deletion() {
        let (jobs, queued) = queues();
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
        let (jobs, _queued) = queues();
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
        let (jobs, _queued) = queues();
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
        let (jobs, _queued) = queues();
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
        let (jobs, queued) = queues();
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
        let (jobs, queued) = queues();
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
        let (jobs, queued) = queues();
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

    #[test]
    fn number_keys_switch_views_and_each_list_deletes_only_its_own_source() {
        let (queues, queued, _) = split_queues();
        let mut app = App::new(Mode::Trash, false);
        for id in 1..=3 {
            app.push(candidate(id, &format!("/p/{id}/node_modules")));
        }
        app.refresh();
        app.set_changes_unknown();
        let listed = [
            ('2', Tab::Caches, Source::Cache, "/home/me/.cache/pip"),
            (
                '5',
                Tab::Toolchains,
                Source::Toolchain,
                "/home/me/.rustup/toolchains/1.80",
            ),
            ('6', Tab::Docker, Source::Docker, "docker:image/3f2a"),
        ];
        for (_, _, source, path) in listed {
            app.push_listed(source, candidate(1, path), String::new(), String::new());
        }
        press(&mut app, &queues, KeyCode::Char(' '), KeyModifiers::NONE);
        assert_eq!(app.selected_count(), 1);

        for (key, tab, source, path) in listed {
            press(&mut app, &queues, KeyCode::Char(key), KeyModifiers::NONE);
            app.refresh();
            assert_eq!(app.tab, tab);
            assert_eq!(app.view.len(), 1);
            assert_eq!(app.selected_count(), 0);
            press(&mut app, &queues, KeyCode::Char('d'), KeyModifiers::NONE);
            assert_eq!(app.overlay, Overlay::None);
            let job = queued[&source].try_recv().expect("job for the open view");
            assert_eq!(job.source, source);
            assert_eq!(job.candidate.path, PathBuf::from(path));
            let mode = if source.permanent() {
                Mode::Permanent
            } else {
                Mode::Trash
            };
            assert_eq!(job.mode, mode, "{source:?}");
            assert!(
                queued
                    .iter()
                    .all(|(other, jobs)| *other == source || jobs.is_empty()),
                "{source:?} reached another queue"
            );
        }

        press(&mut app, &queues, KeyCode::Char('1'), KeyModifiers::NONE);
        app.refresh();
        assert_eq!(app.tab, Tab::Artifacts);
        assert_eq!(app.selected_count(), 1);
        assert_eq!(app.cursor, 1);
    }

    #[test]
    fn review_items_need_the_exact_count_typed_and_esc_cancels() {
        let (jobs, queued) = queues();
        let mut app = App::new(Mode::Permanent, false);
        for id in 1..=12 {
            let mut review = candidate(id, &format!("/p/{id}/dist"));
            review.rule = Arc::new(RuleInfo {
                name: "dist".to_owned(),
                category: Category::Build,
                safety: Safety::Review,
                ecosystem: "node".to_owned(),
            });
            app.push(review);
        }
        app.refresh();
        let type_text = |app: &mut App, text: &str| {
            for digit in text.chars() {
                press(app, &jobs, KeyCode::Char(digit), KeyModifiers::NONE);
            }
        };
        let confirm = |app: &mut App| {
            press(app, &jobs, KeyCode::Char('y'), KeyModifiers::NONE);
        };

        for _ in 0..11 {
            press(&mut app, &jobs, KeyCode::Char(' '), KeyModifiers::NONE);
        }
        press(&mut app, &jobs, KeyCode::Char('d'), KeyModifiers::NONE);
        assert_eq!(app.pending_confirmation(), Confirmation::TypeCount);
        confirm(&mut app);
        type_text(&mut app, "12");
        confirm(&mut app);
        assert!(matches!(app.overlay, Overlay::Confirm { .. }));
        assert!(app.typed_wrong);
        assert!(queued.is_empty());

        press(&mut app, &jobs, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.overlay, Overlay::None);
        assert!(queued.is_empty());
        assert_eq!(app.selected_count(), 11);

        press(&mut app, &jobs, KeyCode::Char('d'), KeyModifiers::NONE);
        type_text(&mut app, "112");
        press(&mut app, &jobs, KeyCode::Backspace, KeyModifiers::NONE);
        assert_eq!(app.typed, "11");
        confirm(&mut app);
        assert_eq!(app.overlay, Overlay::None);
        assert_eq!(queued.len(), 11);
    }

    fn repository(root: &str, merged: &[&str]) -> repos::Repository {
        repos::Repository {
            root: PathBuf::from(root),
            git_dir: PathBuf::from(root).join(".git"),
            objects: repos::Objects::default(),
            default_branch: Some("main".to_owned()),
            current_branch: Some("main".to_owned()),
            merged: merged
                .iter()
                .map(|name| repos::Branch {
                    name: (*name).to_owned(),
                    commit: "0".repeat(40),
                })
                .collect(),
            busy: None,
        }
    }

    #[test]
    fn the_branch_confirmation_starts_on_cancel_and_a_held_enter_never_deletes() {
        let (queues, queued, git_work) = split_queues();
        let mut app = app();
        press(&mut app, &queues, KeyCode::Char('7'), KeyModifiers::NONE);
        assert_eq!(app.tab, Tab::Repos);
        app.set_repos(repos::Inspected {
            repositories: vec![
                repository("/p/one", &["old-feature"]),
                repository("/p/two", &["done", "merged"]),
            ],
            issues: Vec::new(),
        });

        press(&mut app, &queues, KeyCode::Char('b'), KeyModifiers::NONE);
        assert!(matches!(
            app.overlay,
            Overlay::Branches { delete: false, .. }
        ));
        press(&mut app, &queues, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.overlay, Overlay::None);
        assert!(git_work.is_empty());

        press(&mut app, &queues, KeyCode::Char('B'), KeyModifiers::SHIFT);
        press(&mut app, &queues, KeyCode::Tab, KeyModifiers::NONE);
        assert!(matches!(
            app.overlay,
            Overlay::Branches { delete: true, .. }
        ));
        press(&mut app, &queues, KeyCode::Enter, KeyModifiers::NONE);
        assert!(
            git_work.is_empty(),
            "an Enter still held from before deletes nothing"
        );
        assert!(queued.values().all(Receiver::is_empty));
        press(&mut app, &queues, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.overlay, Overlay::None);
        assert!(!app.quit);

        press(&mut app, &queues, KeyCode::Char('B'), KeyModifiers::SHIFT);
        press(&mut app, &queues, KeyCode::Tab, KeyModifiers::NONE);
        press(&mut app, &queues, KeyCode::Char('y'), KeyModifiers::NONE);
        let (task, targets) = git_work.try_recv().expect("branches handed to git");
        assert_eq!(task, app::GitTask::Branches);
        assert_eq!(targets.len(), 2);
        assert_eq!(app.repos.running.len(), 2);
        assert!(queued.values().all(Receiver::is_empty));
    }

    #[test]
    fn report_views_never_select_or_delete_and_esc_only_clears_their_search() {
        let (jobs, queued) = queues();
        let mut app = app();
        press(&mut app, &jobs, KeyCode::Char('4'), KeyModifiers::NONE);
        assert_eq!(app.tab, Tab::Issues);
        for path in ["/p/locked", "/p/gone"] {
            app.push_issue(crate::scan::Issue {
                path: PathBuf::from(path),
                kind: crate::scan::IssueKind::PermissionDenied,
                message: "denied".to_owned(),
            });
        }
        for code in [KeyCode::Char(' '), KeyCode::Char('a'), KeyCode::Char('d')] {
            press(&mut app, &jobs, code, KeyModifiers::NONE);
        }
        press(&mut app, &jobs, KeyCode::Enter, KeyModifiers::NONE);
        assert!(queued.is_empty());
        assert!(app.items.iter().all(|item| !item.selected));
        assert_eq!(app.overlay, Overlay::None);

        press(&mut app, &jobs, KeyCode::Char('/'), KeyModifiers::NONE);
        for c in "lock".chars() {
            press(&mut app, &jobs, KeyCode::Char(c), KeyModifiers::NONE);
        }
        press(&mut app, &jobs, KeyCode::Enter, KeyModifiers::NONE);
        app.refresh();
        assert_eq!(app.issues.rows, [0]);
        press(&mut app, &jobs, KeyCode::Esc, KeyModifiers::NONE);
        app.refresh();
        assert_eq!(app.issues.rows, [0, 1]);
        press(&mut app, &jobs, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.tab, Tab::Issues);
        assert!(!app.quit);
    }
}
