use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::delete::{DeleteError, Guard, Mode};
use crate::detect::{Category, Kind, RuleInfo, Safety};
use crate::scan::{Candidate, Usage};
use crate::units::ByteSize;

const REMOVE_TIMEOUT: Duration = Duration::from_secs(300);
const INSPECT_TIMEOUT: Duration = Duration::from_secs(30);
const TEXT_BUSY: i32 = 26;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Object {
    Image,
    Container,
    BuildCache,
    Volume,
}

impl Object {
    pub const ALL: [Object; 4] = [Self::Image, Self::Container, Self::BuildCache, Self::Volume];

    pub fn name(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Container => "container",
            Self::BuildCache => "build-cache",
            Self::Volume => "volume",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|object| object.name() == name)
    }

    fn category(self) -> Category {
        match self {
            Self::Image => Category::Build,
            Self::Container => Category::Temp,
            Self::BuildCache | Self::Volume => Category::Cache,
        }
    }
}

/// One Docker or Podman object kuizer can remove through the engine's own CLI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub engine: String,
    pub program: String,
    pub object: Object,
    pub id: String,
    pub label: String,
    pub bytes: u64,
    pub safety: Safety,
    pub note: String,
}

impl Item {
    /// The pseudo path that names this object, such as `docker:image/3f2a…`.
    pub fn path(&self) -> PathBuf {
        PathBuf::from(format!(
            "{}:{}/{}",
            self.program,
            self.object.name(),
            self.id
        ))
    }

    pub fn candidate(&self, id: u64) -> Candidate {
        Candidate {
            id,
            path: self.path(),
            kind: Kind::External,
            rule: Arc::new(RuleInfo {
                name: self.object.name().to_owned(),
                category: self.object.category(),
                safety: self.safety,
                ecosystem: self.engine.clone(),
            }),
            project: None,
            usage: Usage {
                allocated: self.bytes,
                apparent: self.bytes,
                entries: 1,
                partial: false,
            },
            newest: None,
            identity: None,
            project_last_active: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    pub items: Vec<Item>,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Failure {
    Missing,
    TimedOut,
    Failed(String),
}

fn spawn(program: &str, args: &[&str]) -> std::io::Result<std::process::Child> {
    let mut attempts = 0;
    loop {
        let result = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        match result {
            Err(error) if error.raw_os_error() == Some(TEXT_BUSY) && attempts < 10 => {
                attempts += 1;
                thread::sleep(Duration::from_millis(20));
            }
            other => return other,
        }
    }
}

fn drain(pipe: Option<impl Read + Send + 'static>) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_string(&mut text);
        }
        text
    })
}

fn run(program: &str, args: &[&str], timeout: Duration) -> Result<String, Failure> {
    let mut child = spawn(program, args).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Failure::Missing
        } else {
            Failure::Failed(error.to_string())
        }
    })?;
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Failure::TimedOut);
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(error) => return Err(Failure::Failed(error.to_string())),
        }
    };
    let out = stdout.join().unwrap_or_default();
    let err = stderr.join().unwrap_or_default();
    if status.success() {
        Ok(out)
    } else {
        Err(Failure::Failed(
            err.lines()
                .map(str::trim)
                .rfind(|line| !line.is_empty())
                .unwrap_or("no output")
                .to_owned(),
        ))
    }
}

fn engine_name(program: &str) -> &'static str {
    let file = Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program);
    if file.contains("podman") {
        "podman"
    } else {
        "docker"
    }
}

fn size(text: &str) -> u64 {
    let first = text.split(" (").next().unwrap_or(text).trim();
    first.parse::<ByteSize>().map_or(0, |size| size.0)
}

fn short(id: &str) -> String {
    id.trim_start_matches("sha256:").chars().take(12).collect()
}

