use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::SystemTime;

use super::walk::Context;
use super::{Cancel, Candidate};
use crate::config::{Excludes, ScanConfig};
use crate::detect::{Detector, Listing};
use crate::git::{self, RootFinder};
use crate::platform::MountTable;
use crate::platform::dir::{Buffer, Dir, EntryKind};
use crate::protect::{self, Protected};

const MAX_ACTIVITY_THREADS: usize = 64;

pub struct ActivityScope<'a> {
    pub detector: &'a Detector,
    pub scan: &'a ScanConfig,
    pub excludes: &'a Excludes,
    pub mounts: &'a MountTable,
    pub protected: &'a Protected,
}

/// @name annotate
/// @description Fills `project_last_active` on every candidate with the activity of its project
/// like [`annotate_in`], under the default scan limits and the given `skip_dirs`: no crossing
/// filesystems, no excluded filesystem types, no protected folders. Prefer [`annotate_in`] with
/// the configuration of the scan that produced the candidates.
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
    let scan = ScanConfig {
        skip_dirs: skip_dirs.to_vec(),
        ..ScanConfig::default()
    };
    let excludes = Excludes::new(&[], &[]).expect("an empty exclude list is valid");
    let mounts = MountTable::load();
    let protected = Protected::for_user();
    annotate_in(
        candidates,
        &ActivityScope {
            detector,
            scan: &scan,
            excludes: &excludes,
            mounts: &mounts,
            protected: &protected,
        },
        cutoff,
        threads,
        &Cancel::new(),
        |_, _| {},
    );
}

/// @name annotate_in
/// @description Fills `project_last_active` on every candidate with the activity of its project:
/// the later of the newest file modification outside the project's artifacts and nested
/// projects, and the last commit of its git repository. Anything `scope.detector` recognises as
/// an artifact is skipped, reported or not, because installs and builds refresh those times in
/// projects that are otherwise abandoned. Folders and files the scan itself would not enter
/// (other filesystems, excluded mounts, excludes, ignore files, protected folders) do not count.
/// With a `cutoff`, a project stops being walked at the first file newer than it. A project
/// without readable files or git history keeps `None`.
///
/// `on_project` gets each project and its activity as soon as it is known, from the worker
/// threads, so a frontend can show results while the rest still runs. Once `cancel` is raised
/// no further results are reported, and candidates whose project was not finished by then are
/// left unknown.
///
/// @example
/// scan::annotate_in(&mut candidates, &scope, None, 4, &cancel, |project, active| show(project, active));
pub fn annotate_in(
    candidates: &mut [Candidate],
    scope: &ActivityScope<'_>,
    cutoff: Option<SystemTime>,
    threads: usize,
    cancel: &Cancel,
    on_project: impl Fn(&Path, Option<SystemTime>) + Sync,
) {
    let artifacts: HashSet<PathBuf> = candidates.iter().map(|c| c.path.clone()).collect();
    let projects: Vec<PathBuf> = candidates
        .iter()
        .filter_map(|c| c.project.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let context = Context::new(
        scope.scan,
        scope.detector,
        scope.excludes,
        scope.mounts,
        scope.protected,
    );
    let roots = RootFinder::from_env();
    let commits = Commits::default();
    let activity = Mutex::new(BTreeMap::new());
    let next = AtomicUsize::new(0);
    let worker = || {
        let mut scratch = Scratch::default();
        while !cancel.is_cancelled() {
            let index = next.fetch_add(1, Ordering::Relaxed);
            let Some(project) = projects.get(index) else {
                break;
            };
            let after = &projects[index + 1..];
            let nested = &after[..after.partition_point(|other| other.starts_with(project))];
            let skip = Skip {
                scope,
                context: &context,
                artifacts: &artifacts,
                nested,
            };
            let commit = roots.find(project).and_then(|root| commits.at(&root));
            let last = project_activity(project, &skip, commit, cutoff, cancel, &mut scratch);
            if cancel.is_cancelled() {
                break;
            }
            on_project(project, last);
            activity
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .insert(project.clone(), last);
        }
    };
    thread::scope(|threads_scope| {
        let extra = threads
            .clamp(1, projects.len().max(1))
            .min(MAX_ACTIVITY_THREADS);
        for index in 1..extra {
            let spawned = thread::Builder::new()
                .name(format!("kuizer-activity-{index}"))
                .spawn_scoped(threads_scope, worker);
            if spawned.is_err() {
                break;
            }
        }
        worker();
    });
    let activity = activity
        .into_inner()
        .unwrap_or_else(|poison| poison.into_inner());
    for candidate in candidates {
        candidate.project_last_active = candidate
            .project
            .as_ref()
            .and_then(|project| activity.get(project).copied().flatten());
    }
}

#[derive(Default)]
struct Commits {
    times: Mutex<HashMap<PathBuf, Arc<OnceLock<Option<SystemTime>>>>>,
}

impl Commits {
    fn at(&self, root: &Path) -> Option<SystemTime> {
        let cell = {
            let mut times = self
                .times
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            Arc::clone(times.entry(root.to_path_buf()).or_default())
        };
        *cell.get_or_init(|| git::last_commit(root))
    }
}

struct Skip<'a> {
    scope: &'a ActivityScope<'a>,
    context: &'a Context<'a>,
    artifacts: &'a HashSet<PathBuf>,
    nested: &'a [PathBuf],
}

