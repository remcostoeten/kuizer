use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use kuizer::dupes;

use crate::model::Stage;
use crate::start::ScanOptions;

const LARGEST: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Folders,
    Duplicates,
    Caches,
    Toolchains,
    Repos,
    Docker,
    History,
}

impl Step {
    pub const ALL: [Step; 7] = [
        Self::Folders,
        Self::Duplicates,
        Self::Caches,
        Self::Toolchains,
        Self::Repos,
        Self::Docker,
        Self::History,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Folders => "Scan folders",
            Self::Duplicates => "Find duplicate files",
            Self::Caches => "Measure global caches",
            Self::Toolchains => "Measure toolchains",
            Self::Repos => "Check git repositories",
            Self::Docker => "Ask Docker and Podman",
            Self::History => "Read cleanup history",
        }
    }

    pub fn enabled(self, options: &ScanOptions) -> bool {
        match self {
            Self::Folders | Self::History => true,
            Self::Duplicates => options.duplicates,
            Self::Caches => options.caches,
            Self::Toolchains => options.toolchains,
            Self::Repos => options.repos,
            Self::Docker => options.docker,
        }
    }

    pub fn of(stage: Stage) -> Self {
        match stage {
            Stage::Starting | Stage::Scanning(_) => Self::Folders,
            Stage::Duplicates(_) => Self::Duplicates,
            Stage::Caches => Self::Caches,
            Stage::Toolchains => Self::Toolchains,
            Stage::Repos => Self::Repos,
            Stage::Docker => Self::Docker,
            Stage::History => Self::History,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Done {
    pub step: Step,
    pub took: Duration,
    pub result: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub path: PathBuf,
    pub name: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State<'a> {
    Off,
    Pending,
    Active,
    Done(&'a Done),
}

/// What a running scan is doing, shared between the scan thread and the window: the step it
/// is on with its live counters, the steps already finished with their result and duration,
/// the folder or item it touched last, and the largest items found so far.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Activity {
    pub stage: Stage,
    pub done: Vec<Done>,
    pub current: Option<PathBuf>,
    pub count: Option<(usize, usize)>,
    pub largest: Vec<Found>,
    pub found: u64,
    pub bytes: u64,
}

impl Activity {
    pub fn state(&self, step: Step, options: &ScanOptions) -> State<'_> {
        if !step.enabled(options) {
            return State::Off;
        }
        if let Some(done) = self.done.iter().find(|done| done.step == step) {
            return State::Done(done);
        }
        if Step::of(self.stage) == step {
            State::Active
        } else {
            State::Pending
        }
    }

    pub fn begin(&mut self, stage: Stage, total: Option<usize>) {
        self.stage = stage;
        self.current = None;
        self.count = total.map(|total| (0, total));
    }

    pub fn finish(&mut self, step: Step, took: Duration, result: String) {
        self.done.retain(|done| done.step != step);
        self.done.push(Done { step, took, result });
        self.count = None;
        self.current = None;
    }

    pub fn advance(&mut self, path: &Path) {
        if let Some((done, total)) = &mut self.count {
            *done = (*done + 1).min(*total);
        }
        self.current = Some(path.to_path_buf());
    }

    /// @name Activity::record
    /// @description Counts a found item into the running totals and keeps it when it is among
    /// the largest found so far.
    ///
    /// @example
    /// activity.record(&candidate.path, &candidate.rule.name, candidate.usage.allocated);
    pub fn record(&mut self, path: &Path, name: &str, bytes: u64) {
        self.found += 1;
        self.bytes += bytes;
        if self.largest.len() == LARGEST
            && self.largest.last().is_some_and(|last| last.bytes >= bytes)
        {
            return;
        }
        let at = self.largest.partition_point(|found| found.bytes >= bytes);
        self.largest.insert(
            at,
            Found {
                path: path.to_path_buf(),
                name: name.to_owned(),
                bytes,
            },
        );
        self.largest.truncate(LARGEST);
    }

    /// @name Activity::detail
    /// @description The live counters of the step that is running, in a few words.
    ///
    /// @example
    /// assert_eq!(Activity::default().detail(), "Starting");
    pub fn detail(&self) -> String {
        match self.stage {
            Stage::Starting => "Starting".to_owned(),
            Stage::Scanning(progress) => format!(
                "{} folders, {} files",
                grouped(progress.dirs),
                grouped(progress.files)
            ),
            Stage::Duplicates(dupes::Stage::Walking { files }) => {
                format!("Collecting {} files", grouped(files))
            }
            Stage::Duplicates(dupes::Stage::Hashing { files }) => {
                format!("Hashing {} files of equal size", grouped(files as u64))
            }
            Stage::Duplicates(dupes::Stage::Verifying { groups }) => {
                format!("Comparing {} groups byte by byte", grouped(groups as u64))
            }
            _ => match self.count {
                Some((done, total)) => format!("{done} of {total}"),
                None => "Working".to_owned(),
            },
        }
    }
}

pub fn update(activity: &Mutex<Activity>, change: impl FnOnce(&mut Activity)) {
    if let Ok(mut activity) = activity.lock() {
        change(&mut activity);
    }
}

pub fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else if let Some(stem) = noun.strip_suffix('y') {
        format!("{} {stem}ies", grouped(count as u64))
    } else {
        format!("{} {noun}s", grouped(count as u64))
    }
}

