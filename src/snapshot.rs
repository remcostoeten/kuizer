use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::caches;
use crate::docker;
use crate::dupes::{self, Duplicates};
use crate::history::History;
use crate::repos::Repository;
use crate::scan::{Candidate, Issue, ScanOptions, SortKey, Summary, sort_candidates};
use crate::toolchains::Toolchain;

const SCHEMA: u32 = 2;
const LIMIT: usize = 12;
const BUDGET: u64 = 48 << 20;
const EXTENSION: &str = "json";
const SIDECAR: &str = "dupes.json";
const TEMP_EXTENSION: &str = "tmp";
const TEMP_STALE: Duration = Duration::from_secs(5 * 60);

/// The last finished scan for each set of folders, stored under the platform state directory.
///
/// @name Snapshots
/// @description Loads and saves scan results and their optional duplicate-search sidecars.
///
/// @example
/// let snapshots = Snapshots::for_user();
/// let shown = snapshots.load(&roots, &options);
#[derive(Debug, Clone)]
pub struct Snapshots {
    dir: Option<PathBuf>,
    writing: Arc<Mutex<()>>,
}

/// Paths deleted while a scan runs, removed from the saved result when it is written.
///
/// @name Forgotten
/// @description Shares deleted paths between the scan worker and its snapshot recorder.
///
/// @example
/// let forgotten = Forgotten::default();
/// forgotten.add(removed_paths);
#[derive(Debug, Clone, Default)]
pub struct Forgotten(Arc<Mutex<HashSet<PathBuf>>>);

impl Forgotten {
    /// @name Forgotten::add
    /// @description Adds removed paths so the in-flight snapshot will not restore them.
    ///
    /// @example
    /// forgotten.add(removed_paths);
    pub fn add(&self, paths: impl IntoIterator<Item = PathBuf>) {
        if let Ok(mut forgotten) = self.0.lock() {
            forgotten.extend(paths);
        }
    }

    fn paths(&self) -> HashSet<PathBuf> {
        self.0
            .lock()
            .map_or_else(|_| HashSet::new(), |paths| paths.clone())
    }
}

/// One saved GUI or terminal scan, including the sections that were enabled for that run.
///
/// @name SavedScan
/// @description Carries a reusable scan report independently of any frontend renderer.
///
/// @example
/// if let Some(saved) = snapshots.load(&roots, &options) { show(saved.candidates); }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedScan {
    schema: u32,
    version: String,
    pub saved: SystemTime,
    pub roots: Vec<PathBuf>,
    pub options: ScanOptions,
    pub candidates: Vec<Candidate>,
    pub issues: Vec<Issue>,
    pub summary: Summary,
    pub caches: Vec<(caches::KnownCache, Candidate)>,
    pub toolchains: Vec<(Toolchain, Candidate)>,
    pub docker: Vec<docker::Item>,
    pub docker_issues: Vec<String>,
    pub repos: Vec<Repository>,
    pub history: Result<SavedHistory, String>,
    pub disks: Vec<(Option<u64>, Option<u64>)>,
}

/// History data saved with a scan, before the frontend applies its current trash listing.
///
/// @name SavedHistory
/// @description Keeps the history record and the trash state that were visible when saved.
///
/// @example
/// let history = SavedHistory { history, kept, trash_known };
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedHistory {
    pub history: History,
    pub kept: HashSet<(usize, usize)>,
    pub trash_known: bool,
}

impl SavedScan {
    /// @name SavedScan::new
    /// @description Starts an empty saved scan for these roots and options.
    ///
    /// @example
    /// let saved = SavedScan::new(&roots, &options);
    pub fn new(roots: &[PathBuf], options: &ScanOptions) -> Self {
        Self {
            schema: SCHEMA,
            version: env!("CARGO_PKG_VERSION").to_owned(),
            saved: SystemTime::now(),
            roots: roots.to_vec(),
            options: options.clone(),
            candidates: Vec::new(),
            issues: Vec::new(),
            summary: Summary {
                progress: Default::default(),
                duration: Duration::ZERO,
                cancelled: false,
            },
            caches: Vec::new(),
            toolchains: Vec::new(),
            docker: Vec::new(),
            docker_issues: Vec::new(),
            repos: Vec::new(),
            history: Err(String::new()),
            disks: Vec::new(),
        }
    }

