use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use serde::Serialize;

use crate::detect::{Kind, Match};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Usage {
    pub allocated: u64,
    pub apparent: u64,
    pub entries: u64,
    pub partial: bool,
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub id: u64,
    pub path: PathBuf,
    pub kind: Kind,
    pub rule: Match,
    pub project: Option<PathBuf>,
    pub usage: Usage,
    pub newest: Option<SystemTime>,
    pub identity: Option<(u64, u64)>,
    pub project_last_active: Option<SystemTime>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum IssueKind {
    PermissionDenied,
    Vanished,
    Io,
    SkippedMount,
    InvalidRoot,
    Protected,
}

#[derive(Debug, Clone, Serialize)]
pub struct Issue {
    pub path: PathBuf,
    pub kind: IssueKind,
    pub message: String,
}

impl Issue {
    pub fn from_io(path: PathBuf, error: &io::Error) -> Self {
        let kind = match error.kind() {
            io::ErrorKind::PermissionDenied => IssueKind::PermissionDenied,
            io::ErrorKind::NotFound => IssueKind::Vanished,
            _ => IssueKind::Io,
        };
        Self {
            path,
            kind,
            message: error.to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Progress {
    pub dirs: u64,
    pub files: u64,
    pub candidates: u64,
    pub bytes: u64,
    pub issues: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub progress: Progress,
    #[serde(serialize_with = "millis")]
    pub duration: Duration,
    pub cancelled: bool,
}

fn millis<S: serde::Serializer>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_u128(duration.as_millis())
}

#[derive(Debug)]
pub enum Event {
    Candidate(Candidate),
    Issue(Issue),
    Finished(Summary),
}

#[derive(Debug, Default)]
pub struct Counters {
    pub dirs: AtomicU64,
    pub files: AtomicU64,
    pub candidates: AtomicU64,
    pub bytes: AtomicU64,
    pub issues: AtomicU64,
}

impl Counters {
    pub fn add(counter: &AtomicU64, amount: u64) {
        counter.fetch_add(amount, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> Progress {
        Progress {
            dirs: self.dirs.load(Ordering::Relaxed),
            files: self.files.load(Ordering::Relaxed),
            candidates: self.candidates.load(Ordering::Relaxed),
            bytes: self.bytes.load(Ordering::Relaxed),
            issues: self.issues.load(Ordering::Relaxed),
        }
    }
}
