use super::*;

impl Kuizer {
    pub(super) fn rescan(&mut self, cx: &mut Context<Self>) {
        let shown = if let Phase::Ready(report) = &self.phase {
            self.stale = Some(report.scanned);
            true
        } else {
            self.set_phase(Phase::Loading(Box::default()), cx);
            self.stale = None;
            self.selection.clear();
            self.rows.clear();
            false
        };
        if let Some(cancel) = self.cancel.take() {
            cancel.cancel();
        }
        self.pending = Part::expected(&self.options);
        self.early.clear();
        self.streamed = false;
        self.progress = Activity::default();
        self.started = Instant::now();
        let cancel = Cancel::new();
        self.cancel = Some(cancel.clone());
        let (sender, mut receiver) = mpsc::unbounded();
        let config = self.config.clone();
        let roots = self.roots.clone();
        let options = self.options.clone();
        let snapshots = self.snapshots.clone();
        let spawned = spawn_worker("kuizer-gui-scan", move || {
            scan_parts(
                &config, &roots, &options, &cancel, &snapshots, &sender, shown,
            );
        });
        if let Err(error) = spawned {
            self.scan_over();
            if shown {
                self.show_toast(Notice::Failed(format!("Could not scan: {error}")), cx);
            } else {
                self.set_phase(Phase::Failed(error), cx);
                cx.notify();
            }
            return;
        }
        self.task = Some(cx.spawn(async move |this, cx| {
            while let Some(first) = receiver.next().await {
                let mut updates = vec![first];
                while let Ok(update) = receiver.try_recv() {
                    updates.push(update);
                }
                let done = updates
                    .iter()
                    .any(|update| matches!(update, Update::Done(_)));
                let updated = this.update(cx, |app, cx| app.take(updates, cx));
                if done || updated.is_err() {
                    return;
                }
            }
            let _ = this.update(cx, |app, cx| {
                app.take(
                    vec![Update::Done(
                        Err("the scan stopped unexpectedly".to_owned()),
                    )],
                    cx,
                );
            });
        }));
    }

    /// Applies everything a running scan sent since the window last heard from it, then
    /// refreshes what depends on the sections that changed, once.
    pub(super) fn take(&mut self, updates: Vec<Update>, cx: &mut Context<Self>) {
        let activity_only = updates
            .iter()
            .all(|update| matches!(update, Update::Activity(_)));
        let mut dropped = Vec::new();
        let (mut rows, mut map, mut sections, mut grown) = (false, false, false, false);
        for update in updates {
            match update {
                Update::Activity(activity) => match &mut self.phase {
                    Phase::Loading(shown) => *shown = activity,
                    _ => self.progress = *activity,
                },
                Update::Cached(report) => {
                    if self.cancel.is_some() && self.cached(*report, &mut dropped) {
                        rows = true;
                        map = true;
                    }
                }
                Update::Loaded(loaded) => {
                    let found = matches!(loaded, Loaded::Found(_));
                    let listed = matches!(loaded, Loaded::Artifacts { .. } | Loaded::Caches(_));
                    if self.loaded(loaded, &mut dropped) {
                        if found {
                            grown = true;
                        } else if listed {
                            rows = true;
                            map = true;
                        } else {
                            sections = true;
                        }
                    }
                }
                Update::Done(Ok(scanned)) => {
                    if matches!(self.phase, Phase::Loading(_)) {
                        self.show_parts(&mut dropped);
                    }
                    if let Phase::Ready(report) = &mut self.phase {
                        report.scanned = scanned;
                    }
                    self.scan_over();
                    rows = true;
                    map = true;
                    if self.options.duplicates && self.search.is_none() {
                        self.find_duplicates(cx);
                    } else if self.tab == Tab::Duplicates {
                        self.load_duplicates(cx);
                    }
                }
                Update::Done(Err(error)) => {
                    self.scan_over();
                    let phase = if error == model::CANCELLED {
                        Phase::Start
                    } else {
                        Phase::Failed(error)
                    };
                    self.set_phase(phase, cx);
                }
            }
        }
        let changed = rows || map || sections || grown;
        if changed {
            if let Phase::Ready(report) = &mut self.phase {
                report.drop_paths(&self.excluded);
            }
            if rows || map || sections {
                self.refresh_stats();
            } else {
                self.grow_stats();
            }
            if rows || grown {
                self.refresh_rows();
            }
            if map || ((rows || grown) && self.map.shown) {
                self.refresh_map();
            }
            if self
                .menu
                .as_ref()
                .is_some_and(|menu| !self.known(&menu.target))
            {
                self.menu = None;
            }
        }
        if !dropped.is_empty() {
            cx.background_spawn(async move { drop(dropped) }).detach();
        }
        if activity_only && !changed && matches!(self.phase, Phase::Ready(_)) {
            if self.content_reads_progress() {
                cx.notify();
            }
            return;
        }
        cx.notify();
    }

