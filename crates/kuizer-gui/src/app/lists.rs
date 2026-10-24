use std::cell::RefCell;

use gpui::UniformList;
use kuizer::caches::KnownCache;
use kuizer::docker;
use kuizer::toolchains::Toolchain;

use super::*;

pub(super) type Slot = (usize, usize);

#[derive(Clone, Default)]
pub(super) struct Listed {
    pub slots: Rc<[Slot]>,
    pub targets: Rc<[Target]>,
}

struct ListKey {
    generation: u64,
    len: usize,
    text: String,
}

#[derive(Default)]
pub(super) struct Lists(RefCell<HashMap<Tab, (ListKey, Listed)>>);

pub(super) fn cache_matches(
    needle: Option<&str>,
    cache: &KnownCache,
    candidate: &Candidate,
) -> bool {
    found(
        needle,
        &[
            candidate.path.to_string_lossy().as_ref(),
            cache.name,
            cache.tool,
        ],
    )
}

pub(super) fn toolchain_matches(
    needle: Option<&str>,
    toolchain: &Toolchain,
    candidate: &Candidate,
) -> bool {
    found(
        needle,
        &[
            candidate.path.to_string_lossy().as_ref(),
            toolchain.tool.name(),
            toolchain.runtime.as_str(),
            toolchain.version.as_str(),
        ],
    )
}

pub(super) fn docker_matches(
    needle: Option<&str>,
    item: &docker::Item,
    candidate: &Candidate,
) -> bool {
    found(
        needle,
        &[
            candidate.path.to_string_lossy().as_ref(),
            item.engine.as_str(),
            item.object.name(),
            item.label.as_str(),
            item.note.as_str(),
        ],
    )
}

fn list_len(report: &Report, tab: Tab) -> usize {
    match tab {
        Tab::Overview | Tab::Artifacts => 0,
        Tab::Caches => report.caches.len(),
        Tab::Toolchains => report.toolchains.len(),
        Tab::Docker => report.docker.len(),
        Tab::Repos => report.repos.len(),
        Tab::Duplicates => report
            .duplicates
            .as_ref()
            .map_or(0, |found| found.groups.len()),
        Tab::Issues => report.issues.len(),
        Tab::History => report
            .history
            .as_ref()
            .map_or(0, |view| view.history.runs.len()),
    }
}

fn list_rows(report: &Report, tab: Tab, needle: Option<&str>) -> Listed {
    let mut slots: Vec<Slot> = Vec::new();
    let mut targets: Vec<Target> = Vec::new();
    match tab {
        Tab::Overview | Tab::Artifacts => {}
        Tab::Caches => {
            for (at, (cache, candidate)) in report.caches.iter().enumerate() {
                if cache_matches(needle, cache, candidate) {
                    slots.push((at, 0));
                    targets.push(Target::Item(
                        Source::Cache,
                        candidate.id,
                        candidate.path.clone(),
                    ));
                }
            }
        }
        Tab::Toolchains => {
            for (at, (toolchain, candidate)) in report.toolchains.iter().enumerate() {
                if toolchain_matches(needle, toolchain, candidate) {
                    slots.push((at, 0));
                    targets.push(Target::Item(
                        Source::Toolchain,
                        candidate.id,
                        candidate.path.clone(),
                    ));
                }
            }
        }
        Tab::Docker => {
            for (at, (item, candidate)) in report.docker.iter().enumerate() {
                if docker_matches(needle, item, candidate) {
                    slots.push((at, 0));
                    targets.push(Target::Item(
                        Source::Docker,
                        candidate.id,
                        candidate.path.clone(),
                    ));
                }
            }
        }
        Tab::Duplicates => {
            let groups = report
                .duplicates
                .iter()
                .flat_map(|found| found.groups.iter().take(DUPLICATE_GROUPS));
            for (group_at, group) in groups.enumerate() {
                for (member_at, member) in group.members.iter().enumerate() {
                    if found_path(needle, &member.path) {
                        slots.push((group_at, member_at));
                        targets.push(Target::Duplicate(member.path.clone()));
                    }
                }
            }
        }
        Tab::Issues => {
            for (at, issue) in report.issues.iter().take(ISSUE_LIMIT).enumerate() {
                let path = issue.path.to_string_lossy();
                if found(needle, &[path.as_ref(), issue.message.as_str()]) {
                    slots.push((at, 0));
                    targets.push(Target::Issue(issue.path.clone()));
                }
            }
        }
        Tab::Repos => {
            for (at, repository) in report.repos.iter().enumerate() {
                if repos::repo_matches(needle, repository) {
                    slots.push((at, 0));
                    targets.push(Target::Repo(repository.root.clone()));
                }
            }
        }
        Tab::History => {
            if let Ok(view) = &report.history {
                let runs = view.history.runs.iter().enumerate().rev();
                for (at, run) in runs.filter(|(_, run)| run.action == Action::Delete) {
                    if shows_run(needle, run) {
                        slots.push((at, 0));
                        targets.push(Target::Run(run.id, run.command.clone()));
                    }
                }
            }
        }
    }
    Listed {
        slots: slots.into(),
        targets: targets.into(),
    }
}