impl Skip<'_> {
    fn contains(&self, path: &Path) -> bool {
        self.artifacts.contains(path)
            || self
                .nested
                .binary_search_by(|other| other.as_path().cmp(path))
                .is_ok()
    }

    fn pruned(&self, own: &Listing) -> bool {
        let scope = self.scope;
        (scope.detector.watches(&scope.scan.ignore_file) && scope.scan.ignores(scope.detector, own))
            || protect::holds_app_data(scope.detector, own)
    }
}

fn project_activity(
    project: &Path,
    skip: &Skip,
    commit: Option<SystemTime>,
    cutoff: Option<SystemTime>,
    cancel: &Cancel,
    scratch: &mut Scratch,
) -> Option<SystemTime> {
    if commit
        .zip(cutoff)
        .is_some_and(|(time, cutoff)| time > cutoff)
    {
        return commit;
    }
    let files = newest_file(project, skip, cutoff, cancel, scratch);
    commit.max(files)
}

#[derive(Default)]
struct Scratch {
    buffer: Buffer,
    names: Vec<u8>,
    entries: Vec<(usize, usize, EntryKind)>,
}

struct Frame {
    path: PathBuf,
    parent: Option<Arc<Listing>>,
    device: u64,
    probe: bool,
    splits: bool,
}

