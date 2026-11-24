use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::thread;
use std::time::SystemTime;

use super::walk::Context;
use super::{Cancel, Candidate};
use crate::config::{Config, ConfigError, Excludes, ScanConfig};
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
/// @description Fills `project_last_active` on every candidate like [`annotate_in`], within the
/// scope of a scan run with `config`: its rules, ignore file, excludes, skipped folders,
/// filesystem boundary, excluded mounts and protected folders. Runs on `scan.size_threads`.
///
/// @example
/// scan::annotate(&mut candidates, &config, Some(cutoff))?;
pub fn annotate(
    candidates: &mut [Candidate],
    config: &Config,
    cutoff: Option<SystemTime>,
) -> Result<(), ConfigError> {
    let detector = config.detector()?;
    let excludes = config.excludes()?;
    let mounts = MountTable::load();
    let protected = Protected::for_user();
    annotate_in(
        candidates,
        &ActivityScope {
            detector: &detector,
            scan: &config.scan,
            excludes: &excludes,
            mounts: &mounts,
            protected: &protected,
        },
        cutoff,
        super::size_threads(config),
        &Cancel::new(),
        |_, _| {},
    );
    Ok(())
}

/// @name annotate_in
/// @description Fills `project_last_active` on every candidate with the activity of its project:
/// the later of the newest file modification outside its artifacts (including nested projects),
/// and the last commit of its git repository. Anything `scope.detector` recognises as an artifact
/// is skipped, reported or not, because installs and builds refresh those times in projects that
/// are otherwise abandoned. Folders and files the scan itself would not enter (other filesystems,
/// excluded mounts, excludes, ignore files, protected folders) do not count. With a `cutoff`, a
/// project stops being walked at the first file newer than it. A project without readable files
/// or git history, or with an unreadable folder in itself or a nested project (unless activity
/// newer than `cutoff` is already known), keeps `None`.
///
/// `on_project` gets each project and its activity once, as soon as the project and everything
/// nested in it are known, so nested projects come before their parents and a frontend can show
/// results while the rest still runs. Calls come from the worker threads, one at a time. Once
/// `cancel` is raised no further results are reported, and candidates whose project was not
/// finished by then are left unknown.
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
    let (parents, states) = project_tree(&projects);
    let context = Context::new(
        scope.scan,
        scope.detector,
        scope.excludes,
        scope.mounts,
        scope.protected,
    );
    let roots = RootFinder::from_env();
    let commits = Commits::default();
    let states = Mutex::new(states);
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
            let own = project_activity(project, &skip, commit, cutoff, cancel, &mut scratch);
            if cancel.is_cancelled() {
                break;
            }
            let mut states = states.lock().unwrap_or_else(PoisonError::into_inner);
            let mut current = Some((index, own));
            while let Some((idx, incoming)) = current {
                let state = &mut states[idx];
                state.activity = state.activity.merge(incoming, cutoff);
                state.remaining -= 1;
                if state.remaining > 0 || cancel.is_cancelled() {
                    break;
                }
                let resolved = state.activity;
                on_project(&projects[idx], resolved.time());
                current = parents[idx].map(|parent| (parent, resolved));
            }
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
    let states = states.into_inner().unwrap_or_else(PoisonError::into_inner);
    for candidate in candidates {
        candidate.project_last_active = candidate
            .project
            .as_ref()
            .and_then(|project| projects.binary_search(project).ok())
            .filter(|&index| states[index].remaining == 0)
            .and_then(|index| states[index].activity.time());
    }
}

#[derive(Clone, Copy)]
enum Activity {
    Known(Option<SystemTime>),
    Unknown,
}

impl Activity {
    fn merge(self, other: Self, cutoff: Option<SystemTime>) -> Self {
        match (self, other) {
            (Self::Known(a), Self::Known(b)) => Self::Known(a.max(b)),
            (Self::Known(time), Self::Unknown) | (Self::Unknown, Self::Known(time))
                if newer_than(time, cutoff) =>
            {
                Self::Known(time)
            }
            _ => Self::Unknown,
        }
    }

    fn time(self) -> Option<SystemTime> {
        match self {
            Self::Known(time) => time,
            Self::Unknown => None,
        }
    }
}

fn newer_than(time: Option<SystemTime>, cutoff: Option<SystemTime>) -> bool {
    matches!((time, cutoff), (Some(time), Some(cutoff)) if time > cutoff)
}