impl Kuizer {
    pub(super) fn listed(&self, tab: Tab) -> Listed {
        let Some(report) = self.report() else {
            return Listed::default();
        };
        let text = self.filters.get(&tab).map_or("", |text| text.trim());
        let (generation, len) = (self.report_generation, list_len(report, tab));
        if let Some((key, listed)) = self.lists.0.borrow().get(&tab)
            && key.generation == generation
            && key.len == len
            && key.text == text
        {
            return listed.clone();
        }
        let needle = self.needle(tab);
        let listed = list_rows(report, tab, needle.as_deref());
        let key = ListKey {
            generation,
            len,
            text: text.to_owned(),
        };
        self.lists.0.borrow_mut().insert(tab, (key, listed.clone()));
        listed
    }

    /// Every row of a list in the order it is drawn, after its filters.
    pub(super) fn targets(&self, tab: Tab) -> Rc<[Target]> {
        let Some(report) = self.report() else {
            return Rc::default();
        };
        if tab != Tab::Artifacts {
            return self.listed(tab).targets;
        }
        self.rows
            .iter()
            .map(|&index| {
                let candidate = &report.candidates[index];
                Target::Item(Source::Artifact, candidate.id, candidate.path.clone())
            })
            .collect()
    }

    pub(super) fn list_scroll(&self, tab: Tab) -> Option<&UniformListScrollHandle> {
        match tab {
            Tab::Artifacts => Some(&self.artifact_scroll),
            _ => self.list_scrolls.get(&tab),
        }
    }

    pub(super) fn table_list(
        &self,
        id: &'static str,
        tab: Tab,
        count: usize,
        rows: impl Fn(&mut Self, Range<usize>, &mut Context<Self>) -> Vec<Stateful<Div>> + 'static,
        cx: &mut Context<Self>,
    ) -> UniformList {
        uniform_list(
            id,
            count,
            cx.processor(move |app, range: Range<usize>, _window, cx| rows(app, range, cx)),
        )
        .track_scroll(self.list_scroll(tab).cloned().unwrap_or_default())
        .flex_1()
        .w_full()
    }