    /// @name SavedScan::fit
    /// @description Applies the active per-scan options to a stored result.
    ///
    /// @example
    /// saved.fit(&current_options);
    pub fn fit(&mut self, options: &ScanOptions) {
        if !options.caches {
            self.caches.clear();
        }
        if !options.toolchains {
            self.toolchains.clear();
        }
        if !options.docker {
            self.docker.clear();
            self.docker_issues.clear();
        }
        if !options.repos {
            self.repos.clear();
        }
        self.candidates.retain(|candidate| {
            options.categories.contains(&candidate.rule.category)
                && candidate.usage.allocated >= options.min_size
        });
        self.options = options.clone();
        sort_candidates(&mut self.candidates, SortKey::Size);
    }

    /// @name SavedScan::forget
    /// @description Removes deleted filesystem and Docker items from a saved result.
    ///
    /// @example
    /// saved.forget(&removed_paths);
    pub fn forget(&mut self, paths: &HashSet<PathBuf>) -> bool {
        if paths.is_empty() {
            return false;
        }
        let before =
            self.candidates.len() + self.caches.len() + self.toolchains.len() + self.docker.len();
        self.candidates
            .retain(|candidate| !paths.contains(&candidate.path));
        self.caches
            .retain(|(cache, _)| !paths.contains(&cache.path));
        self.toolchains
            .retain(|(_, candidate)| !paths.contains(&candidate.path));
        self.docker.retain(|item| !paths.contains(&item.path()));
        let after =
            self.candidates.len() + self.caches.len() + self.toolchains.len() + self.docker.len();
        after < before
    }
}

/// Captures the parts of a scan as they arrive, then writes the finished result once.
///
/// @name Recorder
/// @description Keeps saved scan data off the frontend event queue.
///
/// @example
/// let recorder = Recorder::new(&roots, &options, forgotten);
/// recorder.record_candidates(&batch);
/// recorder.save(&snapshots, SystemTime::now());
pub struct Recorder {
    saved: Mutex<SavedScan>,
    forgotten: Forgotten,
}

impl Recorder {
    /// @name Recorder::new
    /// @description Creates a recorder for the scan roots, options and deleted-path set.
    ///
    /// @example
    /// let recorder = Recorder::new(&roots, &options, forgotten);
    pub fn new(roots: &[PathBuf], options: &ScanOptions, forgotten: Forgotten) -> Self {
        Self {
            saved: Mutex::new(SavedScan::new(roots, options)),
            forgotten,
        }
    }

    /// @name Recorder::record_candidates
    /// @description Appends one streamed batch of artifact candidates.
    ///
    /// @example
    /// recorder.record_candidates(&batch);
    pub fn record_candidates(&self, candidates: &[Candidate]) {
        if let Ok(mut saved) = self.saved.lock() {
            saved.candidates.extend_from_slice(candidates);
        }
    }

    /// @name Recorder::record_artifacts
    /// @description Replaces the artifact section for a non-streamed scan part.
    ///
    /// @example
    /// recorder.record_artifacts(&candidates, &issues, &summary, &disks);
    pub fn record_artifacts(
        &self,
        candidates: &[Candidate],
        issues: &[Issue],
        summary: &Summary,
        disks: &[(Option<u64>, Option<u64>)],
    ) {
        if let Ok(mut saved) = self.saved.lock() {
            saved.candidates = candidates.to_vec();
            saved.issues = issues.to_vec();
            saved.summary = *summary;
            saved.disks = disks.to_vec();
        }
    }

    /// @name Recorder::record_artifact_metadata
    /// @description Completes the artifact section after streamed candidates have arrived.
    ///
    /// @example
    /// recorder.record_artifact_metadata(&issues, &summary, &disks);
    pub fn record_artifact_metadata(
        &self,
        issues: &[Issue],
        summary: &Summary,
        disks: &[(Option<u64>, Option<u64>)],
    ) {
        if let Ok(mut saved) = self.saved.lock() {
            saved.issues = issues.to_vec();
            saved.summary = *summary;
            saved.disks = disks.to_vec();
        }
    }

