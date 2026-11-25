mod engine;
mod objects;
#[cfg(all(test, unix))]
mod tests;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

pub use self::engine::hours_ago;
use self::engine::{
    Failure, describe, engine_name, parse_path, refusal, run, short, still_untagged,
};
pub use self::objects::{SizeUpdate, Sizing};
use self::objects::{engine_items, sort_items, volume_note};

use crate::delete::{DeleteError, Mode};
use crate::detect::{Category, Kind, RuleInfo, Safety};
use crate::process;
use crate::scan::{Candidate, Usage};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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

/// @name remove
/// @description Removes one object through its engine: `image rm`, `container rm`,
/// `builder prune --force` or `volume rm`, never with force on an image, container or volume, so
/// anything still in use is refused by the engine itself. An image is inspected right before it
/// is removed and left alone once it carries a tag. Nothing under `/var/lib/docker` is touched.
/// These removals are permanent. The engine must be listed in `engines`. A dry run only
/// checks the object path and the engine.
///
/// @example
/// docker::remove(&config.docker.engines, &candidate, Mode::Permanent)?;
pub fn remove(engines: &[String], candidate: &Candidate, mode: Mode) -> Result<(), DeleteError> {
    let (program, object, id) = parse_path(&candidate.path).ok_or_else(|| {
        DeleteError::Tool(format!(
            "{} is not a Docker or Podman object",
            candidate.path.display()
        ))
    })?;
    if !engines.iter().any(|engine| engine == program) {
        return Err(DeleteError::Tool(format!(
            "{program} is not enabled in docker.engines"
        )));
    }
    if mode == Mode::DryRun {
        return Ok(());
    }
    let engine = engine_name(program);
    if object == Object::Image && !still_untagged(program, id).map_err(|f| refusal(engine, f))? {
        return Err(DeleteError::Tool(format!(
            "{engine} image {} is tagged now, not removed",
            short(id)
        )));
    }
    let args: Vec<&str> = match object {
        Object::Image => vec!["image", "rm", id],
        Object::Container => vec!["container", "rm", id],
        Object::BuildCache => vec!["builder", "prune", "--force"],
        Object::Volume => vec!["volume", "rm", id],
    };
    run(program, &args, process::MUTATION_TIMEOUT)
        .map(drop)
        .map_err(|failure| refusal(engine, failure))
}
