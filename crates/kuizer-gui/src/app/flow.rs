//! The delete flow as motion: a row blends from idle to marked to deleting to gone, each step
//! starting from where the previous one left it, and the summary bar rises in with the first
//! selected item and fades out with the last.

use gpui::Rgba;

use super::*;

const SLIDE: f32 = space::XXL;
const MARKS: usize = 64;

type Key = (Source, u64);
type Paths = HashMap<Source, HashSet<PathBuf>>;

/// A row part way through the delete flow. Each field runs from 0 to 1, except `opacity`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Look {
    pub(super) checked: f32,
    pub(super) opacity: f32,
    pub(super) slide: f32,
    pub(super) deleting: bool,
}

impl Look {
    fn background(&self) -> Rgba {
        faded(theme::selected(), self.checked)
    }
}

/// When the summary bar last appeared or went away, and the selection it sums up.
struct Shown {
    at: Instant,
    open: bool,
    stats: SelectionStats,
}

/// What a finished cleanup lets go of: `removed` rows fade out and leave the report once the
/// step ends, the other rows of the cleanup fade back from the deleting look.
struct Settling {
    at: Instant,
    from: f32,
    paths: Paths,
    removed: Paths,
    _task: Option<Task<()>>,
}

#[derive(Default)]
pub(super) struct Flow {
    marks: HashMap<Key, Instant>,
    bar: Option<Shown>,
    settling: Option<Settling>,
}

impl Flow {
    /// Starts the checkbox of a row blending toward its new state. A burst of toggles keeps
    /// only the latest few, so a held key never grows the map.
    pub(super) fn mark(&mut self, key: Key) {
        if motion::reduced() {
            return;
        }
        let now = Instant::now();
        self.marks
            .retain(|_, at| now.saturating_duration_since(*at) < motion::STEP);
        if self.marks.len() < MARKS {
            self.marks.insert(key, now);
        }
    }

    /// Notes the selection growing from or shrinking to nothing, which brings the summary bar
    /// in or out. A bar on its way out keeps the totals it showed last.
    pub(super) fn observe(&mut self, stats: SelectionStats) {
        let open = stats.total().all.count > 0;
        match &mut self.bar {
            Some(shown) if shown.open == open => {
                if open {
                    shown.stats = stats;
                }
            }
            None if !open => {}
            bar => {
                let stats = match bar {
                    Some(shown) if !open => shown.stats,
                    _ => stats,
                };
                *bar = Some(Shown {
                    at: Instant::now(),
                    open,
                    stats,
                });
            }
        }
    }

    pub(super) fn bar(&self) -> Option<(SelectionStats, f32)> {
        let shown = self.bar.as_ref()?;
        let blend = progress(shown.at, Instant::now());
        match (shown.open, blend < 1.0) {
            (true, _) => Some((shown.stats, blend)),
            (false, true) => Some((shown.stats, 1.0 - blend)),
            (false, false) => None,
        }
    }

    /// Whether a row is still blending, so the window draws another frame.
    fn moving(&self, deleting: Option<Instant>) -> bool {
        if motion::reduced() {
            return false;
        }
        let now = Instant::now();
        let fresh = |at: Instant| now.saturating_duration_since(at) < motion::STEP;
        self.settling.is_some()
            || deleting.is_some_and(fresh)
            || self.bar.as_ref().is_some_and(|shown| fresh(shown.at))
            || self.marks.values().any(|at| fresh(*at))
    }

    fn check(&self, key: Key, checked: bool, now: Instant) -> f32 {
        let blend = self.marks.get(&key).map_or(1.0, |at| progress(*at, now));
        if checked { blend } else { 1.0 - blend }
    }
}

impl Kuizer {
    /// Where a list item stands in the delete flow right now.
    pub(super) fn look(&self, source: Source, id: u64, path: &Path) -> Look {
        let now = Instant::now();
        let key = (source, id);
        let checked = self.list.selection.contains(&key);
        let within = |paths: &Paths| paths.get(&source).is_some_and(|set| set.contains(path));
        let removing = self
            .session
            .cleaning
            .as_ref()
            .is_some_and(|cleaning| within(&cleaning.paths));
        let (opacity, slide) = if let Some(cleaning) = &self.session.cleaning
            && removing
        {
            (deleting(cleaning.started, now), 0.0)
        } else if let Some(settling) = &self.session.flow.settling
            && within(&settling.paths)
        {
            let blend = progress(settling.at, now);
            if within(&settling.removed) {
                (settling.from * (1.0 - blend), blend)
            } else {
                (settling.from + (1.0 - settling.from) * blend, 0.0)
            }
        } else {
            (1.0, 0.0)
        };
        Look {
            checked: self.session.flow.check(key, checked, now),
            opacity,
            slide,
            deleting: removing,
        }
    }

    /// Lets the rows of a finished cleanup go: the removed ones fade out and leave the report a
    /// step later, or at once with motion off.
    pub(super) fn settle(
        &mut self,
        cleaning: Option<Cleaning>,
        removed: Paths,
        cx: &mut Context<Self>,
    ) {
        let now = Instant::now();
        let (from, paths) = cleaning.map_or((theme::DISABLED, Paths::new()), |cleaning| {
            (deleting(cleaning.started, now), cleaning.paths)
        });
        if let Some(previous) = self.session.flow.settling.take() {
            self.depart(previous.removed, cx);
        }
        if motion::reduced() {
            self.depart(removed, cx);
            return;
        }
        let task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(motion::STEP).await;
            let _ = this.update(cx, |app, cx| {
                if let Some(settling) = app.session.flow.settling.take() {
                    app.depart(settling.removed, cx);
                    cx.notify();
                }
            });
        });
        self.session.flow.settling = Some(Settling {
            at: now,
            from,
            paths,
            removed,
            _task: Some(task),
        });
    }

    /// Takes removed items out of the report, and keeps them out of what a running scan still
    /// delivers.
    fn depart(&mut self, removed: Paths, cx: &Context<Self>) {
        if let Phase::Ready(report) = &mut self.session.phase {
            report.remove(&removed);
        }
        if self.session.cancel.is_some() {
            for (source, paths) in removed {
                self.session.gone.entry(source).or_default().extend(paths);
            }
        }
        self.refresh_stats();
        self.refresh_rows();
        self.refresh_map(cx);
    }

    /// Follows the selection for the summary bar and asks for the next frame while any row or
    /// the bar is still blending.
    pub(super) fn animate_rows(&mut self, window: &mut Window) {
        self.session.flow.observe(self.list.selection.stats());
        if self.session.flow.moving(
            self.session
                .cleaning
                .as_ref()
                .map(|cleaning| cleaning.started),
        ) {
            window.request_animation_frame();
        }
    }

    /// Gives a list item row the look of its place in the delete flow.
    pub(super) fn flow_row<E: Styled + FluentBuilder>(&self, row: E, look: Look) -> E {
        row.bg(look.background())
            .when(look.opacity < 1.0, |this| this.opacity(look.opacity))
            .when(look.slide > 0.0, |this| {
                this.relative().left(unit(-SLIDE * look.slide))
            })
    }
}

fn progress(since: Instant, now: Instant) -> f32 {
    if motion::reduced() {
        return 1.0;
    }
    motion::ease_out(
        now.saturating_duration_since(since).as_secs_f32() / motion::STEP.as_secs_f32(),
    )
}

fn deleting(started: Instant, now: Instant) -> f32 {
    1.0 - (1.0 - theme::DISABLED) * progress(started, now)
}

fn faded(color: Rgba, weight: f32) -> Rgba {
    Rgba {
        a: color.a * weight,
        ..color
    }
}