    pub(super) fn content_reads_progress(&self) -> bool {
        self.tab != Tab::Overview
            && self
                .tab
                .part()
                .is_some_and(|part| self.pending.contains(&part))
    }

    pub(super) fn scan_over(&mut self) {
        self.cancel = None;
        self.stale = None;
        self.pending.clear();
        self.early.clear();
        self.streamed = false;
    }

    /// Shows the saved scan of these folders while the fresh one runs. Before anything fresh
    /// is in, it is the whole report; after that it only fills the sections still pending.
    pub(super) fn cached(&mut self, report: Report, dropped: &mut Vec<Loaded>) -> bool {
        if matches!(self.phase, Phase::Loading(_)) {
            if let Phase::Loading(activity) = &mut self.phase {
                self.progress = std::mem::take(&mut **activity);
            }
            self.stale = Some(report.scanned);
            self.adopt(report);
            self.apply_early(dropped);
            return true;
        }
        let scanned = report.scanned;
        let mut changed = false;
        for part in report.into_parts() {
            let Some(kind) = part.part() else {
                continue;
            };
            let streaming = kind == Part::Artifacts
                && self.streamed
                && self
                    .report()
                    .is_some_and(|shown| !shown.candidates.is_empty());
            if !self.pending.contains(&kind) || streaming {
                dropped.push(part);
                continue;
            }
            if kind == Part::Artifacts {
                self.streamed = false;
            }
            self.stale = Some(scanned);
            self.replace(part, dropped);
            changed = true;
        }
        changed
    }

    pub(super) fn loaded(&mut self, loaded: Loaded, dropped: &mut Vec<Loaded>) -> bool {
        if matches!(self.phase, Phase::Loading(_)) {
            if !matches!(loaded, Loaded::Found(_) | Loaded::Artifacts { .. }) {
                self.early.push(loaded);
                return false;
            }
            self.show_parts(dropped);
        }
        if let Loaded::Found(batch) = loaded {
            return match (self.streamed, &mut self.phase) {
                (true, Phase::Ready(report)) => {
                    dropped.push(report.apply(Loaded::Found(batch)));
                    true
                }
                _ => {
                    dropped.push(Loaded::Found(batch));
                    false
                }
            };
        }
        if let Some(part) = loaded.part() {
            self.pending.remove(&part);
            if part == Part::Artifacts {
                self.streamed = false;
            }
        }
        self.replace(loaded, dropped);
        true
    }

    /// Leaves the activity view for the results once the first artifacts are in, with every
    /// part that arrived before them.
    pub(super) fn show_parts(&mut self, dropped: &mut Vec<Loaded>) {
        if let Phase::Loading(activity) = &mut self.phase {
            self.progress = std::mem::take(&mut **activity);
        }
        self.phase = Phase::Ready(Box::new(Report::empty(&self.roots, &self.options)));
        self.streamed = true;
        self.apply_early(dropped);
    }

    pub(super) fn apply_early(&mut self, dropped: &mut Vec<Loaded>) {
        for part in std::mem::take(&mut self.early) {
            if let Some(kind) = part.part() {
                self.pending.remove(&kind);
            }
            self.replace(part, dropped);
        }
    }