    /// @name Recorder::record_caches
    /// @description Replaces the global cache section.
    ///
    /// @example
    /// recorder.record_caches(&caches);
    pub fn record_caches(&self, found: &[(caches::KnownCache, Candidate)]) {
        if let Ok(mut saved) = self.saved.lock() {
            saved.caches = found.to_vec();
        }
    }

    /// @name Recorder::record_toolchains
    /// @description Replaces the installed toolchain section.
    ///
    /// @example
    /// recorder.record_toolchains(&toolchains);
    pub fn record_toolchains(&self, found: &[(Toolchain, Candidate)]) {
        if let Ok(mut saved) = self.saved.lock() {
            saved.toolchains = found.to_vec();
        }
    }

    /// @name Recorder::record_docker
    /// @description Replaces the Docker and Podman section.
    ///
    /// @example
    /// recorder.record_docker(&entries, &issues);
    pub fn record_docker(&self, entries: &[(docker::Item, Candidate)], issues: &[String]) {
        if let Ok(mut saved) = self.saved.lock() {
            saved.docker = entries.iter().map(|(item, _)| item.clone()).collect();
            saved.docker_issues = issues.to_vec();
        }
    }

    /// @name Recorder::record_repositories
    /// @description Replaces the inspected Git repository section.
    ///
    /// @example
    /// recorder.record_repositories(&repositories);
    pub fn record_repositories(&self, repositories: &[Repository]) {
        if let Ok(mut saved) = self.saved.lock() {
            saved.repos = repositories.to_vec();
        }
    }

    /// @name Recorder::record_history
    /// @description Replaces the deletion history section.
    ///
    /// @example
    /// recorder.record_history(Ok(history));
    pub fn record_history(&self, history: Result<SavedHistory, String>) {
        if let Ok(mut saved) = self.saved.lock() {
            saved.history = history;
        }
    }

    /// @name Recorder::save
    /// @description Excludes paths deleted while scanning and persists the completed result.
    ///
    /// @example
    /// recorder.save(&snapshots, SystemTime::now());
    pub fn save(self, snapshots: &Snapshots, scanned: SystemTime) {
        let mut saved = self
            .saved
            .into_inner()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        saved.saved = scanned;
        saved.forget(&self.forgotten.paths());
        snapshots.save(saved);
    }
}

impl Snapshots {
    /// @name Snapshots::for_user
    /// @description Opens the platform state folder used for saved scans.
    ///
    /// @example
    /// let snapshots = Snapshots::for_user();
    pub fn for_user() -> Self {
        Self::new(
            dirs::state_dir()
                .or_else(dirs::data_local_dir)
                .map(|dir| dir.join("kuizer").join("scans")),
        )
    }

    /// @name Snapshots::new
    /// @description Opens a store at `dir`, or disables persistence when it is `None`.
    ///
    /// @example
    /// let snapshots = Snapshots::new(Some(temp.path().join("scans")));
    pub fn new(dir: Option<PathBuf>) -> Self {
        Self {
            dir,
            writing: Arc::default(),
        }
    }

    /// @name Snapshots::load
    /// @description Loads and fits the saved scan for the same root set.
    ///
    /// @example
    /// let saved = snapshots.load(&roots, &options);
    pub fn load(&self, roots: &[PathBuf], options: &ScanOptions) -> Option<SavedScan> {
        let mut saved = self.read_scan(roots)?;
        saved.fit(options);
        Some(saved)
    }

    /// @name Snapshots::save
    /// @description Atomically saves the completed scan and prunes older scans within the store
    /// budget.
    ///
    /// @example
    /// snapshots.save(saved);
    pub fn save(&self, mut saved: SavedScan) {
        let (Some(dir), Some(file)) = (&self.dir, self.file(&saved.roots, EXTENSION)) else {
            return;
        };
        saved.saved = SystemTime::now();
        sort_candidates(&mut saved.candidates, SortKey::Size);
        let _writing = self.writing.lock();
        if let Err(error) = write(dir, &file, &saved) {
            eprintln!("kuizer: cannot save the scan for next time: {error}");
            return;
        }
        prune(dir, &file);
    }