/// @name hours_ago
/// @description Reads how long ago a container stopped from the status Docker and Podman print,
/// such as `Exited (0) 2 hours ago` or `Exited (1) About a minute ago`. Returns `None` when the
/// status names no time, as for a container that was created but never started.
///
/// @example
/// assert_eq!(docker::hours_ago("Exited (0) 3 days ago"), Some(72));
pub fn hours_ago(status: &str) -> Option<u64> {
    let lowered = status.to_ascii_lowercase();
    let before = lowered.strip_suffix(" ago")?;
    let words: Vec<&str> = before.split_whitespace().collect();
    let unit = words.last()?.trim_end_matches('s');
    let count = match words.get(words.len().checked_sub(2)?)? {
        &"a" | &"an" | &"one" => 1,
        number => number.parse::<u64>().ok()?,
    };
    let hours = match unit {
        "second" | "minute" => 0,
        "hour" => count,
        "day" => count * 24,
        "week" => count * 24 * 7,
        "month" => count * 24 * 30,
        "year" => count * 24 * 365,
        _ => return None,
    };
    Some(hours)
}

fn fields(line: &str) -> Vec<&str> {
    line.split('\t').map(str::trim).collect()
}

fn images(program: &str, engine: &str, timeout: Duration) -> Result<Vec<Item>, Failure> {
    let out = run(
        program,
        &[
            "image",
            "ls",
            "--filter",
            "dangling=true",
            "--no-trunc",
            "--format",
            "{{.ID}}\t{{.Size}}\t{{.CreatedSince}}",
        ],
        timeout,
    )?;
    Ok(out
        .lines()
        .filter_map(|line| {
            let parts = fields(line);
            let id = parts.first().filter(|id| !id.is_empty())?;
            Some(Item {
                engine: engine.to_owned(),
                program: program.to_owned(),
                object: Object::Image,
                id: id.trim_start_matches("sha256:").to_owned(),
                label: format!("<none> {}", short(id)),
                bytes: parts.get(1).map_or(0, |text| size(text)),
                safety: Safety::Safe,
                note: parts
                    .get(2)
                    .map(|created| format!("dangling, created {created}"))
                    .unwrap_or_default(),
            })
        })
        .collect())
}

fn containers(
    program: &str,
    engine: &str,
    timeout: Duration,
    recent_hours: u64,
) -> Result<Vec<Item>, Failure> {
    let out = run(
        program,
        &[
            "container",
            "ls",
            "--all",
            "--size",
            "--no-trunc",
            "--filter",
            "status=exited",
            "--filter",
            "status=created",
            "--format",
            "{{.ID}}\t{{.Names}}\t{{.Image}}\t{{.Size}}\t{{.Status}}",
        ],
        timeout,
    )?;
    Ok(out
        .lines()
        .filter_map(|line| {
            let parts = fields(line);
            let id = parts.first().filter(|id| !id.is_empty())?;
            let status = parts.get(4).copied().unwrap_or_default();
            let recent = hours_ago(status).is_some_and(|hours| hours < recent_hours);
            Some(Item {
                engine: engine.to_owned(),
                program: program.to_owned(),
                object: Object::Container,
                id: (*id).to_owned(),
                label: parts.get(1).copied().unwrap_or(id).to_owned(),
                bytes: parts.get(3).map_or(0, |text| size(text)),
                safety: if recent { Safety::Review } else { Safety::Safe },
                note: format!(
                    "{}, from {}",
                    status,
                    parts.get(2).copied().unwrap_or("an unknown image")
                ),
            })
        })
        .collect())
}

fn cache_item(program: &str, engine: &str, bytes: u64) -> Option<Item> {
    (bytes > 0).then(|| Item {
        engine: engine.to_owned(),
        program: program.to_owned(),
        object: Object::BuildCache,
        id: "all".to_owned(),
        label: "build cache".to_owned(),
        bytes,
        safety: Safety::Safe,
        note: "layers no image uses; the next build makes them again".to_owned(),
    })
}

#[derive(Deserialize)]
struct CacheRecord {
    #[serde(rename = "Reclaimable", default)]
    reclaimable: bool,
    #[serde(rename = "Shared", default)]
    shared: bool,
    #[serde(rename = "Size", default)]
    size: String,
}

