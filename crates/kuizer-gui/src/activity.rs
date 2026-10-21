use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use kuizer::scan::Progress;

use crate::start::ScanOptions;

const LARGEST: usize = 6;
const PULSE: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Folders,
    Caches,
    Toolchains,
    Repos,
    Docker,
    History,
}

impl Step {
    pub const ALL: [Step; 6] = [
        Self::Folders,
        Self::Caches,
        Self::Toolchains,
        Self::Repos,
        Self::Docker,
        Self::History,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Folders => "Scan folders",
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
            Self::Caches => options.caches,
            Self::Toolchains => options.toolchains,
            Self::Repos => options.repos,
            Self::Docker => options.docker,
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

/// A step that is running, with its own counter and the folder or item it touched last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Running {
    pub step: Step,
    pub count: Option<(usize, usize)>,
    pub current: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State<'a> {
    Off,
    Pending,
    Active(&'a Running),
    Done(&'a Done),
}

/// What a running scan is doing: the steps running side by side with their live counters and
/// the folder or item each touched last, the steps already finished with their result and
/// duration, the folder walk's counters, and the largest items found so far.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Activity {
    pub walked: Progress,
    pub running: Vec<Running>,
    pub done: Vec<Done>,
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
        match self.running.iter().find(|running| running.step == step) {
            Some(running) => State::Active(running),
            None => State::Pending,
        }
    }

    pub fn begin(&mut self, step: Step, total: Option<usize>) {
        self.running.retain(|running| running.step != step);
        self.running.push(Running {
            step,
            count: total.map(|total| (0, total)),
            current: None,
        });
    }

    pub fn finish(&mut self, step: Step, took: Duration, result: String) {
        self.running.retain(|running| running.step != step);
        self.done.retain(|done| done.step != step);
        self.done.push(Done { step, took, result });
    }

    pub fn advance(&mut self, step: Step, path: &Path) {
        if let Some(running) = self.running.iter_mut().find(|running| running.step == step) {
            if let Some((done, total)) = &mut running.count {
                *done = (*done + 1).min(*total);
            }
            running.current = Some(path.to_path_buf());
        }
    }

    pub fn touch(&mut self, step: Step, current: Option<PathBuf>) {
        if let Some(running) = self.running.iter_mut().find(|running| running.step == step) {
            running.current = current;
        }
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
        if beats(&self.largest, bytes) && !self.largest.iter().any(|found| found.path == path) {
            keep(
                &mut self.largest,
                Found {
                    path: path.to_path_buf(),
                    name: name.to_owned(),
                    bytes,
                },
            );
        }
    }

    /// @name Activity::merge
    /// @description Adds items counted elsewhere: their count and bytes go into the totals, and
    /// each of `largest` that beats the list and is not on it yet is kept.
    ///
    /// @example
    /// activity.merge(found, bytes, &walk_largest);
    pub fn merge(&mut self, found: u64, bytes: u64, largest: &[Found]) {
        self.found += found;
        self.bytes += bytes;
        for item in largest {
            if beats(&self.largest, item.bytes)
                && !self.largest.iter().any(|found| found.path == item.path)
            {
                keep(&mut self.largest, item.clone());
            }
        }
    }

    /// @name Activity::detail
    /// @description The live counters of a running step, in a few words.
    ///
    /// @example
    /// let detail = activity.detail(running);
    pub fn detail(&self, running: &Running) -> String {
        if running.step == Step::Folders {
            if self.walked == Progress::default() {
                return "Starting".to_owned();
            }
            return format!(
                "{} folders, {} files",
                grouped(self.walked.dirs),
                grouped(self.walked.files)
            );
        }
        match running.count {
            Some((done, total)) => format!("{done} of {total}"),
            None => "Working".to_owned(),
        }
    }
}

/// @name beats
/// @description Whether an item of `bytes` makes it into a list of the largest items.
///
/// @example
/// if beats(&largest, candidate.usage.allocated) { .. }
pub fn beats(largest: &[Found], bytes: u64) -> bool {
    largest.len() < LARGEST || largest.last().is_some_and(|last| last.bytes < bytes)
}

/// @name keep
/// @description Puts an item into a list of the largest items, largest first, and drops what
/// falls off the end.
///
/// @example
/// keep(&mut largest, found);
pub fn keep(largest: &mut Vec<Found>, found: Found) {
    let at = largest.partition_point(|item| item.bytes >= found.bytes);
    largest.insert(at, found);
    largest.truncate(LARGEST);
}

type Notify = Box<dyn Fn(Activity) + Send + Sync>;

/// @name Tracker
/// @description The activity of a running scan, shared by the threads doing its steps. Every
/// change is applied at once, and the window hears about it at most every 100ms: `update`
/// sends right away when the last message is older than that, `flush` sends what is left.
///
/// @example
/// let tracker = Tracker::new(move |activity| { let _ = sender.unbounded_send(activity); });
/// tracker.update(|activity| activity.begin(Step::Caches, Some(4)));
pub struct Tracker {
    state: Mutex<Pulse>,
    notify: Notify,
}

struct Pulse {
    activity: Activity,
    sent: Option<Instant>,
    dirty: bool,
}

impl Tracker {
    pub fn new(notify: impl Fn(Activity) + Send + Sync + 'static) -> Self {
        Self {
            state: Mutex::new(Pulse {
                activity: Activity::default(),
                sent: None,
                dirty: false,
            }),
            notify: Box::new(notify),
        }
    }

