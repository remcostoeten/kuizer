use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};

use crate::config::DeleteConfig;
use crate::delete::{self, Executor, Mode, Status};
use crate::git;
use crate::history::HistoryError;
use crate::platform;

pub use crate::delete::Job;

const STATUS_WORKERS: usize = 3;
const DISK_REFRESH: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitState {
    NotRepo,
    Changes(usize),
    Unknown,
}

pub type Outcome = (u64, Mode, Result<(), String>);
pub type Changes = (PathBuf, GitState);

pub fn spawn_deleters(
    roots: Vec<PathBuf>,
    delete_config: DeleteConfig,
    jobs: Receiver<Job>,
    results: Sender<Outcome>,
    stop: Arc<AtomicBool>,
) -> io::Result<thread::JoinHandle<Option<HistoryError>>> {
    thread::Builder::new()
        .name("kuizer-delete".to_owned())
        .spawn(move || {
            let executor = Executor::new(&roots, &delete_config).with_stop(stop);
            serve(executor, &roots, &jobs, &results)
        })
}

pub fn serve(
    executor: Executor,
    roots: &[PathBuf],
    jobs: &Receiver<Job>,
    results: &Sender<Outcome>,
) -> Option<HistoryError> {
    let guard = executor.guard(roots);
    executor.serve(&guard, jobs, delete::workers(), |mode, removal| {
        let result = match removal.status {
            Status::Failed => Err(removal.error.unwrap_or_default()),
            Status::Removed | Status::WouldRemove => Ok(()),
        };
        results.send((removal.candidate.id, mode, result)).is_ok()
    });
    executor.finish().err()
}

fn repository_dir(project: &Path) -> Option<&Path> {
    project
        .ancestors()
        .find(|dir| dir.join(".git").symlink_metadata().is_ok())
}

fn changes_in(project: &Path, known: &Mutex<HashMap<PathBuf, GitState>>) -> GitState {
    let Some(repository) = repository_dir(project) else {
        return GitState::NotRepo;
    };
    if let Some(state) = known
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(repository)
    {
        return *state;
    }
    let state = match git::uncommitted_changes(repository) {
        Some(count) => GitState::Changes(count),
        None => GitState::Unknown,
    };
    known
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(repository.to_path_buf(), state);
    state
}

pub fn spawn_status(projects: Receiver<PathBuf>, results: Sender<Changes>) -> io::Result<()> {
    let known = Arc::new(Mutex::new(HashMap::new()));
    for index in 0..STATUS_WORKERS {
        let projects = projects.clone();
        let results = results.clone();
        let known = Arc::clone(&known);
        let spawned = thread::Builder::new()
            .name(format!("kuizer-git-{index}"))
            .spawn(move || {
                for project in projects {
                    let count = changes_in(&project, &known);
                    if results.send((project, count)).is_err() {
                        break;
                    }
                }
            });
        if let Err(error) = spawned {
            return if index == 0 { Err(error) } else { Ok(()) };
        }
    }
    Ok(())
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

pub fn spawn_disk(
    roots: Vec<PathBuf>,
    nudges: Receiver<()>,
    results: Sender<Option<u64>>,
) -> io::Result<()> {
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
        .map(drop)
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
