use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::SystemTime;

use super::Candidate;
use crate::detect::{Detector, Listing};
use crate::git;

/// @name annotate
/// @description Fills `project_last_active` on every candidate with the activity of its project:
/// the later of the newest file modification outside the project's artifacts and nested
/// projects, and the last commit of its git repository. Anything `detector` recognises as an
/// artifact is skipped, reported or not, because installs and builds refresh those times in
/// projects that are otherwise abandoned. With a
/// `cutoff`, a project stops being walked at the first file newer than it. A project without
/// readable files or git history keeps `None`.
///
/// @example
/// scan::annotate(&mut candidates, &detector, &config.scan.skip_dirs, Some(cutoff), 4);
pub fn annotate(
    candidates: &mut [Candidate],
    detector: &Detector,
    skip_dirs: &[String],
    cutoff: Option<SystemTime>,
    threads: usize,
) {
    let artifacts: BTreeSet<PathBuf> = candidates.iter().map(|c| c.path.clone()).collect();
    let projects: Vec<PathBuf> = candidates
        .iter()
        .filter_map(|c| c.project.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let activity = Mutex::new(BTreeMap::new());
    let commits = Mutex::new(HashMap::new());
    let next = AtomicUsize::new(0);
    thread::scope(|scope| {
        for _ in 0..threads.clamp(1, projects.len().max(1)) {
            scope.spawn(|| {
                while let Some(project) = projects.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let nested: BTreeSet<&Path> = projects
                        .iter()
                        .filter(|other| *other != project && other.starts_with(project))
                        .map(PathBuf::as_path)
                        .collect();
                    let last = project_activity(
                        project,
                        &Skip {
                            detector,
                            artifacts: &artifacts,
                            nested: &nested,
                            names: skip_dirs,
                        },
                        cutoff,
                        &commits,
                    );
                    activity
                        .lock()
                        .expect("activity lock")
                        .insert(project.clone(), last);
                }
            });
        }
    });
    let activity = activity.into_inner().expect("activity lock");
    for candidate in candidates {
        candidate.project_last_active = candidate
            .project
            .as_ref()
            .and_then(|project| activity.get(project).copied().flatten());
    }
}

struct Skip<'a> {
    detector: &'a Detector,
    artifacts: &'a BTreeSet<PathBuf>,
    nested: &'a BTreeSet<&'a Path>,
    names: &'a [String],
}

impl Skip<'_> {
    fn contains(&self, path: &Path) -> bool {
        self.artifacts.contains(path)
            || self.nested.contains(path)
            || path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| self.names.iter().any(|skip| skip == name))
    }
}

fn project_activity(
    project: &Path,
    skip: &Skip,
    cutoff: Option<SystemTime>,
    commits: &Mutex<HashMap<PathBuf, Option<SystemTime>>>,
) -> Option<SystemTime> {
    let commit = git::repository_root(project).and_then(|root| {
        if let Some(known) = commits.lock().expect("commit lock").get(&root) {
            return *known;
        }
        let time = git::last_commit(&root);
        commits.lock().expect("commit lock").insert(root, time);
        time
    });
    if commit
        .zip(cutoff)
        .is_some_and(|(time, cutoff)| time > cutoff)
    {
        return commit;
    }
    let files = newest_file(project, skip, cutoff);
    commit.max(files)
}

