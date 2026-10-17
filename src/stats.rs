use std::collections::BTreeMap;
use std::time::{Duration, SystemTime};

use crate::history::{Action, History};
use crate::scan::Candidate;

const DAY: u64 = 86_400;
const MB: u64 = 1_000_000;
const GB: u64 = 1_000_000_000;

pub const AGES: [(&str, u64); 6] = [
    ("1 week", 7 * DAY),
    ("1 month", 30 * DAY),
    ("3 months", 91 * DAY),
    ("6 months", 182 * DAY),
    ("1 year", 365 * DAY),
    ("older", u64::MAX),
];

pub const SIZES: [(&str, u64); 6] = [
    ("< 1 MB", MB),
    ("< 10 MB", 10 * MB),
    ("< 100 MB", 100 * MB),
    ("< 1 GB", GB),
    ("< 10 GB", 10 * GB),
    ("10 GB +", u64::MAX),
];

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bucket {
    pub label: String,
    pub bytes: u64,
    pub count: usize,
}

impl Bucket {
    fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            bytes: 0,
            count: 0,
        }
    }

    fn add(&mut self, bytes: u64) {
        self.bytes += bytes;
        self.count += 1;
    }
}

/// @name idle
/// @description How long ago an artifact's project was last changed, falling back to the newest
/// file inside the artifact when the project time is unknown.
///
/// @example
/// let quiet = idle(&candidate, SystemTime::now());
pub fn idle(candidate: &Candidate, now: SystemTime) -> Option<Duration> {
    let changed = candidate.project_last_active.or(candidate.newest)?;
    Some(now.duration_since(changed).unwrap_or_default())
}

/// @name by_age
/// @description Groups artifacts into the fixed `AGES` ranges by how long their project has
/// been idle. Artifacts without any known time are left out.
///
/// @example
/// let ages = by_age(&report.candidates, SystemTime::now());
pub fn by_age(candidates: &[Candidate], now: SystemTime) -> Vec<Bucket> {
    let mut buckets: Vec<Bucket> = AGES.iter().map(|(label, _)| Bucket::new(*label)).collect();
    for candidate in candidates {
        let Some(idle) = idle(candidate, now) else {
            continue;
        };
        let index = AGES
            .iter()
            .position(|(_, limit)| idle.as_secs() < *limit)
            .unwrap_or(AGES.len() - 1);
        buckets[index].add(candidate.usage.allocated);
    }
    buckets
}

/// @name by_size
/// @description Groups artifacts into the fixed `SIZES` ranges by their size on disk.
///
/// @example
/// let sizes = by_size(&report.candidates);
pub fn by_size(candidates: &[Candidate]) -> Vec<Bucket> {
    let mut buckets: Vec<Bucket> = SIZES.iter().map(|(label, _)| Bucket::new(*label)).collect();
    for candidate in candidates {
        let bytes = candidate.usage.allocated;
        let index = SIZES
            .iter()
            .position(|(_, limit)| bytes < *limit)
            .unwrap_or(SIZES.len() - 1);
        buckets[index].add(bytes);
    }
    buckets
}

/// @name by_ecosystem
/// @description Sums artifacts per ecosystem (`node`, `rust`, ...), largest first.
///
/// @example
/// let ecosystems = by_ecosystem(&report.candidates);
pub fn by_ecosystem(candidates: &[Candidate]) -> Vec<Bucket> {
    let mut groups: BTreeMap<&str, Bucket> = BTreeMap::new();
    for candidate in candidates {
        let name = candidate.rule.ecosystem.as_str();
        groups
            .entry(name)
            .or_insert_with(|| Bucket::new(name))
            .add(candidate.usage.allocated);
    }
    let mut buckets: Vec<Bucket> = groups.into_values().collect();
    buckets.sort_by_key(|bucket| std::cmp::Reverse(bucket.bytes));
    buckets
}

/// @name removed_by_month
/// @description Sums the bytes and items of every delete run per calendar month, for the
/// `months` months up to and including `current` (a `YYYY-MM` prefix such as the one
/// `history::now` starts with). Months without runs stay in the list with zero.
///
/// @example
/// let months = removed_by_month(&history, &history::now(), 12);
pub fn removed_by_month(history: &History, current: &str, months: usize) -> Vec<Bucket> {
    let Some((mut year, mut month)) = month_of(current) else {
        return Vec::new();
    };
    let mut keys = Vec::with_capacity(months);
    for _ in 0..months {
        keys.push((year, month));
        if month == 1 {
            year -= 1;
            month = 12;
        } else {
            month -= 1;
        }
    }
    keys.reverse();
    let mut buckets: Vec<Bucket> = keys
        .iter()
        .map(|&(year, month)| Bucket::new(format!("{} {year}", MONTHS[month as usize - 1])))
        .collect();
    for run in history
        .runs
        .iter()
        .filter(|run| run.action == Action::Delete)
    {
        let Some(key) = month_of(&run.started_at) else {
            continue;
        };
        if let Some(index) = keys.iter().position(|&candidate| candidate == key) {
            buckets[index].bytes += run.total.bytes;
            buckets[index].count += run.total.items;
        }
    }
    buckets
}