fn builder_cache_bytes(program: &str, timeout: Duration) -> Result<u64, Failure> {
    let out = run(program, &["builder", "du", "--format", "json"], timeout)?;
    let mut total = 0;
    for line in out.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let record: CacheRecord =
            serde_json::from_str(line).map_err(|error| Failure::Failed(error.to_string()))?;
        if record.reclaimable && !record.shared {
            total += size(&record.size);
        }
    }
    Ok(total)
}

fn system_cache_bytes(program: &str, timeout: Duration) -> Result<u64, Failure> {
    let out = run(
        program,
        &["system", "df", "--format", "{{.Type}}\t{{.Reclaimable}}"],
        timeout,
    )?;
    Ok(out
        .lines()
        .map(fields)
        .filter(|parts| parts.first() == Some(&"Build Cache"))
        .filter_map(|parts| parts.get(1).map(|text| size(text)))
        .sum())
}

fn build_cache(program: &str, engine: &str, timeout: Duration) -> Result<Vec<Item>, Failure> {
    let bytes = match builder_cache_bytes(program, timeout) {
        Ok(bytes) => bytes,
        Err(Failure::TimedOut) => return Err(Failure::TimedOut),
        Err(_) => system_cache_bytes(program, timeout)?,
    };
    Ok(cache_item(program, engine, bytes).into_iter().collect())
}

#[derive(Deserialize)]
struct VolumeSize {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Size")]
    size: String,
}

fn volume_sizes(program: &str, timeout: Duration) -> Vec<VolumeSize> {
    run(
        program,
        &["system", "df", "--verbose", "--format", "{{json .Volumes}}"],
        timeout,
    )
    .ok()
    .and_then(|out| serde_json::from_str(out.trim()).ok())
    .unwrap_or_default()
}

fn volume_item(
    program: &str,
    engine: &str,
    name: &str,
    bytes: Option<u64>,
    measuring: bool,
) -> Item {
    Item {
        engine: engine.to_owned(),
        program: program.to_owned(),
        object: Object::Volume,
        id: name.to_owned(),
        label: if name.len() > 24 {
            short(name)
        } else {
            name.to_owned()
        },
        bytes: bytes.unwrap_or(0),
        safety: Safety::Review,
        note: volume_note(bytes.is_some(), measuring),
    }
}

fn volume_note(known: bool, measuring: bool) -> String {
    match (known, measuring) {
        (true, _) => "no container uses it, but it may hold data such as a database",
        (false, true) => "no container uses it, but it may hold data; measuring its size",
        (false, false) => "no container uses it, but it may hold data; size unknown",
    }
    .to_owned()
}

fn volumes(
    program: &str,
    engine: &str,
    timeout: Duration,
) -> Result<(Vec<Item>, Option<Pending>), Failure> {
    let out = run(
        program,
        &[
            "volume",
            "ls",
            "--filter",
            "dangling=true",
            "--format",
            "{{.Name}}",
        ],
        timeout,
    )?;
    let names: Vec<String> = out
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect();
    if names.is_empty() {
        return Ok((Vec::new(), None));
    }
    let items = names
        .iter()
        .map(|name| volume_item(program, engine, name, None, true))
        .collect();
    let (sender, receiver) = mpsc::channel();
    let owned = program.to_owned();
    thread::spawn(move || {
        let _ = sender.send(volume_sizes(&owned, timeout));
    });
    let pending = Pending {
        program: program.to_owned(),
        engine: engine.to_owned(),
        names,
        receiver,
    };
    Ok((items, Some(pending)))
}

struct Pending {
    program: String,
    engine: String,
    names: Vec<String>,
    receiver: Receiver<Vec<VolumeSize>>,
}

impl Pending {
    fn updates(&self, sizes: &[VolumeSize]) -> Vec<SizeUpdate> {
        self.names
            .iter()
            .map(|name| SizeUpdate {
                program: self.program.clone(),
                engine: self.engine.clone(),
                id: name.clone(),
                bytes: sizes
                    .iter()
                    .find(|volume| volume.name == *name)
                    .map(|volume| size(&volume.size)),
            })
            .collect()
    }
}

