use std::path::PathBuf;
use std::time::SystemTime;

use kuizer::caches;
use kuizer::snapshot::{self as core, SavedHistory};

use crate::model::{Disk, HistoryView, Loaded, Report};
use crate::start::ScanOptions;

pub use kuizer::snapshot::{Forgotten, Snapshots, age_label};

pub struct Recorder(core::Recorder);

impl Recorder {
    pub fn new(roots: &[PathBuf], options: &ScanOptions, forgotten: Forgotten) -> Self {
        Self(core::Recorder::new(roots, options, forgotten))
    }

    pub fn record(&self, loaded: &Loaded) {
        match loaded {
            Loaded::Found(candidates) => self.0.record_candidates(candidates),
            Loaded::Artifacts {
                candidates,
                issues,
                summary,
                disks,
            } => self
                .0
                .record_artifacts(candidates, issues, summary, &disk_values(disks)),
            Loaded::ArtifactsDone {
                issues,
                summary,
                disks,
            } => self
                .0
                .record_artifact_metadata(issues, summary, &disk_values(disks)),
            Loaded::Caches(caches) => self.0.record_caches(caches),
            Loaded::Toolchains(toolchains) => self.0.record_toolchains(toolchains),
            Loaded::Docker { entries, issues } => self.0.record_docker(entries, issues),
            Loaded::Repos(repositories) => self.0.record_repositories(repositories),
            Loaded::History(history) => self.0.record_history(
                history
                    .as_ref()
                    .map(|view| SavedHistory {
                        history: view.history.clone(),
                        kept: view.kept.clone(),
                        trash_known: view.trash_known,
                    })
                    .map_err(Clone::clone),
            ),
        }
    }

    pub fn save(self, snapshots: &Snapshots, scanned: SystemTime) {
        self.0.save(snapshots, scanned);
    }
}

pub fn load(snapshots: &Snapshots, roots: &[PathBuf], options: &ScanOptions) -> Option<Report> {
    let saved = snapshots.load(roots, options)?;
    let caches = saved
        .caches
        .into_iter()
        .filter_map(|(name, path, candidate)| Some((caches::known(&name, path)?, candidate)))
        .collect();
    let history = saved.history.map(|saved| HistoryView {
        history: saved.history,
        kept: saved.kept,
        trash_known: saved.trash_known,
    });
    let disks = saved
        .disks
        .into_iter()
        .map(|(available, total)| Disk { available, total })
        .collect();
    Some(Report {
        roots: saved.roots,
        options: saved.options,
        candidates: saved.candidates,
        issues: saved.issues,
        summary: saved.summary,
        caches,
        toolchains: saved.toolchains,
        docker: kuizer::docker::entries(saved.docker),
        docker_issues: saved.docker_issues,
        repos: saved.repos,
        duplicates: None,
        history,
        disks,
        scanned: saved.saved,
    })
}

fn disk_values(disks: &[Disk]) -> Vec<(Option<u64>, Option<u64>)> {
    disks
        .iter()
        .map(|disk| (disk.available, disk.total))
        .collect()
}
