use std::cell::RefCell;

use gpui::UniformList;
use kuizer::caches::KnownCache;
use kuizer::docker;
use kuizer::toolchains::Toolchain;

use super::*;

pub(super) type Slot = (usize, usize);

/// One drawn line of the duplicate list: a group's heading, or the member at this row of
/// `Listed::targets`.
#[derive(Clone, Copy)]
pub(super) enum Line {
    Group(usize),
    Member(usize),
}

#[derive(Clone, Default)]
pub(super) struct Listed {
    pub slots: Rc<[Slot]>,
    pub targets: Rc<[Target]>,
    pub bytes: u64,
    pub lines: Rc<[Line]>,
    pub line_of: Rc<[usize]>,
}

struct ListKey {
    generation: u64,
    len: usize,
    text: String,
    safety: Option<Safety>,
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

fn list_rows(report: &Report, tab: Tab, needle: Option<&str>, safety: Option<Safety>) -> Listed {
    let mut slots: Vec<Slot> = Vec::new();
    let mut targets: Vec<Target> = Vec::new();
    let mut bytes = 0;
    let mut lines: Vec<Line> = Vec::new();
    let mut line_of: Vec<usize> = Vec::new();
    let graded =
        |candidate: &Candidate| safety.is_none_or(|safety| candidate.rule.safety == safety);
    match tab {
        Tab::Overview | Tab::Artifacts => {}
        Tab::Caches => {
            for (at, (cache, candidate)) in report.caches.iter().enumerate() {
                if graded(candidate) && cache_matches(needle, cache, candidate) {
                    slots.push((at, 0));
                    targets.push(Target::Item(
                        Source::Cache,
                        candidate.id,
                        candidate.path.clone(),
                    ));
                    bytes += candidate.usage.allocated;
                }
            }
        }
        Tab::Toolchains => {
            for (at, (toolchain, candidate)) in report.toolchains.iter().enumerate() {
                if graded(candidate) && toolchain_matches(needle, toolchain, candidate) {
                    slots.push((at, 0));
                    targets.push(Target::Item(
                        Source::Toolchain,
                        candidate.id,
                        candidate.path.clone(),
                    ));
                    bytes += candidate.usage.allocated;
                }
            }
        }
        Tab::Docker => {
            for (at, (item, candidate)) in report.docker.iter().enumerate() {
                if graded(candidate) && docker_matches(needle, item, candidate) {
                    slots.push((at, 0));
                    targets.push(Target::Item(
                        Source::Docker,
                        candidate.id,
                        candidate.path.clone(),
                    ));
                    bytes += candidate.usage.allocated;
                }
            }
        }
        Tab::Duplicates => {
            let groups = report.duplicates.iter().flat_map(|found| &found.groups);
            for (group_at, group) in groups.enumerate() {
                let mut heading = Some(group_at);
                for (member_at, member) in group.members.iter().enumerate() {
                    if found_path(needle, &member.path) {
                        if let Some(group_at) = heading.take() {
                            lines.push(Line::Group(group_at));
                        }
                        line_of.push(lines.len());
                        lines.push(Line::Member(slots.len()));
                        slots.push((group_at, member_at));
                        targets.push(Target::Duplicate(member.path.clone()));
                    }
                }
            }
        }
        Tab::Issues => {
            for (at, issue) in report.issues.iter().enumerate() {
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
        bytes,
        lines: lines.into(),
        line_of: line_of.into(),
    }
}

impl Kuizer {
    pub(super) fn listed(&self, tab: Tab) -> Listed {
        let Some(report) = self.report() else {
            return Listed::default();
        };
        let text = self.filters.get(&tab).map_or("", |text| text.trim());
        let safety = self.safety.filter(|_| tab.grades());
        let (generation, len) = (self.report_generation, list_len(report, tab));
        if let Some((key, listed)) = self.lists.0.borrow().get(&tab)
            && key.generation == generation
            && key.len == len
            && key.text == text
            && key.safety == safety
        {
            return listed.clone();
        }
        let needle = self.needle(tab);
        let listed = list_rows(report, tab, needle.as_deref(), safety);
        let key = ListKey {
            generation,
            len,
            text: text.to_owned(),
            safety,
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
        .when(!self.selection.is_empty(), |this| {
            this.pb(unit(size::SUMMARY_HEIGHT + space::L))
        })
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
            .child(column(col::XXL).child("CACHE"))
            .child(head(col::XS, "Changed", Term::Changed).text_right())
            .child(div().flex_1().child("PATH"));
        let listed = self.listed(Tab::Caches);
        let count = listed.slots.len();
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
                self.list_toolbar(cx)
                    .child(div().flex_1())
                    .child(self.tally(Tab::Caches, Some(listed.bytes))),
            )
            .child(table(
                header,
                if count == 0 {
                    empty_list(
                        Tab::Caches,
                        report.caches.len(),
                        if report.options.caches {
                            "No global caches found"
                        } else {
                            "Global caches were skipped for this scan"
                        },
                        "No caches match these filters",
                    )
                } else {
                    list.into_any_element()
                },
            ))
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
        let look = self.look(Source::Cache, id, &candidate.path);
        self.menu_row(row, target, cursor, cx)
            .child(ui::check_mark(look.checked))
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
            .h(unit(size::row_tall()))
            .on_click(cx.listener(move |app, event: &ClickEvent, _, cx| {
                app.click_row(Source::Cache, id, event, cx);
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
            .child(column(col::WIDE).child("VERSION"))
            .child(div().flex_1().child("PATH"));
        let listed = self.listed(Tab::Toolchains);
        let count = listed.slots.len();
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
                self.list_toolbar(cx)
                    .child(div().flex_1())
                    .child(self.tally(Tab::Toolchains, Some(listed.bytes))),
            )
            .child(table(
                header,
                if count == 0 {
                    empty_list(
                        Tab::Toolchains,
                        report.toolchains.len(),
                        if report.options.toolchains {
                            "No installed toolchain versions found"
                        } else {
                            "Toolchains were skipped for this scan"
                        },
                        "No toolchain versions match these filters",
                    )
                } else {
                    list.into_any_element()
                },
            ))
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
        let look = self.look(Source::Toolchain, id, &candidate.path);
        self.menu_row(row, target, cursor, cx)
            .child(ui::check_mark(look.checked))
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
            .h(unit(size::row_tall()))
            .on_click(cx.listener(move |app, event: &ClickEvent, _, cx| {
                app.click_row(Source::Toolchain, id, event, cx);
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
            .child(head(col::S, "Engine", Term::Engine))
            .child(column(col::L).child("OBJECT"))
            .child(div().flex_1().child("NAME"));
        let listed = self.listed(Tab::Docker);
        let count = listed.slots.len();
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
                self.list_toolbar(cx)
                    .child(div().flex_1())
                    .child(self.tally(Tab::Docker, Some(listed.bytes))),
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
            .child(table(
                header,
                if count == 0 {
                    empty_list(
                        Tab::Docker,
                        report.docker.len(),
                        empty,
                        "No Docker or Podman objects match these filters",
                    )
                } else {
                    list.into_any_element()
                },
            ))
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
        let look = self.look(Source::Docker, id, &candidate.path);
        self.menu_row(row, target, cursor, cx)
            .child(ui::check_mark(look.checked))
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
            .h(unit(size::row_tall()))
            .on_click(cx.listener(move |app, event: &ClickEvent, _, cx| {
                app.click_row(Source::Docker, id, event, cx);
            }))
    }
}

#[cfg(test)]
mod tests {
    use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};

    use super::*;
    use crate::app::tests::{artifact, open, project, wait_for_refresh};

    fn draw(cx: &mut VisualTestContext) {
        cx.update(|window, cx| window.draw(cx).clear());
    }

    #[gpui::test]
    fn an_empty_list_says_whether_the_scan_or_the_filter_emptied_it(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (view, cx) = open(cx, dir.path().join("recent"));
        view.update(cx, |app, cx| {
            app.roots = vec![PathBuf::from("/p")];
            let mut report = Report::empty(&app.roots, &app.options);
            let cache = KnownCache {
                name: "npm",
                tool: "npm",
                path: PathBuf::from("/c/npm"),
                safety: Safety::Safe,
                note: "",
            };
            report.caches = vec![(cache, artifact(1, Path::new("/c/npm")))];
            app.phase = Phase::Ready(Box::new(report));
            app.refresh_stats();
            app.tab = Tab::Caches;
            cx.notify();
        });
        draw(cx);
        assert!(cx.debug_bounds("cache-row-0").is_some());

        view.update(cx, |app, cx| {
            app.filters
                .insert(Tab::Caches, "nothing-like-this".to_owned());
            cx.notify();
        });
        draw(cx);
        assert!(
            cx.debug_bounds("empty-Caches-filtered").is_some(),
            "a filter that hides every cache says so instead of a blank table"
        );

        view.update(cx, |app, cx| {
            app.tab = Tab::Toolchains;
            cx.notify();
        });
        draw(cx);
        assert!(cx.debug_bounds("empty-Toolchains-none").is_some());
        assert!(
            cx.debug_bounds("empty-Toolchains-filtered").is_none(),
            "a list the scan left empty does not blame the filter"
        );
    }

    #[gpui::test]
    fn a_failed_scan_offers_rescan_and_new_scan(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let root = project(dir.path());
        let (view, cx) = open(cx, dir.path().join("recent"));
        let fail = |view: &Entity<Kuizer>, cx: &mut VisualTestContext| {
            view.update(cx, |app, cx| {
                app.roots = vec![root.clone()];
                app.set_phase(Phase::Failed("Permission denied".to_owned()), cx);
                cx.notify();
            });
            draw(cx);
        };

        fail(&view, cx);
        assert!(cx.debug_bounds("scan-failed").is_some());
        assert!(cx.debug_bounds("failed-rescan").is_some());
        cx.simulate_keystrokes("ctrl-r");
        view.read_with(cx, |app, _| {
            assert!(
                !matches!(app.phase, Phase::Failed(_)),
                "the Rescan key scans again"
            );
        });
        wait_for_refresh(&view, cx);
        view.read_with(cx, |app, _| assert!(matches!(app.phase, Phase::Ready(_))));

        fail(&view, cx);
        let new_scan = cx
            .debug_bounds("failed-new-scan")
            .expect("a New scan button");
        cx.simulate_click(new_scan.center(), Modifiers::default());
        view.read_with(cx, |app, _| assert!(matches!(app.phase, Phase::Start)));

        fail(&view, cx);
        cx.simulate_keystrokes("escape");
        view.read_with(cx, |app, _| {
            assert!(
                matches!(app.phase, Phase::Start),
                "Esc backs out of a failed scan to the start screen"
            );
        });
    }
}
