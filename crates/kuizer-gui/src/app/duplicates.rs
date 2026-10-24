use super::*;

impl Kuizer {
    /// @name Kuizer::find_duplicates
    /// @description Starts a duplicate search over the scanned folders beside the results on
    /// screen, and puts what it finds into the Duplicates section when it is done. Running it
    /// while a search is going stops listening to that search.
    ///
    /// @example
    /// app.find_duplicates(cx);
    pub(super) fn find_duplicates(&mut self, cx: &mut Context<Self>) {
        if self.search.take().is_some() {
            cx.notify();
            return;
        }
        if self.report().is_none() {
            return;
        }
        self.search_error = None;
        let (sender, mut receiver) = mpsc::unbounded();
        let config = self.config.clone();
        let roots = self.roots.clone();
        let options = self.options.clone();
        let snapshots = self.snapshots.clone();
        let spawned = spawn_worker("kuizer-gui-duplicates", move || {
            let mut told: Option<Instant> = None;
            let found = model::find_duplicates(&config, &roots, &options, |stage| {
                let quiet = matches!(stage, dupes::Stage::Walking { .. })
                    && told.is_some_and(|told| told.elapsed() < POLL);
                if !quiet {
                    told = Some(Instant::now());
                    let _ = sender.unbounded_send(Searched::Stage(stage));
                }
            });
            let staged = found.as_ref().ok().map(|found| {
                let duplicate_options = dupes::Options::from_config(&options.apply(&config));
                snapshots.stage_duplicates(&roots, duplicate_options, found)
            });
            let _ = sender.unbounded_send(Searched::Done(found));
            if let Some(staged) = staged {
                staged.save();
            }
        });
        if let Err(error) = spawned {
            self.search_error = Some(error);
            cx.notify();
            return;
        }
        let task = cx.spawn(async move |this, cx| {
            while let Some(first) = receiver.next().await {
                let mut last = first;
                while let Ok(next) = receiver.try_recv() {
                    last = next;
                }
                let done = matches!(last, Searched::Done(_));
                let updated = this.update(cx, |app, cx| app.searched(last, cx));
                if done || updated.is_err() {
                    return;
                }
            }
            let _ = this.update(cx, |app, cx| {
                app.searched(
                    Searched::Done(Err("the duplicate search stopped unexpectedly".to_owned())),
                    cx,
                );
            });
        });
        self.search = Some(Search {
            roots: self.roots.clone(),
            stage: None,
            _task: task,
        });
        cx.notify();
    }

    pub(super) fn searched(&mut self, searched: Searched, cx: &mut Context<Self>) {
        let Some(search) = &mut self.search else {
            return;
        };
        match searched {
            Searched::Stage(stage) => search.stage = Some(stage),
            Searched::Done(found) => {
                let roots = std::mem::take(&mut search.roots);
                self.search = None;
                match found {
                    Ok(found) => self.show_duplicates(&roots, found, cx),
                    Err(error) => self.search_error = Some(error),
                }
            }
        }
        cx.notify();
    }

    pub(super) fn show_duplicates(
        &mut self,
        roots: &[PathBuf],
        found: Duplicates,
        cx: &mut Context<Self>,
    ) {
        if let Phase::Ready(report) = &mut self.phase
            && report.roots == roots
        {
            if let Some(old) = report.duplicates.replace(found) {
                cx.background_spawn(async move { drop(old) }).detach();
            }
            report.drop_paths(&self.excluded);
            self.report_generation += 1;
        }
    }