/// The measured size of one volume that was listed without it. `bytes` is `None` when the engine
/// did not report the volume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SizeUpdate {
    pub program: String,
    pub engine: String,
    pub id: String,
    pub bytes: Option<u64>,
}

/// Volume sizes still being measured after `list_fast` returned. Measuring takes seconds on a large
/// engine, so it runs on its own thread; poll for the result or block for it on a worker thread.
#[derive(Default)]
pub struct Sizing {
    pending: Vec<Pending>,
}

impl Sizing {
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// @name wait
    /// @description Blocks until every engine has reported its volume sizes (or failed or timed out,
    /// which leaves the sizes unknown). Call it from a worker thread, then pass the result to
    /// `Listing::apply`.
    ///
    /// @example
    /// let updates = sizing.wait();
    /// listing.apply(&updates);
    pub fn wait(self) -> Vec<SizeUpdate> {
        self.pending
            .iter()
            .flat_map(|pending| {
                let sizes = pending.receiver.recv().unwrap_or_default();
                pending.updates(&sizes)
            })
            .collect()
    }

    /// @name poll
    /// @description Collects the sizes of the engines that have finished measuring without blocking.
    /// Returns `None` while any engine is still measuring, so the sizes arrive in one batch.
    ///
    /// @example
    /// if let Some(updates) = sizing.poll() {
    ///     listing.apply(&updates);
    /// }
    pub fn poll(&mut self) -> Option<Vec<SizeUpdate>> {
        let mut ready = Vec::new();
        for pending in &self.pending {
            match pending.receiver.try_recv() {
                Ok(sizes) => ready.push(pending.updates(&sizes)),
                Err(TryRecvError::Disconnected) => ready.push(pending.updates(&[])),
                Err(TryRecvError::Empty) => return None,
            }
        }
        self.pending.clear();
        Some(ready.into_iter().flatten().collect())
    }
}

fn describe(engine: &str, failure: &Failure) -> String {
    match failure {
        Failure::Missing => format!("{engine} is not installed"),
        Failure::TimedOut => format!("{engine} did not answer in time"),
        Failure::Failed(message) => format!("{engine} is not running: {message}"),
    }
}

fn joined<T>(handle: thread::ScopedJoinHandle<'_, Result<T, Failure>>) -> Result<T, Failure> {
    handle.join().unwrap_or_else(|_| {
        Err(Failure::Failed(
            "the engine call stopped unexpectedly".to_owned(),
        ))
    })
}

fn engine_items(
    program: &str,
    engine: &str,
    timeout: Duration,
    recent_hours: u64,
) -> Result<(Vec<Item>, Option<Pending>), Failure> {
    let (dangling, stopped, cache, unused) = thread::scope(|scope| {
        let dangling = scope.spawn(|| images(program, engine, timeout));
        let stopped = scope.spawn(|| containers(program, engine, timeout, recent_hours));
        let cache =
            (engine == "docker").then(|| scope.spawn(|| build_cache(program, engine, timeout)));
        let unused = volumes(program, engine, timeout);
        (joined(dangling), joined(stopped), cache.map(joined), unused)
    });
    let mut items = dangling?;
    items.extend(stopped?);
    if let Some(cache) = cache {
        items.extend(cache.unwrap_or_default());
    }
    let (volumes, pending) = unused?;
    items.extend(volumes);
    Ok((items, pending))
}

fn sort_items(items: &mut [Item]) {
    items.sort_by(|a, b| {
        b.bytes
            .cmp(&a.bytes)
            .then_with(|| a.object.cmp(&b.object))
            .then_with(|| a.id.cmp(&b.id))
    });
}

