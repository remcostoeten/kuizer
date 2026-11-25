use super::*;

impl Kuizer {
    pub(super) fn rescan(&mut self, cx: &mut Context<Self>) {
        let shown = if let Phase::Ready(report) = &self.session.phase {
            self.session.stale = Some(report.scanned);
            true
        } else {
            self.set_phase(Phase::Loading(Box::default()), cx);
            self.session.stale = None;
            self.list.selection.clear();
            self.list.rows.clear();
            false
        };
        if let Some(cancel) = self.session.cancel.take() {
            cancel.cancel();
        }
        self.session.pending = Part::expected(&self.session.options);
        self.session.early.clear();
        self.session.streamed = false;
        self.session.refreshed.clear();
        self.session.gone.clear();
        self.session.history_fresh = false;
        self.session.progress = Activity::default();
        self.session.started = Instant::now();
        let cancel = Cancel::new();
        self.session.cancel = Some(cancel.clone());
        let (sender, mut receiver) = mpsc::unbounded();
        let config = self.config.clone();
        let roots = self.session.roots.clone();
        let options = self.session.options.clone();
        let snapshots = self.session.snapshots.clone();
        self.session.forgotten = Forgotten::default();
        let forgotten = self.session.forgotten.clone();
        let excluded = self.session.excluded.clone();
        self.session.cached_excluded = excluded.len();
        let spawned = spawn_worker("kuizer-gui-scan", move || {
            scan_parts(
                &config, &roots, &options, &cancel, &snapshots, forgotten, &sender, shown, excluded,
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
        self.session.task = Some(cx.spawn(async move |this, cx| {
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
                Update::Activity(activity) => match &mut self.session.phase {
                    Phase::Loading(shown) => *shown = activity,
                    _ => self.session.progress = *activity,
                },
                Update::Cached(mut report) => {
                    report.drop_paths(
                        self.session
                            .excluded
                            .get(self.session.cached_excluded..)
                            .unwrap_or(&[]),
                    );
                    if !self.session.gone.is_empty() {
                        report.remove(&self.session.gone);
                    }
                    if self.session.cancel.is_some() && self.cached(*report, &mut dropped) {
                        rows = true;
                        map = true;
                    }
                }
                Update::Loaded(mut loaded) => {
                    loaded.without(&self.session.excluded, &self.session.gone);
                    let found = matches!(loaded, Loaded::Found(_));
                    let listed = matches!(
                        loaded,
                        Loaded::Artifacts { .. } | Loaded::ArtifactsDone { .. } | Loaded::Caches(_)
                    );
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
                    if matches!(self.session.phase, Phase::Loading(_)) {
                        self.show_parts(&mut dropped);
                    }
                    if let Phase::Ready(report) = &mut self.session.phase {
                        report.scanned = scanned;
                    }
                    self.scan_over();
                    rows = true;
                    map = true;
                    if self.session.options.duplicates && self.session.search.is_none() {
                        self.find_duplicates(cx);
                    } else if self.list.tab == Tab::Duplicates {
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
            if rows || map || sections {
                self.refresh_stats();
            } else {
                self.grow_stats();
            }
            if rows || grown {
                self.refresh_rows();
            }
            if map || rows || grown {
                self.refresh_map(cx);
            }
            if self
                .overlays
                .menu
                .as_ref()
                .is_some_and(|menu| !self.known(&menu.target))
            {
                self.overlays.menu = None;
            }
        }
        if !dropped.is_empty() {
            cx.background_spawn(async move { drop(dropped) }).detach();
        }
        if activity_only && !changed && matches!(self.session.phase, Phase::Ready(_)) {
            if self.content_reads_progress() {
                cx.notify();
            }
            return;
        }
        cx.notify();
    }

    pub(super) fn content_reads_progress(&self) -> bool {
        self.list.tab != Tab::Overview
            && self
                .list
                .tab
                .part()
                .is_some_and(|part| self.session.pending.contains(&part))
    }

    pub(super) fn scan_over(&mut self) {
        self.session.cancel = None;
        self.session.stale = None;
        self.session.pending.clear();
        self.session.early.clear();
        self.session.streamed = false;
        self.session.refreshed.clear();
    }

    /// Shows the saved scan of these folders while the fresh one runs. Before anything fresh
    /// is in, it is the whole report; after that it only fills the sections still pending.
    pub(super) fn cached(&mut self, report: Report, dropped: &mut Vec<Loaded>) -> bool {
        if matches!(self.session.phase, Phase::Loading(_)) {
            if let Phase::Loading(activity) = &mut self.session.phase {
                self.session.progress = std::mem::take(&mut **activity);
            }
            self.session.stale = Some(report.scanned);
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
            let streaming = kind == Part::Artifacts && self.session.streamed;
            let kept = kind == Part::History && self.session.history_fresh;
            if !self.session.pending.contains(&kind) || streaming || kept {
                dropped.push(part);
                continue;
            }
            if kind == Part::Artifacts {
                self.session.streamed = false;
            }
            self.session.stale = Some(scanned);
            self.replace(part, dropped);
            changed = true;
        }
        changed
    }

    pub(super) fn loaded(&mut self, mut loaded: Loaded, dropped: &mut Vec<Loaded>) -> bool {
        if matches!(self.session.phase, Phase::Loading(_)) {
            if !matches!(
                loaded,
                Loaded::Found(_) | Loaded::Artifacts { .. } | Loaded::ArtifactsDone { .. }
            ) {
                self.session.early.push(loaded);
                return false;
            }
            self.show_parts(dropped);
        }
        if let Loaded::Found(batch) = loaded {
            return match (self.session.streamed, &mut self.session.phase) {
                (true, Phase::Ready(report)) => {
                    dropped.push(report.apply(Loaded::Found(batch)));
                    true
                }
                (false, Phase::Ready(_)) => {
                    self.session.refreshed.extend(batch);
                    false
                }
                _ => {
                    dropped.push(Loaded::Found(batch));
                    false
                }
            };
        }
        if let Some(part) = loaded.part() {
            self.session.pending.remove(&part);
            if part == Part::Artifacts {
                if !self.session.streamed
                    && let Loaded::ArtifactsDone {
                        issues,
                        summary,
                        disks,
                    } = loaded
                {
                    let mut candidates = std::mem::take(&mut self.session.refreshed);
                    kuizer::scan::sort_candidates(&mut candidates, kuizer::scan::SortKey::Size);
                    loaded = Loaded::Artifacts {
                        candidates,
                        issues,
                        summary,
                        disks,
                    };
                }
                self.session.streamed = false;
            }
            if part == Part::History && self.session.history_fresh {
                dropped.push(loaded);
                return false;
            }
        }
        self.replace(loaded, dropped);
        true
    }

    /// Leaves the activity view for the results once the first artifacts are in, with every
    /// part that arrived before them.
    pub(super) fn show_parts(&mut self, dropped: &mut Vec<Loaded>) {
        if let Phase::Loading(activity) = &mut self.session.phase {
            self.session.progress = std::mem::take(&mut **activity);
        }
        self.session.phase = Phase::Ready(Box::new(Report::empty(
            &self.session.roots,
            &self.session.options,
        )));
        self.session.streamed = true;
        self.apply_early(dropped);
    }

    pub(super) fn apply_early(&mut self, dropped: &mut Vec<Loaded>) {
        for part in std::mem::take(&mut self.session.early) {
            if let Some(kind) = part.part() {
                self.session.pending.remove(&kind);
            }
            self.replace(part, dropped);
        }
    }

    /// Puts a part into its section of the report on screen and keeps the selection of that
    /// section on the same paths.
    pub(super) fn replace(&mut self, loaded: Loaded, dropped: &mut Vec<Loaded>) {
        let Phase::Ready(report) = &mut self.session.phase else {
            dropped.push(loaded);
            return;
        };
        let source = loaded.part().and_then(Part::source);
        let kept: HashSet<PathBuf> = source.map_or_else(HashSet::new, |source| {
            report
                .section(source)
                .filter(|candidate| self.list.selection.contains(&(source, candidate.id)))
                .map(|candidate| candidate.path.clone())
                .collect()
        });
        dropped.push(report.apply(loaded));
        if let Some(source) = source {
            let stale: Vec<(Source, u64)> = self
                .list
                .selection
                .keys()
                .filter(|(selected, _)| *selected == source)
                .copied()
                .collect();
            for key in &stale {
                self.list.selection.remove(key);
            }
            for candidate in report
                .section(source)
                .filter(|candidate| kept.contains(&candidate.path))
            {
                self.list
                    .selection
                    .insert((source, candidate.id), candidate);
            }
        }
    }

    /// Changes the phase and drops the report it replaces off the UI thread.
    pub(super) fn set_phase(&mut self, phase: Phase, cx: &mut Context<Self>) {
        if let Phase::Ready(report) = std::mem::replace(&mut self.session.phase, phase) {
            cx.background_spawn(async move { drop(report) }).detach();
        }
        if !matches!(self.session.phase, Phase::Ready(_)) {
            self.session.search = None;
            self.clear_map_tree(cx);
        }
    }

    pub(super) fn adopt(&mut self, report: Report) {
        let selection: HashSet<(Source, u64)> = {
            let kept: HashSet<(Source, &Path)> = self
                .selected()
                .into_iter()
                .map(|(source, candidate)| (source, candidate.path.as_path()))
                .collect();
            if kept.is_empty() {
                HashSet::new()
            } else {
                Source::ALL
                    .into_iter()
                    .flat_map(|source| {
                        report
                            .section(source)
                            .map(move |candidate| (source, candidate))
                    })
                    .filter(|(source, candidate)| {
                        kept.contains(&(*source, candidate.path.as_path()))
                    })
                    .map(|(source, candidate)| (source, candidate.id))
                    .collect()
            }
        };
        self.list.selection = Selection::pending(selection);
        self.session.phase = Phase::Ready(Box::new(report));
        self.overlays.menu = None;
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

#[allow(
    clippy::too_many_arguments,
    reason = "the scan thread takes everything it works with by value or reference"
)]
pub(super) fn scan_parts(
    config: &Config,
    roots: &[PathBuf],
    options: &ScanOptions,
    cancel: &Cancel,
    snapshots: &Snapshots,
    forgotten: Forgotten,
    sender: &mpsc::UnboundedSender<Update>,
    shown: bool,
    excluded: Vec<PathBuf>,
) {
    if !shown {
        let (snapshots, roots, options, sender) = (
            snapshots.clone(),
            roots.to_vec(),
            options.clone(),
            sender.clone(),
        );
        let _ = std::thread::Builder::new()
            .name("kuizer-gui-snapshot".to_owned())
            .spawn(move || {
                if let Some(mut report) = snapshot::load(&snapshots, &roots, &options) {
                    report.drop_paths(&excluded);
                    let _ = sender.unbounded_send(Update::Cached(Box::new(report)));
                }
            });
    }
    let tracker = {
        let sender = sender.clone();
        Tracker::new(move |activity| {
            let _ = sender.unbounded_send(Update::Activity(Box::new(activity)));
        })
    };
    let recorder = Recorder::new(roots, options, forgotten);
    let result = model::load(config, roots, options, cancel, &tracker, &|loaded| {
        recorder.record(&loaded);
        let _ = sender.unbounded_send(Update::Loaded(loaded));
    });
    let scanned = SystemTime::now();
    let finished = result.is_ok();
    let _ = sender.unbounded_send(Update::Done(result.map(|()| scanned)));
    if finished {
        recorder.save(snapshots, scanned);
    }
}