    /// Reads the duplicates the last search under these folders found, off the UI thread, when
    /// the Duplicates section is opened and the report on screen has none yet.
    pub(super) fn load_duplicates(&mut self, cx: &mut Context<Self>) {
        let Some(report) = self.report() else {
            return;
        };
        if report.duplicates.is_some() || self.search.is_some() {
            return;
        }
        let roots = report.roots.clone();
        let snapshots = self.snapshots.clone();
        let options = dupes::Options::from_config(&self.options.apply(&self.config));
        let spawned = spawn_worker("kuizer-gui-saved-duplicates", move || {
            snapshots.load_duplicates(&roots, options)
        });
        let receiver = match spawned {
            Ok(receiver) => receiver,
            Err(error) => {
                self.tell(
                    Notice::Failed(format!("Could not read the saved duplicates: {error}")),
                    cx,
                );
                return;
            }
        };
        let roots = self.roots.clone();
        cx.spawn(async move |this, cx| {
            if let Ok(Some(found)) = receiver.await {
                let _ = this.update(cx, |app, cx| {
                    if app.search.is_none()
                        && app
                            .report()
                            .is_some_and(|report| report.duplicates.is_none())
                    {
                        app.show_duplicates(&roots, found, cx);
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    pub(super) fn render_duplicate_search(&self, cx: &mut Context<Self>) -> AnyElement {
        let (text, label) = match (&self.search, &self.search_error) {
            (Some(search), _) => (
                match search.stage {
                    None => "Starting the duplicate search".to_owned(),
                    Some(dupes::Stage::Walking { files }) => {
                        format!("Collecting files, {} so far", activity::grouped(files))
                    }
                    Some(dupes::Stage::Hashing { files }) => {
                        format!(
                            "Hashing {} files of equal size",
                            activity::grouped(files as u64)
                        )
                    }
                    Some(dupes::Stage::Verifying { groups }) => format!(
                        "Comparing {} groups byte by byte",
                        activity::grouped(groups as u64)
                    ),
                },
                "Stop the search",
            ),
            (None, Some(error)) => (
                format!("The duplicate search failed: {error}"),
                "Find duplicates",
            ),
            (None, None) => (
                "Duplicate files are found on request. It reads every file, so it takes a while"
                    .to_owned(),
                "Find duplicates",
            ),
        };
        let tip = Tooltip::new("Hash files under the scanned folders to find identical copies")
            .keys(&[hint(&Command::FindDuplicates)])
            .build();
        centered(
            div()
                .debug_selector(|| "duplicate-search".to_owned())
                .flex()
                .flex_col()
                .items_center()
                .gap(unit(space::L))
                .child(ui::empty(text))
                .child(
                    ui::button("find-duplicates", label, ButtonKind::Secondary, true)
                        .tip(tip)
                        .on_click(cx.listener(|app, _: &ClickEvent, _, cx| {
                            app.find_duplicates(cx);
                        })),
                ),
        )
    }

    pub(super) fn render_duplicates(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(report) = self.report() else {
            return div().into_any_element();
        };
        let Some(found) = &report.duplicates else {
            return self.render_duplicate_search(cx);
        };
        let wasted = found.reclaimable();
        let copies: usize = found.groups.iter().map(|group| group.members.len()).sum();
        let largest = found
            .groups
            .first()
            .map_or(0, kuizer::dupes::Group::reclaimable);
        let cursor = self.cursor_target();
        let needle = self.needle(Tab::Duplicates);
        scroll("duplicates")
            .debug_selector(|| "duplicate-list".to_owned())
            .track_scroll(&self.scroll_for(Tab::Duplicates))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(unit(space::L))
                    .child(
                        ui::stat(
                            "Reclaimable",
                            theme::bytes(wasted),
                            "by keeping one copy of each",
                        )
                        .id("stat-reclaimable")
                        .tip(tooltip::term(Term::Reclaimable)),
                    )
                    .child(
                        ui::stat(
                            "Groups",
                            found.groups.len().to_string(),
                            format!("{copies} files in total"),
                        )
                        .id("stat-groups")
                        .tip(tooltip::term(Term::Groups)),
                    )
                    .child(
                        ui::stat(
                            "Files compared",
                            found.files.to_string(),
                            format!("in {:.1}s", found.duration.as_secs_f32()),
                        )
                        .id("stat-compared")
                        .tip(tooltip::term(Term::FilesCompared)),
                    ),
            )
            .child(
                ui::card(if found.groups.len() > DUPLICATE_GROUPS {
                    format!(
                        "Largest {DUPLICATE_GROUPS} of {} groups",
                        found.groups.len()
                    )
                } else {
                    "Groups".to_owned()
                })
                .children(
                    found
                        .groups
                        .iter()
                        .take(DUPLICATE_GROUPS)
                        .enumerate()
                        .filter(|(_, group)| {
                            group
                                .members
                                .iter()
                                .any(|member| found_path(needle.as_deref(), &member.path))
                        })
                        .map(|(index, group)| {
                            let members = SharedString::from(format!("dupe-{index}"));
                            div()
                                .flex()
                                .flex_col()
                                .gap(unit(space::XS))
                                .pt(unit(space::M))
                                .border_t_1()
                                .border_color(theme::border())
                                .child(
                                    ui::bar_row(
                                        format!(
                                            "{} copies of {}{}",
                                            group.members.len(),
                                            theme::bytes(group.size),
                                            if group.verified {
                                                ""
                                            } else {
                                                ", not byte-verified"
                                            }
                                        ),
                                        format!(
                                            "{} reclaimable",
                                            theme::bytes(group.reclaimable())
                                        ),
                                        theme::share(group.reclaimable(), largest),
                                        theme::bar(),
                                    )
                                    .id(("group", index))
                                    .tip(group_details(group)),
                                )
                                .children(
                                    group
                                        .members
                                        .iter()
                                        .enumerate()
                                        .filter(|(_, member)| {
                                            found_path(needle.as_deref(), &member.path)
                                        })
                                        .map(|(position, member)| {
                                            let row = div()
                                                .flex()
                                                .gap(unit(space::M))
                                                .text_color(theme::muted())
                                                .debug_selector(move || {
                                                    format!("duplicate-{index}-{position}")
                                                });
                                            let target = Target::Duplicate(member.path.clone());
                                            self.menu_row(row, target, cursor.as_ref(), cx)
                                                .child(
                                                    div()
                                                        .id((members.clone(), position))
                                                        .flex_1()
                                                        .min_w_0()
                                                        .truncate()
                                                        .tip(tooltip::path(&member.path))
                                                        .child(model::short_path(
                                                            &member.path,
                                                            &report.roots,
                                                        )),
                                                )
                                                .child(
                                                    div()
                                                        .flex_none()
                                                        .text_color(theme::faint())
                                                        .child(theme::age(member.modified)),
                                                )
                                        }),
                                )
                        }),
                )
                .when(found.groups.is_empty(), |this| {
                    this.child(ui::empty("No duplicate files found"))
                }),
            )
            .into_any_element()
    }
}

pub(super) fn group_details(
    group: &kuizer::dupes::Group,
) -> impl Fn(&mut Window, &mut App) -> gpui::AnyView + use<> {
    let details = Tooltip::details("Duplicate group")
        .row("Copies", group.members.len().to_string())
        .row("Size of each", theme::bytes(group.size))
        .row("Reclaimable", theme::bytes(group.reclaimable()));
    if group.verified {
        details.build()
    } else {
        details.note(Term::Unverified.text()).build()
    }
}
