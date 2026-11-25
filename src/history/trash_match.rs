use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::thread;

use super::History;
use crate::delete::Mode;

/// @name trashed_paths
/// @description Lists the original locations of everything currently in the system trash, or
/// `None` where the platform cannot list the trash.
///
/// @example
/// let trashed = history::trashed_paths().unwrap_or_default();
#[cfg(any(
    target_os = "windows",
    all(
        unix,
        not(target_os = "macos"),
        not(target_os = "ios"),
        not(target_os = "android")
    )
))]
pub fn trashed_paths() -> Option<Vec<PathBuf>> {
    let items = trash::os_limited::list().ok()?;
    Some(items.iter().map(trash::TrashItem::original_path).collect())
}

#[cfg(not(any(
    target_os = "windows",
    all(
        unix,
        not(target_os = "macos"),
        not(target_os = "ios"),
        not(target_os = "android")
    )
)))]
pub fn trashed_paths() -> Option<Vec<PathBuf>> {
    None
}

/// @name TrashListing
/// @description Runs `trashed_paths` on its own thread so a frontend can start listing the trash
/// at launch and pick the result up once it needs it, instead of waiting for it while loading.
///
/// @example
/// let listing = history::TrashListing::start();
/// let trashed = listing.wait();
pub struct TrashListing(Option<thread::JoinHandle<Option<Vec<PathBuf>>>>);

impl TrashListing {
    pub fn start() -> Self {
        Self(
            thread::Builder::new()
                .name("kuizer-trash".to_owned())
                .spawn(trashed_paths)
                .ok(),
        )
    }

    pub fn wait(self) -> Option<Vec<PathBuf>> {
        match self.0 {
            Some(handle) => handle.join().ok().flatten(),
            None => trashed_paths(),
        }
    }
}

const RESTORE_WINDOW_SECONDS: i64 = 300;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashRecord {
    pub path: PathBuf,
    pub deleted_at: Option<jiff::civil::DateTime>,
}

impl TrashRecord {
    /// @name TrashRecord::new
    /// @description Builds a record from a trash item's original path and the Unix time the
    /// trash library reports. Freedesktop `.trashinfo` files store local wall-clock time without
    /// an offset and the library reads it in today's time zone, so the time is turned back into
    /// that wall-clock time, which stays comparable after a time zone or DST change.
    ///
    /// @example
    /// let record = TrashRecord::new(item.original_path(), item.time_deleted);
    pub fn new(path: PathBuf, time_deleted: i64) -> Self {
        let deleted_at = (time_deleted >= 0)
            .then(|| jiff::Timestamp::from_second(time_deleted).ok())
            .flatten()
            .map(|time| time.to_zoned(jiff::tz::TimeZone::system()).datetime());
        Self { path, deleted_at }
    }
}

/// @name match_trash
/// @description Pairs each requested trashed entry with an item in the trash: the same original
/// path, deleted closest to the entry's local wall-clock time and within five minutes of it,
/// so a time zone or DST change between delete and restore does not matter. Items without a
/// deletion time match only when no timed item does. Every trash item is used at most once.
///
/// @example
/// let matched = history::match_trash(&history, &targets, &records);
/// if let Some(item) = matched[0] { restore(&items[item]) }
pub fn match_trash(
    history: &History,
    targets: &[(usize, usize)],
    trash: &[TrashRecord],
) -> Vec<Option<usize>> {
    let mut used = vec![false; trash.len()];
    let mut by_path: HashMap<&Path, Vec<usize>> = HashMap::new();
    for (index, record) in trash.iter().enumerate() {
        by_path
            .entry(record.path.as_path())
            .or_default()
            .push(index);
    }
    targets
        .iter()
        .map(|&(run, entry)| {
            let entry = &history.runs[run].deleted[entry];
            if entry.mode != Mode::Trash {
                return None;
            }
            let deleted = entry.deleted_at.parse::<jiff::civil::DateTime>().ok();
            let same_path = by_path.get(entry.path.as_path())?;
            let timed = same_path
                .iter()
                .copied()
                .filter(|&index| !used[index])
                .filter_map(|index| {
                    let gap = trash[index]
                        .deleted_at?
                        .duration_since(deleted?)
                        .as_secs()
                        .abs();
                    (gap <= RESTORE_WINDOW_SECONDS).then_some((gap, index))
                })
                .min()
                .map(|(_, index)| index);
            let found = timed.or_else(|| {
                same_path
                    .iter()
                    .copied()
                    .find(|&index| !used[index] && trash[index].deleted_at.is_none())
            });
            if let Some(index) = found {
                used[index] = true;
            }
            found
        })
        .collect()
}