fn month_of(stamp: &str) -> Option<(i32, u32)> {
    let year = stamp.get(..4)?.parse().ok()?;
    let month = stamp.get(5..7)?.parse().ok()?;
    (stamp.get(4..5) == Some("-") && (1..=12).contains(&month)).then_some((year, month))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use super::*;
    use crate::detect::{Category, Kind, RuleInfo, Safety};
    use crate::history::{Run, Total};
    use crate::scan::Usage;

    fn candidate(
        bytes: u64,
        ecosystem: &str,
        idle_days: Option<u64>,
        now: SystemTime,
    ) -> Candidate {
        Candidate {
            id: bytes,
            path: PathBuf::from("/p/node_modules"),
            kind: Kind::Dir,
            rule: Arc::new(RuleInfo {
                name: "rule".to_owned(),
                category: Category::Dependencies,
                safety: Safety::Safe,
                ecosystem: ecosystem.to_owned(),
            }),
            project: None,
            usage: Usage {
                allocated: bytes,
                ..Usage::default()
            },
            newest: None,
            identity: None,
            project_last_active: idle_days.map(|days| now - Duration::from_secs(days * DAY)),
        }
    }

    fn run(started_at: &str, action: Action, bytes: u64, items: usize) -> Run {
        Run {
            id: 0,
            action,
            started_at: started_at.to_owned(),
            finished_at: started_at.to_owned(),
            command: "clean".to_owned(),
            roots: Vec::new(),
            deleted: Vec::new(),
            total: Total {
                items,
                bytes,
                size: String::new(),
            },
        }
    }

    #[test]
    fn ages_fall_into_the_first_range_they_fit() {
        let now = SystemTime::now();
        let candidates = [
            candidate(10, "node", Some(2), now),
            candidate(20, "node", Some(40), now),
            candidate(30, "rust", Some(800), now),
            candidate(40, "rust", None, now),
        ];
        let ages = by_age(&candidates, now);
        assert_eq!(ages.len(), AGES.len());
        assert_eq!((ages[0].bytes, ages[0].count), (10, 1));
        assert_eq!((ages[2].bytes, ages[2].count), (20, 1));
        assert_eq!((ages[5].bytes, ages[5].count), (30, 1));
        assert_eq!(ages.iter().map(|bucket| bucket.count).sum::<usize>(), 3);
    }

    #[test]
    fn sizes_use_decimal_limits() {
        let now = SystemTime::now();
        let candidates = [
            candidate(999_999, "node", None, now),
            candidate(MB, "node", None, now),
            candidate(20 * GB, "node", None, now),
        ];
        let sizes = by_size(&candidates);
        assert_eq!(sizes[0].count, 1);
        assert_eq!(sizes[1].count, 1);
        assert_eq!(sizes[5].bytes, 20 * GB);
    }

    #[test]
    fn ecosystems_are_summed_and_sorted_largest_first() {
        let now = SystemTime::now();
        let candidates = [
            candidate(5, "node", None, now),
            candidate(7, "rust", None, now),
            candidate(4, "node", None, now),
        ];
        let ecosystems = by_ecosystem(&candidates);
        assert_eq!(ecosystems[0].label, "node");
        assert_eq!((ecosystems[0].bytes, ecosystems[0].count), (9, 2));
        assert_eq!(ecosystems[1].label, "rust");
    }

    #[test]
    fn removed_months_cover_the_window_and_skip_other_actions() {
        let history = History {
            runs: vec![
                run("2026-10-02T10:00:00+02:00", Action::Delete, 100, 2),
                run("2026-10-17T10:00:00+02:00", Action::Delete, 50, 1),
                run("2026-01-05T10:00:00+01:00", Action::Delete, 7, 1),
                run("2026-10-03T10:00:00+02:00", Action::Restore, 999, 9),
                run("2025-12-31T10:00:00+01:00", Action::Delete, 3, 1),
                run("garbage", Action::Delete, 1, 1),
            ],
            ..History::default()
        };
        let months = removed_by_month(&history, "2026-10-17T23:00:00+02:00", 12);
        assert_eq!(months.len(), 12);
        assert_eq!(months[0].label, "Nov 2025");
        assert_eq!(months[1].label, "Dec 2025");
        assert_eq!(months[1].bytes, 3);
        assert_eq!(months[2].bytes, 7);
        let last = months.last().unwrap();
        assert_eq!(last.label, "Oct 2026");
        assert_eq!((last.bytes, last.count), (150, 3));
        assert!(removed_by_month(&history, "bad", 12).is_empty());
    }
}
