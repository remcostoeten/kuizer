use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};

use crate::delete::{self, Guard, Lanes, Mode, Target};
use crate::git;
use crate::history::{self, Entry, HistoryError, Recorder};
use crate::platform;
use crate::scan::Candidate;

const STATUS_WORKERS: usize = 3;
const DISK_REFRESH: Duration = Duration::from_secs(2);

pub struct Job {
    pub id: u64,
    pub target: Target,
    pub mode: Mode,
    pub candidate: Candidate,
}

pub type Outcome = (u64, Mode, Result<(), String>);
pub type Changes = (PathBuf, Option<usize>);

pub fn spawn_deleters(
    roots: Vec<PathBuf>,
    jobs: Receiver<Job>,
    results: Sender<Outcome>,
    stop: Arc<AtomicBool>,
) -> thread::JoinHandle<Option<HistoryError>> {
    thread::Builder::new()
        .name("kuizer-delete".to_owned())
        .spawn(move || {
            let guard = Guard::new(&roots);
            let recorder = Mutex::new(Recorder::for_user(history::now(), &roots));
            let lanes = Lanes::default();
            thread::scope(|scope| {
                for _ in 0..delete::workers() {
                    scope.spawn(|| {
                        for job in &jobs {
                            let result = if stop.load(Ordering::Relaxed) {
                                Err("cancelled".to_owned())
                            } else {
                                let _lane = lanes.claim(&job.target.path);
                                delete::delete(&guard, &job.target, job.mode)
                                    .map_err(|error| error.to_string())
                            };
                            if result.is_ok() && job.mode != Mode::DryRun {
                                let mut recorder =
                                    recorder.lock().unwrap_or_else(PoisonError::into_inner);
                                recorder.add(Entry::new(&job.candidate, job.mode));
                                if jobs.is_empty() {
                                    recorder.flush();
                                }
                            }
                            if results.send((job.id, job.mode, result)).is_err() {
                                break;
                            }
                        }
                    });
                }
            });
            recorder
                .into_inner()
                .unwrap_or_else(PoisonError::into_inner)
                .finish()
                .err()
        })
        .expect("spawn delete thread")
}

fn repository_dir(project: &Path) -> Option<&Path> {
    project
        .ancestors()
        .find(|dir| dir.join(".git").symlink_metadata().is_ok())
}

fn changes_in(project: &Path, known: &Mutex<HashMap<PathBuf, Option<usize>>>) -> Option<usize> {
    let repository = repository_dir(project)?;
    if let Some(count) = known
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(repository)
    {
        return *count;
    }
    let count = git::uncommitted_changes(repository);
    known
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(repository.to_path_buf(), count);
    count
}

pub fn spawn_status(projects: Receiver<PathBuf>, results: Sender<Changes>) {
    let known = Arc::new(Mutex::new(HashMap::new()));
    for index in 0..STATUS_WORKERS {
        let projects = projects.clone();
        let results = results.clone();
        let known = Arc::clone(&known);
        thread::Builder::new()
            .name(format!("kuizer-git-{index}"))
            .spawn(move || {
                for project in projects {
                    let count = changes_in(&project, &known);
                    if results.send((project, count)).is_err() {
                        break;
                    }
                }
            })
            .expect("spawn git thread");
    }
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

pub fn spawn_disk(roots: Vec<PathBuf>, nudges: Receiver<()>, results: Sender<Option<u64>>) {
    thread::Builder::new()
        .name("kuizer-disk".to_owned())
        .spawn(move || {
            let filesystems = distinct_filesystems(&roots);
            loop {
                if results.send(available_space(&filesystems)).is_err() {
                    return;
                }
                match nudges.recv_timeout(DISK_REFRESH) {
                    Err(RecvTimeoutError::Disconnected) => return,
                    Ok(()) | Err(RecvTimeoutError::Timeout) => while nudges.try_recv().is_ok() {},
                }
            }
        })
        .expect("spawn disk thread");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_nearest_repository_and_none_outside_one() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let base = std::fs::canonicalize(tmp.path()).expect("canonical");
        std::fs::create_dir_all(base.join("repo/.git")).expect("git dir");
        std::fs::create_dir_all(base.join("repo/apps/web")).expect("project");
        std::fs::create_dir_all(base.join("plain/web")).expect("plain");

        assert_eq!(
            repository_dir(&base.join("repo/apps/web")),
            Some(base.join("repo").as_path())
        );
        assert!(
            repository_dir(&base.join("plain/web")).is_none_or(|found| !found.starts_with(&base))
        );
    }
}