impl Listing {
    /// @name apply
    /// @description Fills in volume sizes that `list_fast` left unknown and sorts the items again.
    ///
    /// @example
    /// listing.apply(&sizing.wait());
    pub fn apply(&mut self, updates: &[SizeUpdate]) {
        for item in &mut self.items {
            if item.object != Object::Volume {
                continue;
            }
            let update = updates
                .iter()
                .find(|update| update.program == item.program && update.id == item.id);
            if let Some(update) = update {
                item.bytes = update.bytes.unwrap_or(0);
                item.note = volume_note(update.bytes.is_some(), false);
            }
        }
        sort_items(&mut self.items);
    }
}

/// @name list_fast
/// @description Like `list`, but returns as soon as everything except volume sizes is known.
/// Unused volumes come back with size 0 and a note saying the size is being measured; the
/// returned `Sizing` delivers the real sizes later, and `Listing::apply` patches them in.
///
/// @example
/// let (mut listing, sizing) = docker::list_fast(&config.docker.engines, Duration::from_secs(10), 24);
/// listing.apply(&sizing.wait());
pub fn list_fast(engines: &[String], timeout: Duration, recent_hours: u64) -> (Listing, Sizing) {
    let mut listing = Listing::default();
    let mut sizing = Sizing::default();
    let mut missing = Vec::new();
    for program in engines {
        let engine = engine_name(program);
        match engine_items(program, engine, timeout, recent_hours) {
            Ok((items, pending)) => {
                listing.items.extend(items);
                sizing.pending.extend(pending);
            }
            Err(Failure::Missing) => missing.push(engine),
            Err(failure) => listing.issues.push(describe(engine, &failure)),
        }
    }
    if !engines.is_empty() && missing.len() == engines.len() {
        listing.issues.push(match missing.as_slice() {
            [only] => format!("{only} is not installed"),
            _ => "neither docker nor podman is installed".to_owned(),
        });
    }
    sort_items(&mut listing.items);
    (listing, sizing)
}

/// @name list
/// @description Asks each engine (`docker`, `podman` or a path to either) for dangling images,
/// stopped containers, its build cache and volumes no container uses. Only the engines' own CLIs
/// are run, so `DOCKER_HOST` and contexts apply. An engine's calls run side by side, and each stops
/// after `timeout`. An engine that is not running gives one issue and no items; when no engine is
/// installed at all, the listing has one issue saying so. Waits for volume sizes; use `list_fast`
/// to get them later.
///
/// @example
/// let listing = docker::list(&config.docker.engines, Duration::from_secs(10), 24);
pub fn list(engines: &[String], timeout: Duration, recent_hours: u64) -> Listing {
    let (mut listing, sizing) = list_fast(engines, timeout, recent_hours);
    listing.apply(&sizing.wait());
    listing
}

/// @name entries
/// @description Pairs every listed item with a removal candidate, ids starting at 1.
///
/// @example
/// let entries = docker::entries(listing.items);
pub fn entries(items: Vec<Item>) -> Vec<(Item, Candidate)> {
    items
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            let candidate = item.candidate(index as u64 + 1);
            (item, candidate)
        })
        .collect()
}

fn parse_path(path: &Path) -> Option<(String, Object, String)> {
    let text = path.to_str()?;
    let (program, rest) = text.split_once(':')?;
    let (object, id) = rest.split_once('/')?;
    let object = Object::from_name(object)?;
    (!program.is_empty() && !id.is_empty() && !id.starts_with('-'))
        .then(|| (program.to_owned(), object, id.to_owned()))
}

fn still_untagged(program: &str, id: &str) -> Result<bool, Failure> {
    let out = run(
        program,
        &["image", "inspect", "--format", "{{len .RepoTags}}", id],
        INSPECT_TIMEOUT,
    )?;
    Ok(out.trim() == "0")
}

fn refusal(engine: &str, failure: Failure) -> DeleteError {
    DeleteError::Tool(match failure {
        Failure::Failed(message) => format!("{engine} refused: {message}"),
        other => describe(engine, &other),
    })
}