#[derive(Clone, Copy)]
struct ProjectState {
    activity: Activity,
    remaining: usize,
}

fn project_tree(projects: &[PathBuf]) -> (Vec<Option<usize>>, Vec<ProjectState>) {
    let mut parents = Vec::with_capacity(projects.len());
    let mut states = vec![
        ProjectState {
            activity: Activity::Known(None),
            remaining: 1,
        };
        projects.len()
    ];
    let mut ancestors: Vec<usize> = Vec::new();
    for (index, project) in projects.iter().enumerate() {
        while ancestors
            .last()
            .is_some_and(|&top| !project.starts_with(&projects[top]))
        {
            ancestors.pop();
        }
        let parent = ancestors.last().copied();
        if let Some(parent) = parent {
            states[parent].remaining += 1;
        }
        parents.push(parent);
        ancestors.push(index);
    }
    (parents, states)
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
) -> Activity {
    if newer_than(commit, cutoff) {
        return Activity::Known(commit);
    }
    let files = newest_file(project, skip, cutoff, cancel, scratch);
    Activity::Known(commit).merge(files, cutoff)
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

fn vanished(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::NotFound
}

fn newest_file(
    project: &Path,
    skip: &Skip,
    cutoff: Option<SystemTime>,
    cancel: &Cancel,
    scratch: &mut Scratch,
) -> Activity {
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
            return Activity::Unknown;
        }
        let dir = match Dir::open(&frame.path, frame.parent.is_none()) {
            Ok(dir) => dir,
            Err(error) if vanished(&error) => continue,
            Err(_) => return Activity::Unknown,
        };
        let device = if frame.parent.is_none() {
            match dir.stat() {
                Ok(stat) => stat.device,
                Err(error) if vanished(&error) => continue,
                Err(_) => return Activity::Unknown,
            }
        } else if frame.probe {
            match context.same_device(&dir, frame.device, &frame.path) {
                Ok(Some(device)) => device,
                Ok(None) => continue,
                Err(error) if vanished(&error) => continue,
                Err(_) => return Activity::Unknown,
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
        let listed = dir.read(buffer, |name, kind, _| {
            let start = names.len();
            names.extend_from_slice(name.as_bytes());
            entries.push((start, names.len(), kind));
        });
        if listed.as_ref().is_err_and(|error| !vanished(error)) {
            return Activity::Unknown;
        }
        let mut own = Listing::default();
        for (start, end, kind) in entries.iter_mut() {
            let name = OsStr::from_bytes(&names[*start..*end]);
            if *kind == EntryKind::Unknown {
                match dir.kind_at(name) {
                    Ok(resolved) => *kind = resolved,
                    Err(error) if vanished(&error) => continue,
                    Err(_) => return Activity::Unknown,
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
                    let stat = match dir.stat_at(name) {
                        Ok(stat) => stat,
                        Err(error) if vanished(&error) => continue,
                        Err(_) => return Activity::Unknown,
                    };
                    let Some(modified) = stat.modified else {
                        continue;
                    };
                    newest = newest.max(Some(modified));
                    if cutoff.is_some_and(|cutoff| modified > cutoff) {
                        return Activity::Known(newest);
                    }
                }
                _ => {}
            }
        }
    }
    Activity::Known(newest)
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

    #[test]
    fn an_unreadable_folder_makes_activity_unknown() {
        use std::os::unix::fs::PermissionsExt;
        if rustix::process::geteuid().is_root() {
            return;
        }
        let tmp = tempfile::tempdir().expect("tempdir");
        let project = tmp.path().join("app");
        touch(&project.join("old.txt"), DAY * 400);
        let locked = project.join("locked");
        touch(&locked.join("recent.txt"), Duration::ZERO);
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).expect("lock");
        let config = crate::config::Config::default();
        let mut found = vec![candidate(project.join("node_modules"))];
        fs::create_dir_all(project.join("node_modules")).expect("modules");
        let annotated = crate::scan::annotate(&mut found, &config, Some(SystemTime::now() - DAY));
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).expect("unlock");
        annotated.expect("annotates");
        assert_eq!(found[0].project_last_active, None);
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
    fn nested_project_activity_counts_toward_its_parent() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let app = tmp.path().join("app");
        let web = app.join("packages/web");
        let sibling = tmp.path().join("app-old");
        touch(&app.join("src/index.js"), DAY * 100);
        touch(&app.join("node_modules/pkg/index.js"), Duration::ZERO);
        touch(&app.join(".git/HEAD"), Duration::ZERO);
        touch(&web.join("main.js"), DAY * 20);
        touch(&web.join("node_modules/pkg/index.js"), Duration::ZERO);
        touch(&sibling.join("main.js"), DAY * 200);
        touch(&sibling.join("node_modules/pkg/index.js"), Duration::ZERO);
        let mut candidates = [
            candidate(app.join("node_modules")),
            candidate(web.join("node_modules")),
            candidate(sibling.join("node_modules")),
        ];
        let detector = detector();
        let scan = ScanConfig::default();
        let excludes = Excludes::new(&[], &[]).expect("excludes");
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
            |project, _| reported.lock().unwrap().push(project.to_path_buf()),
        );
        assert_eq!(days_idle(&candidates[0]), Some(20));
        assert_eq!(days_idle(&candidates[1]), Some(20));
        assert_eq!(days_idle(&candidates[2]), Some(200));
        let reported = reported.into_inner().unwrap();
        assert_eq!(reported.len(), 3);
        let web_at = reported.iter().position(|seen| *seen == web);
        let app_at = reported.iter().position(|seen| *seen == app);
        assert!(web_at.is_some() && web_at < app_at);
    }

    #[test]
    fn an_unreadable_nested_project_keeps_its_parent_unknown_unless_active() {
        use std::os::unix::fs::PermissionsExt;
        if rustix::process::geteuid().is_root() {
            return;
        }
        let tmp = tempfile::tempdir().expect("tempdir");
        let app = tmp.path().join("app");
        let web = app.join("packages/web");
        touch(&app.join("old.txt"), DAY * 400);
        touch(&web.join("old.txt"), DAY * 400);
        let locked = web.join("locked");
        touch(&locked.join("recent.txt"), Duration::ZERO);
        fs::create_dir_all(app.join("node_modules")).expect("modules");
        fs::create_dir_all(web.join("node_modules")).expect("modules");
        let targets = || {
            [
                candidate(app.join("node_modules")),
                candidate(web.join("node_modules")),
            ]
        };
        let config = Config::default();
        let cutoff = SystemTime::now() - DAY * 30;
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).expect("lock");
        let mut idle = targets();
        let idle_result = annotate(&mut idle, &config, Some(cutoff));
        let wrote = fs::write(app.join("new.txt"), "x");
        let mut active = targets();
        let active_result = annotate(&mut active, &config, Some(cutoff));
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).expect("unlock");
        idle_result.expect("annotates");
        wrote.expect("write");
        active_result.expect("annotates");
        assert_eq!(idle[0].project_last_active, None);
        assert_eq!(idle[1].project_last_active, None);
        assert_eq!(days_idle(&active[0]), Some(0));
        assert_eq!(active[1].project_last_active, None);
    }

    #[test]
    fn unknown_activity_stays_none() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let app = tmp.path().join("app");
        touch(&app.join("node_modules/pkg/index.js"), Duration::ZERO);
        let mut candidates = [candidate(app.join("node_modules"))];
        annotate(&mut candidates, &Config::default(), None).expect("annotate");
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
            &Config::default(),
            Some(SystemTime::now() - DAY * 30),
        )
        .expect("annotate");
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
        annotate(&mut candidates, &Config::default(), None).expect("annotate");
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
    fn follows_the_configured_scan_scope() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let app = tmp.path().join("app");
        touch(&app.join("src/main.js"), DAY * 50);
        touch(&app.join("vendored/lib.js"), Duration::ZERO);
        touch(&app.join("debug.log"), Duration::ZERO);
        touch(&app.join("mnt/backup.js"), Duration::ZERO);
        touch(&app.join("node_modules/pkg/index.js"), Duration::ZERO);
        let mut config = Config::default();
        config.scan.exclude = vec!["vendored".to_owned()];
        config.scan.exclude_extensions = vec!["log".to_owned()];
        config.scan.excluded_mounts = vec![app.join("mnt")];
        let mut candidates = [candidate(app.join("node_modules"))];
        annotate(&mut candidates, &config, None).expect("annotate");
        assert_eq!(days_idle(&candidates[0]), Some(50));

        let mut candidates = [candidate(app.join("node_modules"))];
        annotate(&mut candidates, &Config::default(), None).expect("annotate");
        assert_eq!(days_idle(&candidates[0]), Some(0));
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
