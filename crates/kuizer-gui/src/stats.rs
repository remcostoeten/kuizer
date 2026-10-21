//! Figures the views show, computed once when the report or the selection changes instead of on
//! every frame: report totals and charts, an id lookup into the report, and running selection
//! totals.

use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::SystemTime;

use kuizer::detect::{Category, Safety};
use kuizer::scan::Candidate;

use crate::model::{Bucket, Report, Source, TrashState};

/// One selectable item: the list it is in and its candidate id.
pub type Key = (Source, u64);

const PROJECTS: usize = 8;

fn slot(source: Source) -> usize {
    match source {
        Source::Artifact => 0,
        Source::Cache => 1,
        Source::Toolchain => 2,
        Source::Docker => 3,
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Tally {
    pub count: usize,
    pub bytes: u64,
}

impl Tally {
    fn add(&mut self, bytes: u64) {
        self.count += 1;
        self.bytes += bytes;
    }

    fn sub(&mut self, bytes: u64) {
        self.count -= 1;
        self.bytes -= bytes;
    }

    fn plus(self, other: Self) -> Self {
        Self {
            count: self.count + other.count,
            bytes: self.bytes + other.bytes,
        }
    }
}

/// Items and bytes of a group of candidates, in all and split by safety.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Totals {
    pub all: Tally,
    pub safe: Tally,
    pub review: Tally,
}

impl Totals {
    fn add(&mut self, bytes: u64, safety: Safety) {
        self.all.add(bytes);
        match safety {
            Safety::Safe => self.safe.add(bytes),
            Safety::Review => self.review.add(bytes),
        }
    }

    fn sub(&mut self, bytes: u64, safety: Safety) {
        self.all.sub(bytes);
        match safety {
            Safety::Safe => self.safe.sub(bytes),
            Safety::Review => self.review.sub(bytes),
        }
    }

    fn of<'a>(candidates: impl Iterator<Item = &'a Candidate>) -> Self {
        let mut totals = Self::default();
        for candidate in candidates {
            totals.add(candidate.usage.allocated, candidate.rule.safety);
        }
        totals
    }

    fn plus(self, other: Self) -> Self {
        Self {
            all: self.all.plus(other.all),
            safe: self.safe.plus(other.safe),
            review: self.review.plus(other.review),
        }
    }
}

/// Totals of a set of selected items, per list.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SelectionStats {
    sources: [Totals; 4],
}

impl SelectionStats {
    /// @name SelectionStats::of
    /// @description Counts a list of items from scratch.
    ///
    /// @example
    /// let stats = SelectionStats::of(app.selected().into_iter());
    pub fn of<'a>(items: impl Iterator<Item = (Source, &'a Candidate)>) -> Self {
        let mut stats = Self::default();
        for (source, candidate) in items {
            stats.sources[slot(source)].add(candidate.usage.allocated, candidate.rule.safety);
        }
        stats
    }

    pub fn source(&self, source: Source) -> Totals {
        self.sources[slot(source)]
    }

    pub fn total(&self) -> Totals {
        self.sources
            .iter()
            .fold(Totals::default(), |sum, totals| sum.plus(*totals))
    }

    /// How many items are deleted for good instead of moved to the trash.
    pub fn permanent(&self) -> usize {
        Source::ALL
            .into_iter()
            .filter(|source| source.permanent())
            .map(|source| self.source(source).all.count)
            .sum()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Weight {
    bytes: u64,
    safety: Safety,
}

/// @name Selection
/// @description The selected items with running totals, so the status bar and the confirmation
/// read their figures without walking the report. Every change goes through a method that keeps
/// the totals in step.
///
/// @example
/// selection.insert((Source::Artifact, candidate.id), candidate);
/// let bytes = selection.stats().total().all.bytes;
#[derive(Debug, Default)]
pub struct Selection {
    keys: HashMap<Key, Weight>,
    stats: SelectionStats,
    generation: u64,
}

impl Selection {
    /// Keys carried over from an earlier report. They count for nothing until `reweigh` runs
    /// against the report they belong to.
    pub fn pending(keys: impl IntoIterator<Item = Key>) -> Self {
        let unweighed = Weight {
            bytes: 0,
            safety: Safety::Safe,
        };
        Self {
            keys: keys.into_iter().map(|key| (key, unweighed)).collect(),
            stats: SelectionStats::default(),
            generation: 0,
        }
    }

    pub fn stats(&self) -> SelectionStats {
        self.stats
    }

    /// Changes whenever the selection does, so views can cache what they derive from it.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn contains(&self, key: &Key) -> bool {
        self.keys.contains_key(key)
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    pub fn keys(&self) -> impl Iterator<Item = &Key> {
        self.keys.keys()
    }

    /// Adds an item, and says whether it was not selected before.
    pub fn insert(&mut self, key: Key, candidate: &Candidate) -> bool {
        if self.keys.contains_key(&key) {
            return false;
        }
        let weight = Weight {
            bytes: candidate.usage.allocated,
            safety: candidate.rule.safety,
        };
        self.stats.sources[slot(key.0)].add(weight.bytes, weight.safety);
        self.keys.insert(key, weight);
        self.generation += 1;
        true
    }

    /// Drops an item, and says whether it was selected.
    pub fn remove(&mut self, key: &Key) -> bool {
        let Some(weight) = self.keys.remove(key) else {
            return false;
        };
        self.stats.sources[slot(key.0)].sub(weight.bytes, weight.safety);
        self.generation += 1;
        true
    }

    pub fn clear(&mut self) {
        self.keys.clear();
        self.stats = SelectionStats::default();
        self.generation += 1;
    }

    /// @name Selection::reset
    /// @description Replaces the selection with `keys`, keeping only those `find` knows and
    /// weighing each by its current candidate. Used whenever the report changes.
    ///
    /// @example
    /// selection.reset(keys, |key| index.find(report, key));
    pub fn reset<'a>(
        &mut self,
        keys: impl IntoIterator<Item = Key>,
        find: impl Fn(Key) -> Option<&'a Candidate>,
    ) {
        self.clear();
        for key in keys {
            if let Some(candidate) = find(key) {
                self.insert(key, candidate);
            }
        }
    }

    /// Weighs the selection again against the report, forgetting items it no longer has.
    pub fn reweigh<'a>(&mut self, find: impl Fn(Key) -> Option<&'a Candidate>) {
        let keys: Vec<Key> = self.keys.keys().copied().collect();
        self.reset(keys, find);
    }
}