/// @name remove
/// @description Removes one object through its engine: `image rm`, `container rm`,
/// `builder prune --force` or `volume rm`, never with force on an image, container or volume, so
/// anything still in use is refused by the engine itself. An image is inspected right before it
/// is removed and left alone once it carries a tag. Nothing under `/var/lib/docker` is touched.
/// These removals are permanent; a dry run does nothing.
///
/// @example
/// docker::remove(&guard, &candidate, Mode::Permanent)?;
pub fn remove(_guard: &Guard, candidate: &Candidate, mode: Mode) -> Result<(), DeleteError> {
    let (program, object, id) = parse_path(&candidate.path).ok_or_else(|| {
        DeleteError::Tool(format!(
            "{} is not a Docker or Podman object",
            candidate.path.display()
        ))
    })?;
    if mode == Mode::DryRun {
        return Ok(());
    }
    let engine = engine_name(&program);
    if object == Object::Image && !still_untagged(&program, &id).map_err(|f| refusal(engine, f))? {
        return Err(DeleteError::Tool(format!(
            "{engine} image {} is tagged now, not removed",
            short(&id)
        )));
    }
    let args: Vec<&str> = match object {
        Object::Image => vec!["image", "rm", &id],
        Object::Container => vec!["container", "rm", &id],
        Object::BuildCache => vec!["builder", "prune", "--force"],
        Object::Volume => vec!["volume", "rm", &id],
    };
    run(&program, &args, REMOVE_TIMEOUT)
        .map(drop)
        .map_err(|failure| refusal(engine, failure))
}

#[cfg(all(test, unix))]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    const TIMEOUT: Duration = Duration::from_secs(10);

    fn script(dir: &Path, name: &str, body: &str) -> String {
        let path = dir.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("script");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod");
        path.display().to_string()
    }

    fn fake_docker(dir: &Path) -> String {
        script(
            dir,
            "docker",
            r#"log="$(dirname "$0")/calls"
echo "$*" >> "$log"
case "$1 $2" in
  "image ls") printf 'sha256:aaaaaaaaaaaaaaaa\t1.5GB\t3 weeks ago\nsha256:bbbbbbbbbbbbbbbb\t200MB\t2 days ago\n' ;;
  "image inspect") cat "$(dirname "$0")/tags" 2>/dev/null || echo 0 ;;
  "container ls") printf 'c1\told_api\tapi:1\t12.5MB (virtual 900MB)\tExited (0) 3 days ago\nc2\tfresh\tweb:2\t1kB (virtual 10MB)\tExited (1) 2 hours ago\nc3\tnever\tweb:2\t0B (virtual 10MB)\tCreated\n' ;;
  "system df")
    if [ "$3" = "--verbose" ]; then
      echo '[{"Name":"pgdata","Size":"300MB","Links":0}]'
    else
      printf 'Images\t2.1GB (90%%)\nContainers\t12.5MB (100%%)\nLocal Volumes\t300MB (100%%)\nBuild Cache\t4.2GB\n'
    fi ;;
  "volume ls") echo pgdata ;;
  "builder du") printf '{"Reclaimable":true,"Shared":false,"Size":"4.1GB"}\n{"Reclaimable":true,"Shared":false,"Size":"100MB"}\n{"Reclaimable":true,"Shared":true,"Size":"9GB"}\n{"Reclaimable":false,"Shared":false,"Size":"7GB"}\n' ;;
  *) exit 0 ;;
