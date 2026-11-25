use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};

use super::app::{GitDone, GitTask, GitWork};
use super::jobs::{self, Job, Outcome};
use crate::caches;
use crate::config::{Config, GitConfig};
use crate::delete::Executor;
use crate::docker;
use crate::dupes::{self, Duplicates, Options, Stage};
use crate::history::HistoryError;
use crate::protect::Protected;
use crate::repos::{self, Inspected};
use crate::scan::{self, Cancel, Candidate};
use crate::source::Source;
use crate::toolchains::{self, pins};

const STAGE_EVERY: Duration = Duration::from_millis(50);

pub enum Found {
    Item {
        source: Source,
        candidate: Candidate,
        note: String,
        label: String,
    },
    Listed(Source, Vec<String>),
    ListFailed(Source, String),
    Stage(Stage),
    Duplicates(Duplicates),
    DuplicatesFailed(String),
    Repos(Inspected),
    Git(GitDone),
}

pub struct ListWork {
    pub config: Config,
    pub roots: Vec<PathBuf>,
    pub jobs: Receiver<Job>,
    pub results: Sender<Outcome>,
    pub stop: Arc<AtomicBool>,
    pub cancel: Cancel,
    pub found: Sender<Found>,
}

impl ListWork {
    fn item(&self, source: Source, candidate: &Candidate, note: String, label: String) {
        let _ = self.found.send(Found::Item {
            source,
            candidate: candidate.clone(),
            note,
            label,
        });
    }
}

pub fn spawn_list(source: Source, work: ListWork) -> io::Result<JoinHandle<Option<HistoryError>>> {
    thread::Builder::new()
        .name(format!("kuizer-{}", source_name(source)))
        .spawn(move || {
            let listed = match source {
                Source::Cache => list_caches(&work),
                Source::Toolchain => list_toolchains(&work),
                Source::Docker => list_docker(&work),
                Source::Artifact => Err("artifacts come from the scan".to_owned()),
            };
            let (guard_roots, issues) = match listed {
                Ok(listed) => listed,
                Err(problem) => {
                    let _ = work.found.send(Found::ListFailed(source, problem));
                    return None;
                }
            };
            let _ = work.found.send(Found::Listed(source, issues));
            if work.cancel.is_cancelled() {
                return None;
            }
            let executor = Executor::new(&guard_roots, &work.config.delete)
                .with_engines(&work.config.docker.engines)
                .with_stop(work.stop);
            jobs::serve(executor, &guard_roots, &work.jobs, &work.results)
        })
}

fn source_name(source: Source) -> &'static str {
    match source {
        Source::Artifact => "artifacts",
        Source::Cache => "caches",
        Source::Toolchain => "toolchains",
        Source::Docker => "docker",
    }
}

type Listed = Result<(Vec<PathBuf>, Vec<String>), String>;

fn list_caches(work: &ListWork) -> Listed {
    let locations = caches::Locations::for_user().ok_or("no home folder")?;
    let found = caches::find_excluding(
        &locations,
        &Protected::for_user(),
        &work.config.usable_excludes(),
    );
    let entries = caches::measure_threaded(
        found,
        &work.cancel,
        scan::size_threads(&work.config),
        |candidate| {
            if candidate.usage.entries > 0 {
                let note = caches::known(&candidate.rule.name, candidate.path.clone())
                    .map_or("", |cache| cache.note);
                work.item(Source::Cache, candidate, note.to_owned(), String::new());
            }
        },
    );
    let candidates: Vec<Candidate> = entries
        .into_iter()
        .map(|(_, candidate)| candidate)
        .collect();
    Ok((caches::guard_roots(&candidates), Vec::new()))
}

fn list_toolchains(work: &ListWork) -> Listed {
    let config = &work.config;
    let locations = toolchains::Locations::for_user().ok_or("no home folder")?;
    let mut found = toolchains::find_excluding(
        &locations,
        &config.toolchains.tools,
        &config.usable_excludes(),
    );
    if config.toolchains.pins && !found.is_empty() {
        let pins = pins::find(config, &work.roots, &work.cancel)
            .map_err(|error| format!("could not look for pinned versions: {error}"))?;
        toolchains::apply_pins(&mut found, &pins);
    }
    let notes: HashMap<PathBuf, String> = found
        .iter()
        .map(|toolchain| (toolchain.path.clone(), toolchain.note()))
        .collect();
    let entries = toolchains::measure_with_threads(
        found,
        &work.cancel,
        scan::size_threads(config),
        |candidate| {
            let note = notes.get(&candidate.path).cloned().unwrap_or_default();
            work.item(Source::Toolchain, candidate, note, String::new());
        },
    );
    let candidates: Vec<Candidate> = entries
        .into_iter()
        .map(|(_, candidate)| candidate)
        .collect();
    Ok((toolchains::roots(&candidates), Vec::new()))
}

fn list_docker(work: &ListWork) -> Listed {
    let config = &work.config.docker;
    let listing = docker::list(&config.engines, config.timeout(), config.recent_hours);
    for (item, candidate) in docker::entries(listing.items) {
        work.item(Source::Docker, &candidate, item.note, item.label);
    }
    Ok((Vec::new(), listing.issues))
}

pub fn spawn_duplicates(
    config: Config,
    roots: Vec<PathBuf>,
    cancel: Cancel,
    found: Sender<Found>,
) -> io::Result<JoinHandle<()>> {
    thread::Builder::new()
        .name("kuizer-duplicates".to_owned())
        .spawn(move || {
            let mut reported: Option<Instant> = None;
            let options = Options::from_config(&config);
            let result = dupes::find_cancellable(&config, &roots, options, &cancel, |stage| {
                if reported.is_none_or(|at| at.elapsed() >= STAGE_EVERY) {
                    reported = Some(Instant::now());
                    let _ = found.send(Found::Stage(stage));
                }
            });
            let _ = found.send(match result {
                Ok(duplicates) => Found::Duplicates(duplicates),
                Err(error) => Found::DuplicatesFailed(error.to_string()),
            });
        })
}

pub fn spawn_repos(
    roots: Vec<PathBuf>,
    config: GitConfig,
    cancel: Cancel,
    found: Sender<Found>,
) -> io::Result<JoinHandle<()>> {
    thread::Builder::new()
        .name("kuizer-repos".to_owned())
        .spawn(move || {
            let inspected = repos::inspect_all_collecting(roots, &config, &cancel, |_| {});
            let _ = found.send(Found::Repos(inspected));
        })
}

pub fn spawn_git(
    config: GitConfig,
    work: Receiver<GitWork>,
    stop: Arc<AtomicBool>,
    found: Sender<Found>,
) -> io::Result<JoinHandle<()>> {
    thread::Builder::new()
        .name("kuizer-git".to_owned())
        .spawn(move || {
            for (task, repositories) in work {
                for repository in repositories {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    let (collected, pruned) = match task {
                        GitTask::Gc => (
                            Some(repos::collect_garbage(&repository, &config)),
                            Vec::new(),
                        ),
                        GitTask::Branches => (
                            None,
                            repos::delete_branches(&repository, &repository.merged, &config),
                        ),
                    };
                    let done = GitDone {
                        after: repos::inspect(&repository.root, &config).ok(),
                        root: repository.root,
                        collected,
                        pruned,
                    };
                    if found.send(Found::Git(done)).is_err() {
                        return;
                    }
                }
            }
        })
}