fn newest_file(
    project: &Path,
    skip: &Skip,
    cutoff: Option<SystemTime>,
    cancel: &Cancel,
    scratch: &mut Scratch,
) -> Option<SystemTime> {
    let (scope, context) = (skip.scope, skip.context);
    let mut newest: Option<SystemTime> = None;
    let mut pending = vec![Frame {
        path: project.to_path_buf(),
        parent: None,
        device: 0,
        probe: false,
        splits: context.splits(project),
    }];
    while let Some(frame) = pending.pop() {
        if cancel.is_cancelled() {
            return None;
        }
        let Ok(dir) = Dir::open(&frame.path, frame.parent.is_none()) else {
            continue;
        };
        let device = if frame.parent.is_none() {
            match dir.stat() {
                Ok(stat) => stat.device,
                Err(_) => continue,
            }
        } else if frame.probe {
            match context.same_device(&dir, frame.device, &frame.path) {
                Ok(Some(device)) => device,
                _ => continue,
            }
        } else {
            frame.device
        };
        let Scratch {
            buffer,
            names,
            entries,
        } = scratch;
        names.clear();
        entries.clear();
        let _ = dir.read(buffer, |name, kind, _| {
            let start = names.len();
            names.extend_from_slice(name.as_bytes());
            entries.push((start, names.len(), kind));
        });
        let mut own = Listing::default();
        for (start, end, kind) in entries.iter_mut() {
            let name = OsStr::from_bytes(&names[*start..*end]);
            if *kind == EntryKind::Unknown {
                match dir.kind_at(name) {
                    Ok(resolved) => *kind = resolved,
                    Err(_) => continue,
                }
            }
            scope.detector.note(&mut own, name);
        }
        if let Some(parent) = &frame.parent
            && scope
                .detector
                .classify_dir(&frame.path, parent, &own)
                .is_some()
        {
            continue;
        }
        if skip.pruned(&own) {
            continue;
        }
        let own = Arc::new(own);
        for &(start, end, kind) in entries.iter() {
            let name = OsStr::from_bytes(&names[start..end]);
            match kind {
                EntryKind::Dir => {
                    let Some(child) = context.child(&frame.path, name, frame.splits) else {
                        continue;
                    };
                    if skip.contains(&child.path) {
                        continue;
                    }
                    pending.push(Frame {
                        path: child.path,
                        parent: Some(Arc::clone(&own)),
                        device,
                        probe: child.probe,
                        splits: child.splits,
                    });
                }
                EntryKind::File => {
                    if scope.excludes.extension(name)
                        || scope.excludes.name(name)
                        || scope.detector.classify_file(name, &own).is_some()
                        || (scope.excludes.matches_paths()
                            && scope.excludes.full_path(&frame.path.join(name)))
                    {
                        continue;
                    }
                    let Some(modified) = dir.stat_at(name).ok().and_then(|stat| stat.modified)
                    else {
                        continue;
                    };
                    newest = newest.max(Some(modified));
                    if cutoff.is_some_and(|cutoff| modified > cutoff) {
                        return newest;
                    }
                }
                _ => {}
            }
        }
    }
    newest
}

#[cfg(test)]
mod tests {
    use std::fs;
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

    #[test]
    fn leaves_out_what_the_scan_would_not_enter() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let app = tmp.path().join("app");
        touch(&app.join("src/main.js"), DAY * 50);
        touch(&app.join("vendored/lib.js"), Duration::ZERO);
        touch(&app.join("shared/.kuizerignore"), Duration::ZERO);
        touch(&app.join("shared/new.js"), Duration::ZERO);
        touch(&app.join("node_modules/pkg/index.js"), Duration::ZERO);
        let mut candidates = [candidate(app.join("node_modules"))];
        let detector = detector().watching([".kuizerignore", ".sweepignore"]);
        let scan = ScanConfig::default();
        let excludes = Excludes::new(&["vendored".to_owned()], &[]).expect("excludes");
        let mounts = MountTable::load();
        let protected = Protected::default();
        let reported = Mutex::new(Vec::new());
        annotate_in(
            &mut candidates,
            &ActivityScope {
                detector: &detector,
                scan: &scan,
                excludes: &excludes,
                mounts: &mounts,
                protected: &protected,
            },
            None,
            2,
            &Cancel::new(),
            |project, active| {
                reported
                    .lock()
                    .unwrap()
                    .push((project.to_path_buf(), active.is_some()));
            },
        );
        assert_eq!(days_idle(&candidates[0]), Some(50));
        assert_eq!(reported.into_inner().unwrap(), [(app, true)]);
    }

    #[test]
    fn a_cancelled_run_leaves_activity_unknown() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let app = tmp.path().join("app");
        touch(&app.join("src/main.js"), DAY * 50);
        touch(&app.join("node_modules/pkg/index.js"), Duration::ZERO);
        let mut candidates = [candidate(app.join("node_modules"))];
        let cancel = Cancel::new();
        cancel.cancel();
        let detector = detector();
        let scan = ScanConfig::default();
        let excludes = Excludes::new(&[], &[]).expect("excludes");
        let mounts = MountTable::load();
        let protected = Protected::default();
        annotate_in(
            &mut candidates,
            &ActivityScope {
                detector: &detector,
                scan: &scan,
                excludes: &excludes,
                mounts: &mounts,
                protected: &protected,
            },
            None,
            2,
            &cancel,
            |_, _| panic!("nothing is reported after cancel"),
        );
        assert_eq!(candidates[0].project_last_active, None);
    }
}