    pub fn update(&self, change: impl FnOnce(&mut Activity)) {
        self.apply(change, false);
    }

    pub fn update_now(&self, change: impl FnOnce(&mut Activity)) {
        self.apply(change, true);
    }

    pub fn flush(&self) {
        let Ok(mut pulse) = self.state.lock() else {
            return;
        };
        if pulse.dirty && pulse.due() {
            self.send(&mut pulse);
        }
    }

    fn apply(&self, change: impl FnOnce(&mut Activity), now: bool) {
        let Ok(mut pulse) = self.state.lock() else {
            return;
        };
        change(&mut pulse.activity);
        pulse.dirty = true;
        if now || pulse.due() {
            self.send(&mut pulse);
        }
    }

    fn send(&self, pulse: &mut Pulse) {
        pulse.sent = Some(Instant::now());
        pulse.dirty = false;
        (self.notify)(pulse.activity.clone());
    }
}

impl Pulse {
    fn due(&self) -> bool {
        self.sent.is_none_or(|sent| sent.elapsed() >= PULSE)
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

    fn options() -> ScanOptions {
        ScanOptions {
            caches: true,
            docker: false,
            ..ScanOptions::from_config(&kuizer::config::Config::default())
        }
    }

    #[test]
    fn keeps_the_largest_items_in_order() {
        let mut activity = Activity::default();
        for (index, bytes) in [5, 1, 9, 3, 7, 2, 8, 4].into_iter().enumerate() {
            activity.record(Path::new(&format!("/p/{index}")), "target", bytes);
        }
        activity.merge(
            2,
            15,
            &[
                Found {
                    path: PathBuf::from("/p/2"),
                    name: "target".to_owned(),
                    bytes: 9,
                },
                Found {
                    path: PathBuf::from("/q"),
                    name: "target".to_owned(),
                    bytes: 6,
                },
            ],
        );
        let sizes: Vec<u64> = activity.largest.iter().map(|found| found.bytes).collect();
        assert_eq!(sizes, vec![9, 8, 7, 6, 5, 4]);
        assert_eq!(activity.found, 10);
        assert_eq!(activity.bytes, 54);
    }

    #[test]
    fn steps_run_side_by_side() {
        let mut activity = Activity::default();
        let options = options();
        activity.begin(Step::Folders, None);
        activity.begin(Step::Caches, Some(4));
        activity.advance(Step::Caches, Path::new("/home/me/.cargo/registry"));
        assert!(matches!(
            activity.state(Step::Folders, &options),
            State::Active(_)
        ));
        let State::Active(caches) = activity.state(Step::Caches, &options) else {
            panic!("caches run beside the folder walk");
        };
        assert_eq!(activity.detail(caches), "1 of 4");
        assert_eq!(
            caches.current.as_deref(),
            Some(Path::new("/home/me/.cargo/registry"))
        );
        assert_eq!(activity.state(Step::Repos, &options), State::Pending);
        assert_eq!(activity.state(Step::Docker, &options), State::Off);

        activity.finish(Step::Folders, Duration::from_secs(2), "3 items".to_owned());
        assert!(matches!(
            activity.state(Step::Folders, &options),
            State::Done(done) if done.result == "3 items"
        ));
        assert!(matches!(
            activity.state(Step::Caches, &options),
            State::Active(_)
        ));
    }

    #[test]
    fn the_tracker_sends_at_most_every_pulse_and_flushes_the_rest() {
        let sent = std::sync::Arc::new(Mutex::new(Vec::new()));
        let tracker = {
            let sent = std::sync::Arc::clone(&sent);
            Tracker::new(move |activity| sent.lock().unwrap().push(activity))
        };
        for index in 0..50 {
            tracker.update(|activity| activity.record(Path::new("/p"), "target", index));
        }
        assert_eq!(sent.lock().unwrap().len(), 1);
        std::thread::sleep(PULSE);
        tracker.flush();
        let sent = sent.lock().unwrap();
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[1].found, 50);
    }

    #[test]
    fn describes_the_folder_walk_with_grouped_counts() {
        let mut activity = Activity {
            walked: Progress {
                dirs: 12_345,
                files: 987_654,
                ..Progress::default()
            },
            ..Activity::default()
        };
        activity.begin(Step::Folders, None);
        assert_eq!(
            activity.detail(&activity.running[0].clone()),
            "12,345 folders, 987,654 files"
        );
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