    pub(super) fn render_caches(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(report) = self.report() else {
            return div().into_any_element();
        };
        let header = table_header()
            .child(div().w(unit(size::CHECK)))
            .child(head(col::M, "Size", Term::Size).text_right())
            .child(head(col::S, "Safety", Term::Safety))
            .child(head(col::M, "Tool", Term::Tool))
            .child(column(col::XXL).child("Cache"))
            .child(head(col::XS, "Changed", Term::Changed).text_right())
            .child(div().flex_1().child("Path"));
        let count = self.listed(Tab::Caches).slots.len();
        let list = self
            .table_list(
                "caches",
                Tab::Caches,
                count,
                |app, range, cx| app.cache_rows(range, cx),
                cx,
            )
            .debug_selector(|| "cache-list".to_owned());
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(
                ui::toolbar()
                    .child(format!(
                        "{} in {} caches",
                        theme::bytes(report.cache_total(None)),
                        report.caches.len()
                    ))
                    .child(
                        div()
                            .id("caches-safe")
                            .tip(tooltip::term(Term::Safe))
                            .child(format!(
                                "safe {}",
                                theme::bytes(report.cache_total(Some(Safety::Safe)))
                            )),
                    )
                    .child(
                        div()
                            .id("caches-review")
                            .text_color(theme::review())
                            .tip(tooltip::term(Term::Review))
                            .child(format!(
                                "review {}",
                                theme::bytes(report.cache_total(Some(Safety::Review)))
                            )),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_color(theme::faint())
                            .child("Safe caches are rebuilt on the next install or build"),
                    ),
            )
            .child(header)
            .child(if report.caches.is_empty() {
                div()
                    .flex_1()
                    .child(ui::empty(if report.options.caches {
                        "No global caches found"
                    } else {
                        "Global caches were skipped for this scan"
                    }))
                    .into_any_element()
            } else if count == 0 {
                div().flex_1().into_any_element()
            } else {
                list.into_any_element()
            })
            .into_any_element()
    }