    /// @name Snapshots::forget
    /// @description Removes deleted items from a saved scan so they do not return next time.
    ///
    /// @example
    /// snapshots.forget(&roots, &removed_paths);
    pub fn forget(&self, roots: &[PathBuf], paths: &HashSet<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        let Some(file) = self.file(roots, EXTENSION) else {
            return;
        };
        let _writing = self.writing.lock();
        let Some(mut saved) = self.read_scan(roots) else {
            return;
        };
        if saved.forget(paths) {
            self.write_unlocked(&file, &saved);
        }
    }

    /// @name Snapshots::load_duplicates
    /// @description Loads duplicate groups when their roots and options still match.
    ///
    /// @example
    /// let groups = snapshots.load_duplicates(&roots, options);
    pub fn load_duplicates(
        &self,
        roots: &[PathBuf],
        options: dupes::Options,
    ) -> Option<Duplicates> {
        let bytes = std::fs::read(self.file(roots, SIDECAR)?).ok()?;
        let sidecar: Sidecar = serde_json::from_slice(&bytes).ok()?;
        (sidecar.schema == SCHEMA
            && sidecar.version == env!("CARGO_PKG_VERSION")
            && sidecar.roots == roots
            && sidecar.min_size == options.min_size
            && sidecar.verify == options.verify)
            .then_some(sidecar.duplicates)
    }

    /// @name Snapshots::stage_duplicates
    /// @description Prepares duplicate groups for atomic persistence next to their scan.
    ///
    /// @example
    /// snapshots.stage_duplicates(&roots, options, &duplicates).save();
    pub fn stage_duplicates(
        &self,
        roots: &[PathBuf],
        options: dupes::Options,
        duplicates: &Duplicates,
    ) -> Staged {
        Staged {
            snapshots: self.clone(),
            sidecar: Sidecar {
                schema: SCHEMA,
                version: env!("CARGO_PKG_VERSION").to_owned(),
                roots: roots.to_vec(),
                min_size: options.min_size,
                verify: options.verify,
                duplicates: duplicates.clone(),
            },
        }
    }

    fn read_scan(&self, roots: &[PathBuf]) -> Option<SavedScan> {
        let bytes = std::fs::read(self.file(roots, EXTENSION)?).ok()?;
        let saved: SavedScan = serde_json::from_slice(&bytes).ok()?;
        (saved.schema == SCHEMA
            && saved.version == env!("CARGO_PKG_VERSION")
            && saved.roots == roots)
            .then_some(saved)
    }

    fn file(&self, roots: &[PathBuf], extension: &str) -> Option<PathBuf> {
        let key = serde_json::to_vec(roots).ok()?;
        let dir = self.dir.as_ref()?;
        Some(dir.join(format!("{:016x}.{extension}", fnv(&key))))
    }

    fn write_unlocked(&self, file: &Path, saved: &SavedScan) {
        let Some(dir) = &self.dir else {
            return;
        };
        if let Err(error) = write(dir, file, saved) {
            eprintln!("kuizer: cannot save the scan for next time: {error}");
            return;
        }
        prune(dir, file);
    }
}

/// A duplicate sidecar ready to be saved beside its scan.
pub struct Staged {
    snapshots: Snapshots,
    sidecar: Sidecar,
}

impl Staged {
    /// @name Staged::save
    /// @description Atomically saves the staged duplicate groups.
    ///
    /// @example
    /// snapshots.stage_duplicates(&roots, options, &duplicates).save();
    pub fn save(self) {
        let (Some(dir), Some(file)) = (
            &self.snapshots.dir,
            self.snapshots.file(&self.sidecar.roots, SIDECAR),
        ) else {
            return;
        };
        let _writing = self.snapshots.writing.lock();
        if let Err(error) = write(dir, &file, &self.sidecar) {
            eprintln!("kuizer: cannot save duplicate groups for next time: {error}");
            return;
        }
        prune(dir, &file);
    }
}

#[derive(Serialize, Deserialize)]
struct Sidecar {
    schema: u32,
    version: String,
    roots: Vec<PathBuf>,
    min_size: u64,
    verify: bool,
    duplicates: Duplicates,
}

