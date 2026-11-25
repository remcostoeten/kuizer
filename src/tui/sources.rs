use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};

use super::jobs::{self, Job, Outcome};
use crate::caches::{self, Locations};
use crate::config::Config;
use crate::dupes::{self, Duplicates, Options, Stage};
use crate::history::HistoryError;
use crate::protect::Protected;
use crate::scan::{self, Cancel, Candidate};

const STAGE_EVERY: Duration = Duration::from_millis(50);

pub enum Found {
    Cache(Candidate),
    CachesDone,
    CachesFailed(String),
    Stage(Stage),
    Duplicates(Duplicates),
    DuplicatesFailed(String),
}

pub struct CacheWork {
    pub config: Config,
    pub jobs: Receiver<Job>,
    pub results: Sender<Outcome>,
    pub stop: Arc<AtomicBool>,
    pub cancel: Cancel,
    pub found: Sender<Found>,
}

pub fn spawn_caches(work: CacheWork) -> io::Result<JoinHandle<Option<HistoryError>>> {
    thread::Builder::new()
        .name("kuizer-caches".to_owned())
        .spawn(move || {
            let Some(locations) = Locations::for_user() else {
                let _ = work
                    .found
                    .send(Found::CachesFailed("no home folder".to_owned()));
                return None;
            };
            let listed = caches::find_excluding(
                &locations,
                &Protected::for_user(),
                &work.config.usable_excludes(),
            );
            let entries = caches::measure_threaded(
                listed,
                &work.cancel,
                scan::size_threads(&work.config),
                |candidate| {
                    if candidate.usage.entries > 0 {
                        let _ = work.found.send(Found::Cache(candidate.clone()));
                    }
                },
            );
            let _ = work.found.send(Found::CachesDone);
            if work.cancel.is_cancelled() {
                return None;
            }
            let candidates: Vec<Candidate> = entries
                .into_iter()
                .map(|(_, candidate)| candidate)
                .collect();
            let deleter = jobs::spawn_deleters(
                caches::guard_roots(&candidates),
                work.config.delete.clone(),
                work.jobs,
                work.results,
                work.stop,
            );
            match deleter {
                Ok(deleter) => deleter.join().ok().flatten(),
                Err(error) => {
                    let _ = work.found.send(Found::CachesFailed(format!(
                        "could not start the cache delete worker: {error}"
                    )));
                    None
                }
            }
        })
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