    /// Puts a part into its section of the report on screen and keeps the selection of that
    /// section on the same paths.
    pub(super) fn replace(&mut self, loaded: Loaded, dropped: &mut Vec<Loaded>) {
        let Phase::Ready(report) = &mut self.phase else {
            dropped.push(loaded);
            return;
        };
        let source = loaded.part().and_then(Part::source);
        let kept: HashSet<PathBuf> = source.map_or_else(HashSet::new, |source| {
            report
                .section(source)
                .filter(|candidate| self.selection.contains(&(source, candidate.id)))
                .map(|candidate| candidate.path.clone())
                .collect()
        });
        dropped.push(report.apply(loaded));
        if let Some(source) = source {
            let stale: Vec<(Source, u64)> = self
                .selection
                .keys()
                .filter(|(selected, _)| *selected == source)
                .copied()
                .collect();
            for key in &stale {
                self.selection.remove(key);
            }
            for candidate in report
                .section(source)
                .filter(|candidate| kept.contains(&candidate.path))
            {
                self.selection.insert((source, candidate.id), candidate);
            }
        }
    }

    /// Changes the phase and drops the report it replaces off the UI thread.
    pub(super) fn set_phase(&mut self, phase: Phase, cx: &mut Context<Self>) {
        if let Phase::Ready(report) = std::mem::replace(&mut self.phase, phase) {
            cx.background_spawn(async move { drop(report) }).detach();
        }
        if !matches!(self.phase, Phase::Ready(_)) {
            self.search = None;
        }
    }

    pub(super) fn adopt(&mut self, report: Report) {
        let selection: HashSet<(Source, u64)> = {
            let kept: HashSet<(Source, &Path)> = self
                .selected()
                .into_iter()
                .map(|(source, candidate)| (source, candidate.path.as_path()))
                .collect();
            Source::ALL
                .into_iter()
                .flat_map(|source| {
                    report
                        .section(source)
                        .map(move |candidate| (source, candidate))
                })
                .filter(|(source, candidate)| kept.contains(&(*source, candidate.path.as_path())))
                .map(|(source, candidate)| (source, candidate.id))
                .collect()
        };
        self.selection = Selection::pending(selection);
        self.phase = Phase::Ready(Box::new(report));
        self.menu = None;
        self.prune();
    }
}

pub(super) fn spawn_worker<T: Send + 'static>(
    name: &str,
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<oneshot::Receiver<T>, String> {
    let (sender, receiver) = oneshot::channel();
    std::thread::Builder::new()
        .name(name.to_owned())
        .spawn(move || {
            let _ = sender.send(work());
        })
        .map_err(|error| format!("the {name} thread did not start: {error}"))?;
    Ok(receiver)
}

pub(super) fn scan_parts(
    config: &Config,
    roots: &[PathBuf],
    options: &ScanOptions,
    cancel: &Cancel,
    snapshots: &Snapshots,
    sender: &mpsc::UnboundedSender<Update>,
    shown: bool,
) {
    let saved = (!shown)
        .then(|| {
            let (snapshots, roots, options, sender) = (
                snapshots.clone(),
                roots.to_vec(),
                options.clone(),
                sender.clone(),
            );
            std::thread::Builder::new()
                .name("kuizer-gui-snapshot".to_owned())
                .spawn(move || {
                    if let Some(report) = snapshots.load(&roots, &options) {
                        let _ = sender.unbounded_send(Update::Cached(Box::new(report)));
                    }
                })
                .ok()
        })
        .flatten();
    let saved = Mutex::new(saved);
    let shown_first = || {
        if let Some(loading) = saved.lock().ok().and_then(|mut saved| saved.take()) {
            let _ = loading.join();
        }
    };
    let tracker = {
        let sender = sender.clone();
        Tracker::new(move |activity| {
            let _ = sender.unbounded_send(Update::Activity(Box::new(activity)));
        })
    };
    let recorder = Recorder::new(roots, options);
    let result = model::load(config, roots, options, cancel, &tracker, &|loaded| {
        recorder.record(&loaded);
        shown_first();
        let _ = sender.unbounded_send(Update::Loaded(loaded));
    });
    shown_first();
    let scanned = SystemTime::now();
    let finished = result.is_ok();
    let _ = sender.unbounded_send(Update::Done(result.map(|()| scanned)));
    if finished {
        recorder.save(snapshots, scanned);
    }
}