/// @name took
/// @description Writes how long something ran: tenths below ten seconds, whole seconds below a
/// minute, then minutes and seconds.
///
/// @example
/// assert_eq!(took(Duration::from_secs(75)), "1m 15s");
pub fn took(duration: Duration) -> String {
    let seconds = duration.as_secs();
    if seconds < 10 {
        format!("{:.1}s", duration.as_secs_f64())
    } else if seconds < 60 {
        format!("{seconds}s")
    } else {
        format!("{}m {:02}s", seconds / 60, seconds % 60)
    }
}

/// @name grouped
/// @description Writes a count with thin thousands separators, the way every figure in the
/// window is written.
///
/// @example
/// assert_eq!(grouped(1234567), "1,234,567");
pub fn grouped(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use kuizer::scan::Progress;

    fn options() -> ScanOptions {
        ScanOptions {
            duplicates: false,
            caches: true,
            ..ScanOptions::from_config(&kuizer::config::Config::default())
        }
    }

    #[test]
    fn keeps_the_largest_items_in_order() {
        let mut activity = Activity::default();
        for bytes in [5, 1, 9, 3, 7, 2, 8, 4] {
            activity.record(Path::new("/p"), "target", bytes);
        }
        let sizes: Vec<u64> = activity.largest.iter().map(|found| found.bytes).collect();
        assert_eq!(sizes, vec![9, 8, 7, 5, 4, 3]);
        assert_eq!(activity.found, 8);
        assert_eq!(activity.bytes, 39);
    }

    #[test]
    fn steps_move_from_pending_to_active_to_done() {
        let mut activity = Activity::default();
        let options = options();
        assert_eq!(activity.state(Step::Folders, &options), State::Active);
        assert_eq!(activity.state(Step::Caches, &options), State::Pending);
        assert_eq!(activity.state(Step::Duplicates, &options), State::Off);

        activity.finish(Step::Folders, Duration::from_secs(2), "3 items".to_owned());
        activity.begin(Stage::Caches, Some(4));
        activity.advance(Path::new("/home/me/.cargo/registry"));
        assert!(matches!(
            activity.state(Step::Folders, &options),
            State::Done(done) if done.result == "3 items"
        ));
        assert_eq!(activity.state(Step::Caches, &options), State::Active);
        assert_eq!(activity.detail(), "1 of 4");
        assert_eq!(
            activity.current.as_deref(),
            Some(Path::new("/home/me/.cargo/registry"))
        );
    }

    #[test]
    fn describes_the_folder_walk_with_grouped_counts() {
        let activity = Activity {
            stage: Stage::Scanning(Progress {
                dirs: 12_345,
                files: 987_654,
                ..Progress::default()
            }),
            ..Activity::default()
        };
        assert_eq!(activity.detail(), "12,345 folders, 987,654 files");
    }

    #[test]
    fn writes_durations_for_people() {
        assert_eq!(took(Duration::from_millis(420)), "0.4s");
        assert_eq!(took(Duration::from_secs(42)), "42s");
        assert_eq!(took(Duration::from_secs(75)), "1m 15s");
    }

    #[test]
    fn plurals_count_with_separators() {
        assert_eq!(plural(1, "repository"), "1 repository");
        assert_eq!(plural(3, "repository"), "3 repositories");
        assert_eq!(plural(1_200, "item"), "1,200 items");
    }

    #[test]
    fn groups_thousands() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1_000), "1,000");
        assert_eq!(grouped(1_234_567), "1,234,567");
    }
}
