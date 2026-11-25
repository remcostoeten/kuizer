use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Duration;

use serde::Deserialize;

use super::engine::{Failure, fields, joined, run, short, size};
use super::{Item, Object, hours_ago};
use crate::detect::Safety;

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

pub(super) fn containers(
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

pub(super) fn volume_note(known: bool, measuring: bool) -> String {
    match (known, measuring) {
        (true, _) => "no container uses it, but it may hold data such as a database",
        (false, true) => "no container uses it, but it may hold data; measuring its size",
        (false, false) => "no container uses it, but it may hold data; size unknown",
    }
    .to_owned()
}

pub(super) fn volumes(
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

pub(super) struct Pending {
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
    pub(super) pending: Vec<Pending>,
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

pub(super) fn engine_items(
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

pub(super) fn sort_items(items: &mut [Item]) {
    items.sort_by(|a, b| {
        b.bytes
            .cmp(&a.bytes)
            .then_with(|| a.object.cmp(&b.object))
            .then_with(|| a.id.cmp(&b.id))
    });
}