esac"#,
        )
    }

    #[test]
    fn lists_every_object_kind() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let program = fake_docker(tmp.path());
        let listing = list(std::slice::from_ref(&program), TIMEOUT, 24);
        assert!(listing.issues.is_empty(), "{:?}", listing.issues);
        let rows: Vec<(Object, &str, u64, Safety)> = listing
            .items
            .iter()
            .map(|item| (item.object, item.id.as_str(), item.bytes, item.safety))
            .collect();
        assert_eq!(
            rows,
            [
                (Object::BuildCache, "all", 4_200_000_000, Safety::Safe),
                (
                    Object::Image,
                    "aaaaaaaaaaaaaaaa",
                    1_500_000_000,
                    Safety::Safe
                ),
                (Object::Volume, "pgdata", 300_000_000, Safety::Review),
                (Object::Image, "bbbbbbbbbbbbbbbb", 200_000_000, Safety::Safe),
                (Object::Container, "c1", 12_500_000, Safety::Safe),
                (Object::Container, "c2", 1_000, Safety::Review),
                (Object::Container, "c3", 0, Safety::Safe),
            ]
        );
        let entries = entries(listing.items);
        let (item, candidate) = &entries[1];
        assert_eq!(candidate.kind, Kind::External);
        assert_eq!(candidate.rule.name, "image");
        assert_eq!(candidate.rule.ecosystem, "docker");
        assert_eq!(
            candidate.path,
            PathBuf::from(format!("{program}:image/aaaaaaaaaaaaaaaa"))
        );
        assert_eq!(item.label, "<none> aaaaaaaaaaaa");
    }

    #[test]
    fn removes_through_the_engine_without_force() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let program = fake_docker(tmp.path());
        let entries = entries(list(&[program], TIMEOUT, 24).items);
        let guard = Guard::new(&[]);
        for (_, candidate) in &entries {
            remove(&guard, candidate, Mode::DryRun).expect("dry run");
        }
        let before = fs::read_to_string(tmp.path().join("calls")).expect("calls");
        for (_, candidate) in &entries {
            remove(&guard, candidate, Mode::Permanent).expect("removed");
        }
        let calls = fs::read_to_string(tmp.path().join("calls")).expect("calls");
        let removals: Vec<&str> = calls[before.len()..].lines().collect();
        assert_eq!(
            removals,
            [
                "builder prune --force",
                "image inspect --format {{len .RepoTags}} aaaaaaaaaaaaaaaa",
                "image rm aaaaaaaaaaaaaaaa",
                "volume rm pgdata",
                "image inspect --format {{len .RepoTags}} bbbbbbbbbbbbbbbb",
                "image rm bbbbbbbbbbbbbbbb",
                "container rm c1",
                "container rm c2",
                "container rm c3",
            ]
        );
    }

    #[test]
    fn an_image_tagged_after_listing_is_not_removed() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let program = fake_docker(tmp.path());
        let entries = entries(list(&[program], TIMEOUT, 24).items);
        let (_, image) = entries
            .iter()
            .find(|(item, _)| item.id == "aaaaaaaaaaaaaaaa")
            .expect("image");
        fs::write(tmp.path().join("tags"), "1\n").expect("tags");

        let error = remove(&Guard::new(&[]), image, Mode::Permanent).unwrap_err();

        assert!(error.to_string().contains("is tagged now"), "{error}");
        let calls = fs::read_to_string(tmp.path().join("calls")).expect("calls");
        assert!(!calls.contains("image rm"), "{calls}");
    }

    #[test]
    fn a_stopped_daemon_is_one_issue() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let program = script(
            tmp.path(),
            "docker",
            "echo 'Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?' >&2\nexit 1",
        );
        let listing = list(&[program], TIMEOUT, 24);
        assert!(listing.items.is_empty());
        assert_eq!(listing.issues.len(), 1);
        assert!(
            listing.issues[0].starts_with("docker is not running: Cannot connect"),
            "{}",
            listing.issues[0]
        );
    }

    #[test]
    fn missing_engines_are_one_issue() {
        let listing = list(
            &[
                "/nonexistent/kuizer/docker".to_owned(),
                "/nonexistent/kuizer/podman".to_owned(),
            ],
            TIMEOUT,
            24,
        );
        assert!(listing.items.is_empty());
        assert_eq!(listing.issues, ["neither docker nor podman is installed"]);
    }

    #[test]
    fn a_hanging_engine_times_out() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let program = script(tmp.path(), "podman", "sleep 5");
        let started = Instant::now();
        let listing = list(&[program], Duration::from_millis(200), 24);
        assert!(started.elapsed() < Duration::from_secs(3));
        assert_eq!(listing.issues, ["podman did not answer in time"]);
    }

    #[test]
    fn one_engine_answers_its_calls_side_by_side() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let program = script(
            tmp.path(),
            "docker",
            r#"sleep 1
case "$1 $2" in
  "volume ls") echo pgdata ;;
  "system df") if [ "$3" = "--verbose" ]; then echo '[{"Name":"pgdata","Size":"300MB","Links":0}]'; fi ;;
