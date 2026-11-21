//! The delete flow as motion: a row blends from idle to marked to confirming to deleting to
//! gone, each step starting from where the previous one left it.

use gpui::Rgba;

use super::*;

/// How long one step of the delete flow takes to blend into the next.
pub(super) const STEP: Duration = Duration::from_millis(140);
const SLIDE: f32 = space::XXL;
const MARKS: usize = 64;

type Key = (Source, u64);
type Paths = HashMap<Source, HashSet<PathBuf>>;

/// A row part way through the delete flow. Each field runs from 0 to 1, except `opacity`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Look {
    pub(super) checked: f32,
    pub(super) confirming: f32,
    pub(super) opacity: f32,
    pub(super) slide: f32,
}

impl Look {
    fn background(&self) -> Rgba {
        mix(
            faded(theme::selected(), self.checked),
            theme::danger_soft(),
            self.confirming,
        )
    }
}

/// When the confirmation last opened or closed, and whether it covers one item or the
/// selection.
struct Shown {
    at: Instant,
    open: bool,
    only: Option<Key>,
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
    confirm: Option<Shown>,
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
            .retain(|_, at| now.saturating_duration_since(*at) < STEP);
        if self.marks.len() < MARKS {
            self.marks.insert(key, now);
        }
    }

    /// Notes the confirmation opening, closing or following its item to a new id.
    pub(super) fn observe(&mut self, confirm: Option<Option<Key>>) {
        let open = confirm.is_some();
        match (&mut self.confirm, confirm) {
            (Some(shown), Some(only)) if shown.open => shown.only = only,
            (Some(shown), None) if !shown.open => {}
            (None, None) => {}
            (shown, covered) => {
                let only = match covered {
                    Some(only) => only,
                    None => shown.as_ref().and_then(|shown| shown.only),
                };
                *shown = Some(Shown {
                    at: Instant::now(),
                    open,
                    only,
                });
            }
        }
    }

    /// Whether a row is still blending, so the window draws another frame.
    fn moving(&self, deleting: Option<Instant>) -> bool {
        if motion::reduced() {
            return false;
        }
        let now = Instant::now();
        let fresh = |at: Instant| now.saturating_duration_since(at) < STEP;
        self.settling.is_some()
            || deleting.is_some_and(fresh)
            || self.confirm.as_ref().is_some_and(|shown| fresh(shown.at))
            || self.marks.values().any(|at| fresh(*at))
    }

    fn check(&self, key: Key, checked: bool, now: Instant) -> f32 {
        let blend = self.marks.get(&key).map_or(1.0, |at| progress(*at, now));
        if checked { blend } else { 1.0 - blend }
    }

    fn confirming(&self, key: Key, checked: bool, now: Instant) -> f32 {
        let Some(shown) = &self.confirm else {
            return 0.0;
        };
        let covers = shown.only.map_or(checked, |only| only == key);
        if !covers {
            return 0.0;
        }
        let blend = progress(shown.at, now);
        if shown.open { blend } else { 1.0 - blend }
    }
}

impl Kuizer {
    /// Where a list item stands in the delete flow right now.
    pub(super) fn look(&self, source: Source, id: u64, path: &Path) -> Look {
        let now = Instant::now();
        let key = (source, id);
        let checked = self.selection.contains(&key);
        let within = |paths: &Paths| paths.get(&source).is_some_and(|set| set.contains(path));
        let (opacity, slide) = if let Some(cleaning) = &self.cleaning
            && within(&cleaning.paths)
        {
            (deleting(cleaning.started, now), 0.0)
        } else if let Some(settling) = &self.flow.settling
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
            checked: self.flow.check(key, checked, now),
            confirming: self.flow.confirming(key, checked, now),
            opacity,
            slide,
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
        if let Some(previous) = self.flow.settling.take() {
            self.depart(previous.removed);
        }
        if motion::reduced() {
            self.depart(removed);
            return;
        }
        let task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(STEP).await;
            let _ = this.update(cx, |app, cx| {
                if let Some(settling) = app.flow.settling.take() {
                    app.depart(settling.removed);
                    cx.notify();
                }
            });
        });
        self.flow.settling = Some(Settling {
            at: now,
            from,
            paths,
            removed,
            _task: Some(task),
        });
    }

    /// Takes removed items out of the report, and keeps them out of what a running scan still
    /// delivers.
    fn depart(&mut self, removed: Paths) {
        if let Phase::Ready(report) = &mut self.phase {
            report.remove(&removed);
        }
        if self.cancel.is_some() {
            for (source, paths) in removed {
                self.gone.entry(source).or_default().extend(paths);
            }
        }
        self.refresh_stats();
        self.refresh_rows();
        self.refresh_map();
    }

    /// Follows the confirmation and asks for the next frame while any row is still blending.
    pub(super) fn animate_rows(&mut self, window: &mut Window) {
        let confirm = match &self.dialog {
            Some(Dialog::Confirm { only, .. }) => Some(*only),
            _ => None,
        };
        self.flow.observe(confirm);
        if self
            .flow
            .moving(self.cleaning.as_ref().map(|cleaning| cleaning.started))
        {
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
    motion::ease_out(now.saturating_duration_since(since).as_secs_f32() / STEP.as_secs_f32())
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

fn mix(from: Rgba, to: Rgba, weight: f32) -> Rgba {
    let blend = |a: f32, b: f32| a + (b - a) * weight;
    Rgba {
        r: blend(from.r, to.r),
        g: blend(from.g, to.g),
        b: blend(from.b, to.b),
        a: blend(from.a, to.a),
    }
}