/// @name age_label
/// @description Describes when the saved scan was last updated.
///
/// @example
/// println!("{}", age_label(saved.saved, SystemTime::now()));
pub fn age_label(saved: SystemTime, now: SystemTime) -> String {
    let seconds = now
        .duration_since(saved)
        .map_or(0, |elapsed| elapsed.as_secs());
    let (count, noun) = match seconds {
        0..60 => return "From just now".to_owned(),
        60..3_600 => (seconds / 60, "minute"),
        3_600..86_400 => (seconds / 3_600, "hour"),
        _ => (seconds / 86_400, "day"),
    };
    let plural = if count == 1 { "" } else { "s" };
    format!("From {count} {noun}{plural} ago")
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn write(dir: &Path, file: &Path, saved: &impl Serialize) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let bytes = serde_json::to_vec(saved)?;
    crate::platform::write_atomic(file, &bytes)
}

#[derive(Clone, Copy)]
enum SavedKind {
    Scan,
    Sidecar,
    Temp,
}

struct SavedFile {
    path: PathBuf,
    modified: SystemTime,
    bytes: u64,
}

#[derive(Default)]
struct SavedPair {
    scan: Option<SavedFile>,
    sidecar: Option<SavedFile>,
}

impl SavedPair {
    fn files(&self) -> impl Iterator<Item = &SavedFile> {
        self.sidecar.iter().chain(&self.scan)
    }

    fn modified(&self) -> SystemTime {
        self.files()
            .map(|file| file.modified)
            .max()
            .unwrap_or(SystemTime::UNIX_EPOCH)
    }

    fn bytes(&self) -> u64 {
        self.files().map(|file| file.bytes).sum()
    }

    fn remove(&self) {
        for file in self.files() {
            let _ = std::fs::remove_file(&file.path);
        }
    }
}

fn all_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

fn saved_name(name: &str) -> Option<(u64, SavedKind)> {
    let (key, rest) = name.split_at_checked(16)?;
    let is_hex = |byte: u8| matches!(byte, b'0'..=b'9' | b'a'..=b'f');
    if !key.bytes().all(is_hex) {
        return None;
    }
    let key = u64::from_str_radix(key, 16).ok()?;
    let rest = rest.strip_prefix('.')?;
    if rest == EXTENSION {
        return Some((key, SavedKind::Scan));
    }
    if rest == SIDECAR {
        return Some((key, SavedKind::Sidecar));
    }
    let counters = rest.strip_suffix(TEMP_EXTENSION)?.strip_suffix('.')?;
    let counters = SIDECAR
        .strip_suffix(EXTENSION)
        .and_then(|stem| counters.strip_prefix(stem))
        .unwrap_or(counters);
    let (process, write) = counters.split_once('.')?;
    (all_digits(process) && all_digits(write)).then_some((key, SavedKind::Temp))
}

fn is_stale(modified: SystemTime, now: SystemTime) -> bool {
    now.duration_since(modified)
        .is_ok_and(|age| age > TEMP_STALE)
}

fn prune(dir: &Path, keep: &Path) {
    let Some((kept, _)) = keep
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(saved_name)
    else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let now = SystemTime::now();
    let mut pairs: HashMap<u64, SavedPair> = HashMap::new();
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some((key, kind)) = saved_name(name) else {
            continue;
        };
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let file = SavedFile {
            path,
            modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            bytes: metadata.len(),
        };
        let pair = pairs.entry(key).or_default();
        match kind {
            SavedKind::Scan => pair.scan = Some(file),
            SavedKind::Sidecar => pair.sidecar = Some(file),
            SavedKind::Temp if is_stale(file.modified, now) => {
                let _ = std::fs::remove_file(file.path);
            }
            SavedKind::Temp => {}
        }
    }
    let mut saved: Vec<_> = pairs.into_iter().collect();
    saved.sort_by_key(|(_, pair)| std::cmp::Reverse(pair.modified()));
    let mut count = 0usize;
    let mut bytes = 0u64;
    for (key, pair) in saved {
        let pair_bytes = pair.bytes();
        if key == kept || (count < LIMIT && bytes.saturating_add(pair_bytes) <= BUDGET) {
            count += usize::from(pair.scan.is_some());
            bytes = bytes.saturating_add(pair_bytes);
        } else {
            pair.remove();
        }
    }
}