fn newest_file(project: &Path, skip: &Skip, cutoff: Option<SystemTime>) -> Option<SystemTime> {
    let mut newest: Option<SystemTime> = None;
    let mut pending: Vec<(PathBuf, Option<Arc<Listing>>)> = vec![(project.to_path_buf(), None)];
    while let Some((dir, parent)) = pending.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        let children: Vec<(OsString, fs::FileType)> = entries
            .flatten()
            .filter_map(|entry| Some((entry.file_name(), entry.file_type().ok()?)))
            .collect();
        let own = Listing::new(children.iter().map(|(name, _)| name.clone()).collect());
        if let Some(parent) = parent
            && skip.detector.classify_dir(&dir, &parent, &own).is_some()
        {
            continue;
        }
        let own = Arc::new(own);
        for (name, file_type) in children {
            let path = dir.join(&name);
            if file_type.is_dir() {
                if !skip.contains(&path) {
                    pending.push((path, Some(Arc::clone(&own))));
                }
                continue;
            }
            if !file_type.is_file() || skip.detector.classify_file(&name, &own).is_some() {
                continue;
            }
            let Ok(modified) = fs::symlink_metadata(&path).and_then(|meta| meta.modified()) else {
                continue;
            };
            newest = newest.max(Some(modified));
            if cutoff.is_some_and(|cutoff| modified > cutoff) {
                return newest;
            }
        }
    }
    newest
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use super::*;
    use crate::detect::{Category, Kind, RuleInfo, builtin_rules};
    use crate::scan::Usage;

    const DAY: Duration = Duration::from_secs(86_400);

    fn touch(path: &Path, age: Duration) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir");
        }
        fs::write(path, "x").expect("write");
        let file = fs::File::options().write(true).open(path).expect("open");
        file.set_modified(SystemTime::now() - age)
            .expect("set mtime");
    }

    fn candidate(path: PathBuf) -> Candidate {
        Candidate {
            id: 1,
            project: path.parent().map(Path::to_path_buf),
            path,
            kind: Kind::Dir,
            rule: Arc::new(RuleInfo {
                name: "node-modules".to_owned(),
                category: Category::Dependencies,
                safety: crate::detect::Safety::Safe,
                ecosystem: "node".to_owned(),
            }),
            usage: Usage::default(),
            newest: None,
            identity: None,
            project_last_active: None,
        }
    }

    fn detector() -> Detector {
        Detector::new(&builtin_rules()).expect("builtin rules")
    }

    fn days_idle(candidate: &Candidate) -> Option<u64> {
        candidate
            .project_last_active
            .and_then(|time| SystemTime::now().duration_since(time).ok())
            .map(|elapsed| elapsed.as_secs() / 86_400)
    }

    #[test]
    fn ignores_the_artifact_and_nested_projects() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let app = tmp.path().join("app");
        touch(&app.join("src/index.js"), DAY * 100);
        touch(&app.join("node_modules/pkg/index.js"), Duration::ZERO);
        touch(&app.join("packages/web/main.js"), Duration::ZERO);
        touch(&app.join(".git/HEAD"), Duration::ZERO);
        let mut candidates = [
            candidate(app.join("node_modules")),
            candidate(app.join("packages/web/node_modules")),
        ];
        annotate(&mut candidates, &detector(), &[".git".to_owned()], None, 2);
        assert_eq!(days_idle(&candidates[0]), Some(100));
        assert_eq!(days_idle(&candidates[1]), Some(0));
    }

    #[test]
    fn unknown_activity_stays_none() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let app = tmp.path().join("app");
        touch(&app.join("node_modules/pkg/index.js"), Duration::ZERO);
        let mut candidates = [candidate(app.join("node_modules"))];
        annotate(&mut candidates, &detector(), &[], None, 1);
        assert_eq!(candidates[0].project_last_active, None);
    }

    #[test]
    fn stops_at_the_first_file_newer_than_the_cutoff() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let app = tmp.path().join("app");
        touch(&app.join("a.txt"), DAY * 10);
        touch(&app.join("node_modules/x"), Duration::ZERO);
        let mut candidates = [candidate(app.join("node_modules"))];
        annotate(
            &mut candidates,
            &detector(),
            &[],
            Some(SystemTime::now() - DAY * 30),
            1,
        );
        assert_eq!(days_idle(&candidates[0]), Some(10));
    }

    #[test]
    fn skips_artifacts_that_are_not_candidates() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let app = tmp.path().join("app");
        touch(&app.join("package.json"), DAY * 90);
        touch(&app.join("dist/main.js"), DAY * 90);
        touch(&app.join("node_modules/pkg/index.js"), Duration::ZERO);
        touch(&app.join("tsconfig.tsbuildinfo"), Duration::ZERO);
        let mut candidates = [candidate(app.join("dist"))];
        annotate(&mut candidates, &detector(), &[], None, 1);
        assert_eq!(days_idle(&candidates[0]), Some(90));
    }
}