    fn cache_rows(&self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<Stateful<Div>> {
        let Some(report) = self.report() else {
            return Vec::new();
        };
        let listed = self.listed(Tab::Caches);
        let cursor = self.cursor_target();
        range
            .filter_map(|row| {
                let (cache, candidate) = report.caches.get(listed.slots.get(row)?.0)?;
                Some(self.cache_row(row, cache, candidate, cursor.as_ref(), cx))
            })
            .collect()
    }

    fn cache_row(
        &self,
        index: usize,
        cache: &KnownCache,
        candidate: &Candidate,
        cursor: Option<&Target>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let id = candidate.id;
        let checked = self.selection.contains(&(Source::Cache, id));
        let review = candidate.rule.safety == Safety::Review;
        let row =
            table_row(("cache", id), checked).debug_selector(move || format!("cache-row-{index}"));
        let target = Target::Item(Source::Cache, id, candidate.path.clone());
        self.menu_row(row, target, cursor, cx)
            .child(ui::checkbox(checked))
            .child(
                column(col::M)
                    .id(("cache-size", id))
                    .text_right()
                    .tip(size_details(candidate, &[]))
                    .font_weight(FontWeight::MEDIUM)
                    .child(theme::bytes(candidate.usage.allocated)),
            )
            .child(
                column(col::S)
                    .flex()
                    .child(ui::safety_label(("cache-safety", id), review)),
            )
            .child(column(col::M).text_color(theme::muted()).child(cache.tool))
            .child(column(col::XXL).truncate().child(cache.name))
            .child(
                column(col::XS)
                    .text_right()
                    .text_color(theme::muted())
                    .child(theme::age(candidate.newest)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .id(("cache-path", id))
                            .truncate()
                            .text_color(theme::muted())
                            .tip(tooltip::path(&candidate.path))
                            .child(display_path(&candidate.path)),
                    )
                    .when(!cache.note.is_empty(), |this| {
                        this.child(
                            div()
                                .truncate()
                                .text_size(unit(font::SMALL))
                                .text_color(theme::faint())
                                .child(cache.note),
                        )
                    }),
            )
            .h(unit(size::ROW_TALL))
            .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                if !app.busy() {
                    app.toggle(Source::Cache, id, cx);
                }
            }))
    }

    pub(super) fn render_toolchains(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(report) = self.report() else {
            return div().into_any_element();
        };
        let header = table_header()
            .child(div().w(unit(size::CHECK)))
            .child(head(col::M, "Size", Term::Size).text_right())
            .child(head(col::S, "Safety", Term::Safety))
            .child(head(col::S, "Tool", Term::Tool))
            .child(column(col::WIDE).child("Version"))
            .child(div().flex_1().child("Path"));
        let count = self.listed(Tab::Toolchains).slots.len();
        let list = self
            .table_list(
                "toolchains",
                Tab::Toolchains,
                count,
                |app, range, cx| app.toolchain_rows(range, cx),
                cx,
            )
            .debug_selector(|| "toolchain-list".to_owned());
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(
                ui::toolbar()
                    .child(format!(
                        "{} in {}",
                        theme::bytes(report.toolchain_total(None)),
                        plural(report.toolchains.len(), "version")
                    ))
                    .child(
                        div()
                            .id("toolchains-safe")
                            .tip(tooltip::term(Term::Safe))
                            .child(format!(
                                "unused {}",
                                theme::bytes(report.toolchain_total(Some(Safety::Safe)))
                            )),
                    )
                    .child(
                        div()
                            .id("toolchains-review")
                            .text_color(theme::review())
                            .tip(tooltip::term(Term::Review))
                            .child(format!(
                                "default or pinned {}",
                                theme::bytes(report.toolchain_total(Some(Safety::Review)))
                            )),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_color(theme::faint())
                            .child("Removed for good, not moved to the trash"),
                    ),
            )
            .child(header)
            .child(if report.toolchains.is_empty() {
                div()
                    .flex_1()
                    .child(ui::empty(if report.options.toolchains {
                        "No installed toolchain versions found"
                    } else {
                        "Toolchains were skipped for this scan"
                    }))
                    .into_any_element()
            } else if count == 0 {
                div().flex_1().into_any_element()
            } else {
                list.into_any_element()
            })
            .into_any_element()
    }

    fn toolchain_rows(&self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<Stateful<Div>> {
        let Some(report) = self.report() else {
            return Vec::new();
        };
        let listed = self.listed(Tab::Toolchains);
        let cursor = self.cursor_target();
        range
            .filter_map(|row| {
                let (toolchain, candidate) = report.toolchains.get(listed.slots.get(row)?.0)?;
                Some(self.toolchain_row(row, toolchain, candidate, cursor.as_ref(), cx))
            })
            .collect()
    }

    fn toolchain_row(
        &self,
        index: usize,
        toolchain: &Toolchain,
        candidate: &Candidate,
        cursor: Option<&Target>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let id = candidate.id;
        let checked = self.selection.contains(&(Source::Toolchain, id));
        let review = candidate.rule.safety == Safety::Review;
        let note = toolchain.note();
        let row = table_row(("toolchain", id), checked)
            .debug_selector(move || format!("toolchain-row-{index}"));
        let target = Target::Item(Source::Toolchain, id, candidate.path.clone());
        self.menu_row(row, target, cursor, cx)
            .child(ui::checkbox(checked))
            .child(
                column(col::M)
                    .id(("toolchain-size", id))
                    .text_right()
                    .tip(size_details(candidate, &[]))
                    .font_weight(FontWeight::MEDIUM)
                    .child(theme::bytes(candidate.usage.allocated)),
            )
            .child(
                column(col::S)
                    .flex()
                    .child(ui::safety_label(("toolchain-safety", id), review)),
            )
            .child(
                column(col::S)
                    .text_color(theme::muted())
                    .child(toolchain.tool.name()),
            )
            .child(
                column(col::WIDE)
                    .id(("toolchain-version", id))
                    .truncate()
                    .tip(tooltip::plain(toolchain.version.clone()))
                    .child(toolchain_label(toolchain)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .id(("toolchain-path", id))
                            .truncate()
                            .text_color(theme::muted())
                            .tip(tooltip::path(&candidate.path))
                            .child(display_path(&candidate.path)),
                    )
                    .when(!note.is_empty(), |this| {
                        this.child(
                            div()
                                .truncate()
                                .text_size(unit(font::SMALL))
                                .text_color(theme::faint())
                                .child(note),
                        )
                    }),
            )
            .h(unit(size::ROW_TALL))
            .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                if !app.busy() {
                    app.toggle(Source::Toolchain, id, cx);
                }
            }))
    }

    pub(super) fn render_docker(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(report) = self.report() else {
            return div().into_any_element();
        };
        let header = table_header()
            .child(div().w(unit(size::CHECK)))
            .child(head(col::M, "Size", Term::Size).text_right())
            .child(head(col::S, "Safety", Term::Safety))
            .child(column(col::S).child("Engine"))
            .child(column(col::L).child("Object"))
            .child(div().flex_1().child("Name"));
        let count = self.listed(Tab::Docker).slots.len();
        let list = self
            .table_list(
                "docker",
                Tab::Docker,
                count,
                |app, range, cx| app.docker_rows(range, cx),
                cx,
            )
            .debug_selector(|| "docker-list".to_owned());
        let empty = if !report.options.docker {
            "Docker and Podman were skipped for this scan".to_owned()
        } else if report.docker_issues.is_empty() {
            "Nothing to free in Docker or Podman".to_owned()
        } else {
            report.docker_issues.join(". ")
        };
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(
                ui::toolbar()
                    .child(format!(
                        "{} in {}",
                        theme::bytes(report.docker_total(None)),
                        plural(report.docker.len(), "object")
                    ))
                    .child(
                        div()
                            .id("docker-safe")
                            .tip(tooltip::term(Term::Safe))
                            .child(format!(
                                "safe {}",
                                theme::bytes(report.docker_total(Some(Safety::Safe)))
                            )),
                    )
                    .child(
                        div()
                            .id("docker-review")
                            .text_color(theme::review())
                            .tip(tooltip::term(Term::Review))
                            .child(format!(
                                "review {}",
                                theme::bytes(report.docker_total(Some(Safety::Review)))
                            )),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_color(theme::faint())
                            .child("Removed through the engine, not moved to the trash"),
                    ),
            )
            .when(
                !report.docker.is_empty() && !report.docker_issues.is_empty(),
                |this| {
                    this.child(
                        div()
                            .px(unit(space::GUTTER))
                            .py(unit(space::S))
                            .text_color(theme::faint())
                            .child(report.docker_issues.join(". ")),
                    )
                },
            )
            .child(header)
            .child(if report.docker.is_empty() {
                div().flex_1().child(ui::empty(empty)).into_any_element()
            } else if count == 0 {
                div().flex_1().into_any_element()
            } else {
                list.into_any_element()
            })
            .into_any_element()
    }

    fn docker_rows(&self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<Stateful<Div>> {
        let Some(report) = self.report() else {
            return Vec::new();
        };
        let listed = self.listed(Tab::Docker);
        let cursor = self.cursor_target();
        range
            .filter_map(|row| {
                let (item, candidate) = report.docker.get(listed.slots.get(row)?.0)?;
                Some(self.docker_row(row, item, candidate, cursor.as_ref(), cx))
            })
            .collect()
    }

    fn docker_row(
        &self,
        index: usize,
        item: &docker::Item,
        candidate: &Candidate,
        cursor: Option<&Target>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let id = candidate.id;
        let checked = self.selection.contains(&(Source::Docker, id));
        let review = candidate.rule.safety == Safety::Review;
        let row = table_row(("docker", id), checked)
            .debug_selector(move || format!("docker-row-{index}"));
        let target = Target::Item(Source::Docker, id, candidate.path.clone());
        self.menu_row(row, target, cursor, cx)
            .child(ui::checkbox(checked))
            .child(
                column(col::M)
                    .text_right()
                    .font_weight(FontWeight::MEDIUM)
                    .child(theme::bytes(candidate.usage.allocated)),
            )
            .child(
                column(col::S)
                    .flex()
                    .child(ui::safety_label(("docker-safety", id), review)),
            )
            .child(
                column(col::S)
                    .text_color(theme::muted())
                    .child(item.engine.clone()),
            )
            .child(
                column(col::L)
                    .text_color(theme::muted())
                    .child(item.object.name()),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .id(("docker-name", id))
                            .truncate()
                            .tip(tooltip::plain(format!("{}  {}", item.label, item.id)))
                            .child(item.label.clone()),
                    )
                    .when(!item.note.is_empty(), |this| {
                        this.child(
                            div()
                                .truncate()
                                .text_size(unit(font::SMALL))
                                .text_color(theme::faint())
                                .child(item.note.clone()),
                        )
                    }),
            )
            .h(unit(size::ROW_TALL))
            .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                if !app.busy() {
                    app.toggle(Source::Docker, id, cx);
                }
            }))
    }
}