impl PartialEq<HashSet<Key>> for Selection {
    fn eq(&self, other: &HashSet<Key>) -> bool {
        self.keys.len() == other.len() && other.iter().all(|key| self.keys.contains_key(key))
    }
}

/// @name Index
/// @description Finds a candidate of the report by list and id without a linear search. Built
/// on first use after each report change; a stale entry falls back to a search, never to a
/// wrong item.
///
/// @example
/// let index = Index::default();
/// let candidate = index.find(&report, (Source::Cache, id));
#[derive(Debug, Default)]
pub struct Index(OnceCell<HashMap<Key, usize>>);

impl Index {
    fn map(&self, report: &Report) -> &HashMap<Key, usize> {
        self.0.get_or_init(|| {
            Source::ALL
                .into_iter()
                .flat_map(|source| {
                    report
                        .section(source)
                        .enumerate()
                        .map(move |(position, candidate)| ((source, candidate.id), position))
                })
                .collect()
        })
    }

    pub fn find<'a>(&self, report: &'a Report, key: Key) -> Option<&'a Candidate> {
        self.locate(report, key).map(|(_, candidate)| candidate)
    }

    /// Where an item sits in its list of the report, and the item.
    pub fn locate<'a>(&self, report: &'a Report, key: Key) -> Option<(usize, &'a Candidate)> {
        if let Some(&position) = self.map(report).get(&key)
            && let Some(candidate) = at(report, key.0, position).filter(|c| c.id == key.1)
        {
            return Some((position, candidate));
        }
        (0..count(report, key.0)).find_map(|position| {
            at(report, key.0, position)
                .filter(|candidate| candidate.id == key.1)
                .map(|candidate| (position, candidate))
        })
    }
}

fn count(report: &Report, source: Source) -> usize {
    match source {
        Source::Artifact => report.candidates.len(),
        Source::Cache => report.caches.len(),
        Source::Toolchain => report.toolchains.len(),
        Source::Docker => report.docker.len(),
    }
}

fn at(report: &Report, source: Source, position: usize) -> Option<&Candidate> {
    match source {
        Source::Artifact => report.candidates.get(position),
        Source::Cache => report.caches.get(position).map(|(_, candidate)| candidate),
        Source::Toolchain => report
            .toolchains
            .get(position)
            .map(|(_, candidate)| candidate),
        Source::Docker => report.docker.get(position).map(|(_, candidate)| candidate),
    }
}

/// What the history tab shows about past cleanups.
#[derive(Debug, Default)]
pub struct HistoryStats {
    pub trash: Vec<TrashState>,
    pub freed: u64,
    pub in_trash: u64,
}

/// The overview charts, added up the first time the overview shows a report.
pub struct Charts {
    pub categories: Vec<Bucket<Category>>,
    pub projects: Vec<Bucket<PathBuf>>,
    pub ages: Vec<kuizer::stats::Bucket>,
    pub sizes: Vec<kuizer::stats::Bucket>,
    pub ecosystems: Vec<kuizer::stats::Bucket>,
    pub removed: Option<Vec<kuizer::stats::Bucket>>,
}