esac"#,
        );
        let started = Instant::now();
        let listing = list(&[program], TIMEOUT, 24);
        assert!(listing.issues.is_empty(), "{:?}", listing.issues);
        assert_eq!(listing.items.len(), 1);
        assert!(started.elapsed() < Duration::from_millis(3500));
    }

    #[test]
    fn volume_sizes_arrive_after_the_fast_listing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let program = script(
            tmp.path(),
            "docker",
            r#"case "$1 $2" in
  "volume ls") echo pgdata ;;
  "system df") sleep 2; echo '[{"Name":"pgdata","Size":"300MB","Links":0}]' ;;
  "container ls") printf 'c1\told\tapi:1\t1MB\tExited (0) 3 days ago\n' ;;
esac"#,
        );
        let started = Instant::now();
        let (mut listing, sizing) = list_fast(&[program], TIMEOUT, 24);
        assert!(started.elapsed() < Duration::from_millis(1500));
        let volume = listing
            .items
            .iter()
            .find(|item| item.object == Object::Volume);
        assert_eq!(volume.map(|item| item.bytes), Some(0));
        assert!(!sizing.is_empty());
        listing.apply(&sizing.wait());
        let rows: Vec<(Object, u64)> = listing
            .items
            .iter()
            .map(|item| (item.object, item.bytes))
            .collect();
        assert_eq!(
            rows,
            [
                (Object::Volume, 300_000_000),
                (Object::Container, 1_000_000)
            ]
        );
    }

    #[test]
    fn build_cache_falls_back_to_system_df() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let program = script(
            tmp.path(),
            "docker",
            r#"case "$1 $2" in
  "builder du") echo "unknown command" >&2; exit 1 ;;
  "system df") printf 'Build Cache\t4.2GB\n' ;;
esac"#,
        );
        let listing = list(&[program], TIMEOUT, 24);
        let rows: Vec<(Object, u64)> = listing
            .items
            .iter()
            .map(|item| (item.object, item.bytes))
            .collect();
        assert_eq!(rows, [(Object::BuildCache, 4_200_000_000)]);
    }

    #[test]
    fn refusals_name_the_engine() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let program = script(
            tmp.path(),
            "podman",
            "echo 'Error: volume pgdata is being used' >&2\nexit 2",
        );
        let item = Item {
            engine: "podman".to_owned(),
            program,
            object: Object::Volume,
            id: "pgdata".to_owned(),
            label: "pgdata".to_owned(),
            bytes: 0,
            safety: Safety::Review,
            note: String::new(),
        };
        let error = remove(&Guard::new(&[]), &item.candidate(1), Mode::Permanent).unwrap_err();
        assert_eq!(
            error.to_string(),
            "podman refused: Error: volume pgdata is being used"
        );
    }

    #[test]
    fn reads_stop_times() {
        assert_eq!(hours_ago("Exited (0) 3 days ago"), Some(72));
        assert_eq!(hours_ago("Exited (137) 2 hours ago"), Some(2));
        assert_eq!(hours_ago("Exited (1) About an hour ago"), Some(1));
        assert_eq!(hours_ago("Exited (0) 45 seconds ago"), Some(0));
        assert_eq!(hours_ago("Exited (0) 2 weeks ago"), Some(336));
        assert_eq!(hours_ago("Created"), None);
    }

    #[test]
    fn foreign_paths_are_not_removed() {
        for path in [
            "/home/me/x",
            "docker:image/",
            "docker:thing/abc",
            "docker:image/-f",
        ] {
            assert!(parse_path(Path::new(path)).is_none(), "{path}");
        }
    }
}