/// @name ReportStats
/// @description What the sidebar, the status line, the overview and the history tab add up from
/// the report. Totals are counted when the report changes; charts and trash states the first
/// time a view asks for them, so a streaming scan only pays for what is on screen.
///
/// @example
/// let stats = ReportStats::new(&report);
/// let safe = stats.artifacts.safe.bytes;
#[derive(Default)]
pub struct ReportStats {
    pub artifacts: Totals,
    pub caches: Totals,
    pub toolchains: Totals,
    pub docker: Totals,
    charts: OnceCell<Charts>,
    history: OnceCell<Option<HistoryStats>>,
}

impl ReportStats {
    pub fn new(report: &Report) -> Self {
        Self {
            artifacts: Totals::of(report.section(Source::Artifact)),
            caches: Totals::of(report.section(Source::Cache)),
            toolchains: Totals::of(report.section(Source::Toolchain)),
            docker: Totals::of(report.section(Source::Docker)),
            charts: OnceCell::new(),
            history: OnceCell::new(),
        }
    }

    /// Everything the space bar and the stat cards count as removable.
    pub fn space(&self) -> u64 {
        self.artifacts.all.bytes
            + self.caches.all.bytes
            + self.toolchains.all.bytes
            + self.docker.all.bytes
    }

    pub fn charts(&self, report: &Report) -> &Charts {
        self.charts.get_or_init(|| {
            let mut projects = report.by_project();
            projects.truncate(PROJECTS);
            Charts {
                categories: report.by_category(),
                projects,
                ages: kuizer::stats::by_age(&report.candidates, SystemTime::now()),
                sizes: kuizer::stats::by_size(&report.candidates),
                ecosystems: kuizer::stats::by_ecosystem(&report.candidates),
                removed: report.history.as_ref().ok().map(|view| {
                    kuizer::stats::removed_by_month(&view.history, &kuizer::history::now(), 12)
                }),
            }
        })
    }

    pub fn history(&self, report: &Report) -> Option<&HistoryStats> {
        self.history
            .get_or_init(|| {
                report.history.as_ref().ok().map(|view| HistoryStats {
                    trash: view.trash_states(),
                    freed: view.freed(),
                    in_trash: view.in_trash(),
                })
            })
            .as_ref()
    }

    pub fn trash_state(&self, report: &Report, run: usize) -> TrashState {
        self.history(report)
            .and_then(|history| history.trash.get(run).copied())
            .unwrap_or(TrashState::NotTrashed)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use kuizer::detect::{Kind, RuleInfo};
    use kuizer::scan::Usage;

    use super::*;

    fn candidate(id: u64, bytes: u64, safety: Safety) -> Candidate {
        Candidate {
            id,
            path: PathBuf::from(format!("/p/{id}")),
            kind: Kind::Dir,
            rule: Arc::new(RuleInfo {
                name: "x".to_owned(),
                category: Category::ALL[0],
                safety,
                ecosystem: "x".to_owned(),
            }),
            project: None,
            usage: Usage {
                allocated: bytes,
                apparent: bytes,
                entries: 1,
                partial: false,
            },
            newest: None,
            identity: None,
            project_last_active: None,
        }
    }

    #[test]
    fn running_totals_match_a_recount() {
        let sources = [
            Source::Artifact,
            Source::Cache,
            Source::Toolchain,
            Source::Docker,
        ];
        let items: Vec<(Source, Candidate)> = (0..64)
            .map(|id| {
                let safety = if id % 3 == 0 {
                    Safety::Review
                } else {
                    Safety::Safe
                };
                (sources[id as usize % 4], candidate(id, id * 97 + 1, safety))
            })
            .collect();
        let mut selection = Selection::default();
        let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
        for round in 0..2000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let (source, candidate) = &items[(seed % items.len() as u64) as usize];
            let key = (*source, candidate.id);
            match seed % 7 {
                0..=2 => {
                    selection.insert(key, candidate);
                }
                3..=5 => {
                    selection.remove(&key);
                }
                _ if round % 50 == 0 => selection.clear(),
                _ => selection.reweigh(|key| {
                    items
                        .iter()
                        .find(|(source, candidate)| (*source, candidate.id) == key)
                        .filter(|_| key.1 % 5 != 0)
                        .map(|(_, candidate)| candidate)
                }),
            }
            let recount = SelectionStats::of(
                items
                    .iter()
                    .filter(|(source, candidate)| selection.contains(&(*source, candidate.id)))
                    .map(|(source, candidate)| (*source, candidate)),
            );
            assert_eq!(selection.stats(), recount, "round {round}");
        }
    }
}
