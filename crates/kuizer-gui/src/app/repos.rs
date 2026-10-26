//! The Git repositories section: what `git gc` would free and which local branches are already
//! merged, with one click to pack a repository and a confirmation before branches go.

use kuizer::repos::{self, Collected, Pruned, RepoError, Repository};

use gpui::AnyView;

use super::keys::contains;
use super::*;

pub(super) const GIT_RUNNING: &str = "Available once the running git command finishes";
const NOTHING_TO_PACK: &str = "git gc would free less than git.min_gain here";
const NO_BRANCHES: &str = "No merged branches here";
const NOT_IN_REPOS: &str = "Available in Git repositories";

/// What one background git run did to a repository, and how the repository looks after it.
struct Done {
    root: PathBuf,
    collected: Option<Result<Collected, RepoError>>,
    pruned: Vec<Pruned>,
    after: Option<Repository>,
}

/// A git run in progress: the repositories it works on.
#[derive(Default)]
pub(super) struct GitJob {
    pub roots: HashSet<PathBuf>,
}

/// Which repositories a git command acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum GitScope {
    One(PathBuf),
    All,
}

impl Kuizer {
    pub(super) fn git_running(&self) -> bool {
        !self.git.roots.is_empty()
    }

    fn repository(&self, root: &Path) -> Option<&Repository> {
        self.report()?
            .repos
            .iter()
            .find(|repository| repository.root == root)
    }

    fn min_gain(&self) -> u64 {
        self.config.git.min_gain.0
    }

    /// The repositories a scope covers that git gc would run in.
    fn collectable(&self, scope: &GitScope) -> Vec<&Repository> {
        let min_gain = self.min_gain();
        self.scoped(scope)
            .into_iter()
            .filter(|repository| repository.collectable(min_gain) && repository.busy.is_none())
            .collect()
    }

    /// The repositories a scope covers that have merged branches to delete.
    pub(super) fn prunable(&self, scope: &GitScope) -> Vec<&Repository> {
        self.scoped(scope)
            .into_iter()
            .filter(|repository| !repository.merged.is_empty() && repository.busy.is_none())
            .collect()
    }

    fn scoped(&self, scope: &GitScope) -> Vec<&Repository> {
        let Some(report) = self.report() else {
            return Vec::new();
        };
        match scope {
            GitScope::One(root) => report
                .repos
                .iter()
                .filter(|repository| repository.root == *root)
                .collect(),
            GitScope::All => {
                let needle = self.needle(Tab::Repos);
                report
                    .repos
                    .iter()
                    .filter(|repository| repo_matches(needle.as_deref(), repository))
                    .collect()
            }
        }
    }

    /// The repository under the keyboard cursor of the Git repositories section.
    pub(super) fn cursor_repo(&self) -> Option<PathBuf> {
        match self.cursor_target() {
            Some(Target::Repo(root)) if self.tab == Tab::Repos => Some(root),
            _ => None,
        }
    }

    /// Why git gc cannot run for the scope now, or `None` when it can.
    pub(super) fn gc_blocked(&self, scope: Option<&GitScope>) -> Option<&'static str> {
        let Some(scope) = scope else {
            return Some(NOT_IN_REPOS);
        };
        match scope {
            GitScope::One(root) => match self.repository(root) {
                Some(repository) => self.gc_lock(repository),
                None if self.git_running() => Some(GIT_RUNNING),
                None => Some(NOTHING_TO_PACK),
            },
            GitScope::All => self.gc_lock_all(&self.scoped(scope)),
        }
    }

    /// Why merged branches cannot be deleted for the scope now, or `None` when they can.
    pub(super) fn branches_blocked(&self, scope: Option<&GitScope>) -> Option<&'static str> {
        let Some(scope) = scope else {
            return Some(NOT_IN_REPOS);
        };
        match scope {
            GitScope::One(root) => match self.repository(root) {
                Some(repository) => self.branches_lock(repository),
                None if self.git_running() => Some(GIT_RUNNING),
                None => Some(NO_BRANCHES),
            },
            GitScope::All => self.branches_lock_all(&self.scoped(scope)),
        }
    }

    fn gc_lock(&self, repository: &Repository) -> Option<&'static str> {
        if self.git_running() {
            return Some(GIT_RUNNING);
        }
        if let Some(busy) = repository.busy {
            return Some(busy_reason(busy));
        }
        (!repository.collectable(self.min_gain()))
            .then_some(NOTHING_TO_PACK)
            .or_else(|| self.wait())
    }

    fn gc_lock_all(&self, shown: &[&Repository]) -> Option<&'static str> {
        if self.git_running() {
            return Some(GIT_RUNNING);
        }
        let min_gain = self.min_gain();
        let any = shown
            .iter()
            .any(|repository| repository.collectable(min_gain) && repository.busy.is_none());
        (!any).then_some(NOTHING_TO_PACK).or_else(|| self.wait())
    }

    fn branches_lock(&self, repository: &Repository) -> Option<&'static str> {
        if self.git_running() {
            return Some(GIT_RUNNING);
        }
        if let Some(busy) = repository.busy {
            return Some(busy_reason(busy));
        }
        repository
            .merged
            .is_empty()
            .then_some(NO_BRANCHES)
            .or_else(|| self.wait())
    }

    fn branches_lock_all(&self, shown: &[&Repository]) -> Option<&'static str> {
        if self.git_running() {
            return Some(GIT_RUNNING);
        }
        let any = shown
            .iter()
            .any(|repository| !repository.merged.is_empty() && repository.busy.is_none());
        (!any).then_some(NO_BRANCHES).or_else(|| self.wait())
    }

    /// Runs `git gc` in every repository of the scope on a worker, with no confirmation: it
    /// only packs what git would pack on its own.
    pub(super) fn collect_garbage(&mut self, scope: &GitScope, cx: &mut Context<Self>) {
        if self.gc_blocked(Some(scope)).is_some() {
            return;
        }
        let targets: Vec<Repository> = self.collectable(scope).into_iter().cloned().collect();
        let config = self.config.git.clone();
        self.run_git(targets, cx, move |repository| Done {
            root: repository.root.clone(),
            collected: Some(repos::collect_garbage(&repository, &config)),
            pruned: Vec::new(),
            after: repos::inspect(&repository.root, &config).ok(),
        });
    }

    /// Opens the confirmation that lists the merged branches of the scope.
    pub(super) fn ask_branches(&mut self, scope: GitScope, cx: &mut Context<Self>) {
        if self.branches_blocked(Some(&scope)).is_some() {
            return;
        }
        self.menu = None;
        self.dialog = Some(Dialog::Branches(scope));
        cx.notify();
    }

    pub(super) fn accept_branches(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::Branches(scope)) = self.dialog.take() else {
            return;
        };
        if self.branches_blocked(Some(&scope)).is_some() {
            cx.notify();
            return;
        }
        let targets: Vec<Repository> = self.prunable(&scope).into_iter().cloned().collect();
        let config = self.config.git.clone();
        self.run_git(targets, cx, move |repository| Done {
            root: repository.root.clone(),
            collected: None,
            pruned: repos::delete_branches(&repository, &repository.merged, &config),
            after: repos::inspect(&repository.root, &config).ok(),
        });
    }

    fn run_git(
        &mut self,
        targets: Vec<Repository>,
        cx: &mut Context<Self>,
        work: impl Fn(Repository) -> Done + Send + 'static,
    ) {
        if targets.is_empty() {
            return;
        }
        let roots = targets
            .iter()
            .map(|repository| repository.root.clone())
            .collect();
        let spawned = spawn_worker("kuizer-gui-git", move || {
            targets.into_iter().map(work).collect::<Vec<Done>>()
        });
        let receiver = match spawned {
            Ok(receiver) => receiver,
            Err(error) => {
                self.tell(Notice::Failed(format!("Could not run git: {error}")), cx);
                return;
            }
        };
        self.git.roots = roots;
        cx.notify();
        self.git_task = Some(cx.spawn(async move |this, cx| {
            let done = receiver.await.ok();
            let _ = this.update(cx, |app, cx| app.git_done(done, cx));
        }));
    }

    fn git_done(&mut self, done: Option<Vec<Done>>, cx: &mut Context<Self>) {
        self.git = GitJob::default();
        self.git_task = None;
        let Some(done) = done else {
            self.tell(
                Notice::Failed("The git command stopped unexpectedly".to_owned()),
                cx,
            );
            cx.notify();
            return;
        };
        let min_gain = self.min_gain();
        self.report_generation += 1;
        if let Phase::Ready(report) = &mut self.phase {
            for result in &done {
                let index = report
                    .repos
                    .iter()
                    .position(|repository| repository.root == result.root);
                match (index, &result.after) {
                    (Some(index), Some(after)) if after.worth(min_gain) => {
                        report.repos[index] = after.clone();
                    }
                    (Some(index), _) => {
                        report.repos.remove(index);
                    }
                    (None, _) => {}
                }
            }
        }
        let notice = summary(&done, |root| self.shown_path(root));
        self.tell(notice, cx);
        cx.notify();
    }

    pub(super) fn render_repos(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(report) = self.report() else {
            return div().into_any_element();
        };
        let min_gain = self.min_gain();
        let shown: Vec<&Repository> = self
            .listed(Tab::Repos)
            .slots
            .iter()
            .filter_map(|&(at, _)| report.repos.get(at))
            .collect();
        let gain: u64 = shown
            .iter()
            .filter(|repository| repository.collectable(min_gain))
            .map(|repository| repository.objects.gain())
            .sum();
        let branches: usize = shown.iter().map(|repository| repository.merged.len()).sum();
        let header = table_header()
            .child(head(col::M, "git gc gain", Term::GcGain).text_right())
            .child(head(col::S, "Loose", Term::LooseObjects).text_right())
            .child(head(col::S, "Merged", Term::MergedBranches).text_right())
            .child(div().flex_1().child("Repository"))
            .child(div().w(unit(size::ACTIONS)).flex_none());
        let rows = shown.len();
        let list = self
            .table_list(
                "repos",
                Tab::Repos,
                rows,
                |app, range, cx| app.repo_rows(range, cx),
                cx,
            )
            .debug_selector(|| "repo-list".to_owned());
        let all_gc = self.gc_lock_all(&shown);
        let all_branches = self.branches_lock_all(&shown);
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(
                self.list_toolbar(cx)
                    .child(div().child(count(branches, "merged branch", "merged branches")))
                    .child(div().flex_1())
                    .child(self.tally(Tab::Repos, Some(gain)))
                    .child(
                        ui::button(
                            "repos-gc-all",
                            "Run git gc in all",
                            ButtonKind::Secondary,
                            all_gc.is_none(),
                        )
                        .debug_selector(|| "repos-gc-all".to_owned())
                        .tip(
                            match all_gc {
                                Some(reason) => Tooltip::new(reason),
                                None => {
                                    Tooltip::new("Pack every listed repository that is worth it")
                                        .keys(&[hint(&Command::GcAll)])
                                }
                            }
                            .build(),
                        )
                        .on_click(cx.listener(
                            |app, _: &ClickEvent, _, cx| {
                                app.collect_garbage(&GitScope::All, cx);
                            },
                        )),
                    )
                    .child(
                        ui::button(
                            "repos-branches-all",
                            "Delete merged branches",
                            ButtonKind::Ghost,
                            all_branches.is_none(),
                        )
                        .debug_selector(|| "repos-branches-all".to_owned())
                        .tip(
                            match all_branches {
                                Some(reason) => Tooltip::new(reason),
                                None => Tooltip::new(
                                    "Delete every listed merged branch, after a confirmation",
                                )
                                .keys(&[hint(&Command::BranchesAll)]),
                            }
                            .build(),
                        )
                        .on_click(cx.listener(
                            |app, _: &ClickEvent, _, cx| {
                                app.ask_branches(GitScope::All, cx);
                            },
                        )),
                    ),
            )
            .child(header)
            .child(if report.repos.is_empty() {
                div()
                    .flex_1()
                    .child(ui::empty(if report.options.repos {
                        "No git repositories worth cleaning"
                    } else {
                        "Git repositories were skipped for this scan"
                    }))
                    .into_any_element()
            } else if rows == 0 {
                div().flex_1().into_any_element()
            } else {
                list.into_any_element()
            })
            .into_any_element()
    }

    fn repo_rows(&self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<Stateful<Div>> {
        let Some(report) = self.report() else {
            return Vec::new();
        };
        let listed = self.listed(Tab::Repos);
        let cursor = self.cursor_target();
        range
            .filter_map(|row| {
                let repository = report.repos.get(listed.slots.get(row)?.0)?;
                Some(self.repo_row(row, repository, cursor.as_ref(), cx))
            })
            .collect()
    }

    fn repo_row(
        &self,
        index: usize,
        repository: &Repository,
        cursor: Option<&Target>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let root = repository.root.clone();
        let running = self.git.roots.contains(&root);
        let gc_lock = self.gc_lock(repository);
        let branch_lock = self.branches_lock(repository);
        let id = index as u64;
        let row =
            table_row(("repo", id), false).debug_selector(move || format!("repo-row-{index}"));
        let target = Target::Repo(root.clone());
        let names: Vec<&str> = repository
            .merged
            .iter()
            .map(|branch| branch.name.as_str())
            .collect();
        let detail = match (repository.busy, names.is_empty()) {
            (Some(busy), _) => Some((format!("Skipped: {}", busy_reason(busy)), true)),
            (None, false) => Some((
                format!(
                    "merged into {}: {}",
                    repository.default_branch.as_deref().unwrap_or("default"),
                    names.join(", ")
                ),
                false,
            )),
            (None, true) => None,
        };
        let gc_root = root.clone();
        let branch_root = root.clone();
        self.menu_row(row, target, cursor, cx)
            .h(unit(size::ROW_TALL))
            .child(
                column(col::M)
                    .id(("repo-gain", id))
                    .text_right()
                    .tip(gain_details(repository))
                    .font_weight(FontWeight::MEDIUM)
                    .child(theme::bytes(repository.objects.gain())),
            )
            .child(
                column(col::S)
                    .text_right()
                    .text_color(theme::muted())
                    .child(repository.objects.loose.to_string()),
            )
            .child(
                column(col::S)
                    .text_right()
                    .text_color(theme::muted())
                    .child(repository.merged.len().to_string()),
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
                            .id(("repo-path", id))
                            .truncate()
                            .tip(tooltip::path(&repository.root))
                            .child(self.shown_path(&repository.root)),
                    )
                    .children(detail.map(|(text, busy)| {
                        div()
                            .truncate()
                            .text_size(unit(font::SMALL))
                            .text_color(if busy {
                                theme::review()
                            } else {
                                theme::faint()
                            })
                            .child(text)
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .justify_end()
                    .gap(unit(space::S))
                    .w(unit(size::ACTIONS))
                    .child(
                        ui::button(
                            ("repo-gc", id),
                            if running { "Running" } else { "Run git gc" },
                            ButtonKind::Secondary,
                            gc_lock.is_none(),
                        )
                        .debug_selector(move || format!("repo-gc-{index}"))
                        .tip(
                            match gc_lock {
                                Some(reason) => Tooltip::new(reason),
                                None => Tooltip::new(format!(
                                    "Pack loose objects, frees up to {}",
                                    theme::bytes(repository.objects.gain())
                                ))
                                .keys(&[hint(&Command::Gc)]),
                            }
                            .build(),
                        )
                        .on_click(cx.listener(
                            move |app, _: &ClickEvent, _, cx| {
                                cx.stop_propagation();
                                app.collect_garbage(&GitScope::One(gc_root.clone()), cx);
                            },
                        )),
                    )
                    .child(
                        ui::button(
                            ("repo-branches", id),
                            "Branches",
                            ButtonKind::Ghost,
                            branch_lock.is_none(),
                        )
                        .debug_selector(move || format!("repo-branches-{index}"))
                        .tip(
                            match branch_lock {
                                Some(reason) => Tooltip::new(reason),
                                None => {
                                    Tooltip::new("Delete the merged branches, after a confirmation")
                                        .keys(&[hint(&Command::Branches)])
                                }
                            }
                            .build(),
                        )
                        .on_click(cx.listener(
                            move |app, _: &ClickEvent, _, cx| {
                                cx.stop_propagation();
                                app.ask_branches(GitScope::One(branch_root.clone()), cx);
                            },
                        )),
                    ),
            )
    }

    pub(super) fn render_branch_dialog(
        &self,
        scope: &GitScope,
        panel: Div,
        cx: &mut Context<Self>,
    ) -> Div {
        let targets = self.prunable(scope);
        let total: usize = targets
            .iter()
            .map(|repository| repository.merged.len())
            .sum();
        let lines: Vec<(String, String)> = targets
            .iter()
            .flat_map(|repository| {
                let shown = self.shown_path(&repository.root);
                repository
                    .merged
                    .iter()
                    .map(move |branch| (shown.clone(), branch.name.clone()))
            })
            .collect();
        panel
            .child(
                div()
                    .text_size(unit(font::TITLE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(format!(
                        "Delete {} for good?",
                        count(total, "merged branch", "merged branches")
                    )),
            )
            .child(
                div()
                    .debug_selector(|| "permanent-warning".to_owned())
                    .text_color(theme::danger())
                    .child(format!(
                        "{} deleted permanently. They skip the trash, cannot be restored and the undo bar cannot bring them back.",
                        count(total, "merged branch is", "merged branches are")
                    )),
            )
            .child(div().text_color(theme::muted()).child(
                "Their commits stay in the default branch, so git branch NAME COMMIT recreates one. Remote branches are not touched.",
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(unit(space::XS))
                    .p(unit(space::L))
                    .rounded(theme::radius_panel())
                    .bg(theme::panel())
                    .children(lines.iter().take(LIST_LIMIT).map(|(repository, branch)| {
                        div()
                            .flex()
                            .gap(unit(space::M))
                            .child(div().flex_none().child(branch.clone()))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_right()
                                    .text_color(theme::muted())
                                    .child(repository.clone()),
                            )
                    }))
                    .when(lines.len() > LIST_LIMIT, |this| {
                        this.child(
                            div()
                                .text_color(theme::faint())
                                .child(format!("and {} more", lines.len() - LIST_LIMIT)),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(unit(space::M))
                    .child(
                        ui::button("cancel", "Cancel", ButtonKind::Secondary, true)
                            .tip(Tooltip::new("Keep every branch").keys(&["Esc"]).build())
                            .on_click(cx.listener(|app, _: &ClickEvent, _, cx| {
                                app.dialog = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        ui::button("confirm", "Delete for good", ButtonKind::Primary, total > 0)
                            .debug_selector(|| "confirm-branches".to_owned())
                            .tip(
                                Tooltip::new("Delete these local branches")
                                    .keys(&["Enter"])
                                    .build(),
                            )
                            .on_click(cx.listener(|app, _: &ClickEvent, _, cx| {
                                app.accept_branches(cx);
                            })),
                    ),
            )
    }
}

pub(super) fn repo_matches(needle: Option<&str>, repository: &Repository) -> bool {
    needle.is_none_or(|needle| {
        contains(&repository.root.to_string_lossy(), needle)
            || repository
                .merged
                .iter()
                .any(|branch| contains(&branch.name, needle))
    })
}

fn count(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{count} {many}")
    }
}

fn busy_reason(busy: repos::Busy) -> &'static str {
    match busy {
        repos::Busy::Rebase => "A rebase is in progress in this repository",
        repos::Busy::Merge => "A merge is in progress in this repository",
        repos::Busy::CherryPick => "A cherry-pick is in progress in this repository",
        repos::Busy::Revert => "A revert is in progress in this repository",
        repos::Busy::Bisect => "A bisect is in progress in this repository",
        repos::Busy::Locked => "Another git command holds a lock in this repository",
        repos::Busy::Collecting => "git gc is already running in this repository",
    }
}

fn gain_details(repository: &Repository) -> impl Fn(&mut Window, &mut App) -> AnyView + use<> {
    let objects = repository.objects;
    Tooltip::details("What git gc frees")
        .row("Loose objects", theme::bytes(objects.loose_bytes))
        .row("Garbage", theme::bytes(objects.garbage_bytes))
        .row("Already packed", theme::bytes(objects.packed_bytes))
        .row("Packs", objects.packs.to_string())
        .note("An estimate: packing keeps a compressed copy of the loose objects.")
        .build()
}

/// The message after a git run: what was freed and deleted, or the first things that failed.
fn summary(done: &[Done], shown: impl Fn(&Path) -> String) -> Notice {
    let freed: u64 = done
        .iter()
        .filter_map(|result| result.collected.as_ref()?.as_ref().ok())
        .map(Collected::freed)
        .sum();
    let packed = done
        .iter()
        .filter(|result| matches!(result.collected, Some(Ok(_))))
        .count();
    let deleted: usize = done
        .iter()
        .map(|result| result.pruned.iter().filter(|p| p.result.is_ok()).count())
        .sum();
    let failures: Vec<String> =
        done.iter()
            .flat_map(|result| {
                let gc = result
                    .collected
                    .as_ref()
                    .and_then(|collected| collected.as_ref().err())
                    .map(|error| format!("{}: {error}", shown(&result.root)));
                let branches = result.pruned.iter().filter_map(|pruned| {
                    pruned.result.as_ref().err().map(|error| {
                        format!("{} {}: {error}", shown(&result.root), pruned.branch.name)
                    })
                });
                gc.into_iter().chain(branches).collect::<Vec<_>>()
            })
            .collect();
    let mut parts = Vec::new();
    if packed > 0 {
        parts.push(format!(
            "freed {} with git gc in {}",
            theme::bytes(freed),
            count(packed, "repository", "repositories")
        ));
    }
    if deleted > 0 {
        parts.push(format!(
            "deleted {}",
            count(deleted, "merged branch", "merged branches")
        ));
    }
    if failures.is_empty() {
        let mut text = parts.join(", ");
        if let Some(first) = text.get(..1) {
            text.replace_range(..1, &first.to_uppercase());
        }
        return Notice::Info(text);
    }
    let mut text = failures
        .iter()
        .take(TOAST_PROBLEMS)
        .cloned()
        .collect::<Vec<_>>()
        .join("; ");
    if failures.len() > TOAST_PROBLEMS {
        text.push_str(&format!(" and {} more", failures.len() - TOAST_PROBLEMS));
    }
    Notice::Failed(text)
}

#[cfg(test)]
mod tests {
    use std::process::{Command as Process, Stdio};

    use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};
    use kuizer::units::ByteSize;

    use super::*;
    use crate::app::tests::{open, type_text, wait_for_refresh};

    fn git(dir: &Path, args: &[&str]) -> bool {
        Process::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args([
                "-c",
                "init.defaultBranch=main",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    fn repository(dir: &Path) -> Option<PathBuf> {
        let root = dir.join("apps").join("kearn");
        std::fs::create_dir_all(&root).unwrap();
        if !git(&root, &["init", "-q"]) {
            return None;
        }
        for index in 0..5 {
            let name = format!("{index}.txt");
            std::fs::write(root.join(&name), &name).unwrap();
            if !git(&root, &["add", &name]) || !git(&root, &["commit", "-q", "-m", &name]) {
                return None;
            }
        }
        git(&root, &["branch", "done"]).then(|| std::fs::canonicalize(root).unwrap())
    }

    fn scanned<'a>(
        cx: &'a mut TestAppContext,
        dir: &Path,
    ) -> Option<(Entity<Kuizer>, &'a mut VisualTestContext, PathBuf)> {
        let root = repository(dir)?;
        let (view, cx) = open(cx, dir.join("recent"));
        view.update(cx, |app, _| app.config.git.min_gain = ByteSize(1));
        type_text(cx, &dir.join("apps").display().to_string());
        cx.simulate_keystrokes("enter");
        wait_for_refresh(&view, cx);
        view.update(cx, |app, cx| app.go_to(Tab::Repos, cx));
        cx.update(|window, cx| window.draw(cx).clear());
        Some((view, cx, root))
    }

    fn wait_for_git(view: &Entity<Kuizer>, cx: &mut VisualTestContext) {
        for _ in 0..1000 {
            cx.executor().advance_clock(POLL);
            cx.run_until_parked();
            if view.read_with(cx, |app, _| !app.git_running()) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the git command did not finish");
    }

    fn toast(view: &Entity<Kuizer>, cx: &mut VisualTestContext) -> String {
        view.read_with(cx, |app, _| {
            match app.toast.as_ref().map(|toast| &toast.notice) {
                Some(Notice::Info(text) | Notice::Failed(text)) => text.clone(),
                _ => String::new(),
            }
        })
    }

    #[gpui::test]
    fn a_scan_lists_the_repository_with_its_gain_and_merged_branch(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let Some((view, cx, root)) = scanned(cx, dir.path()) else {
            return;
        };
        view.read_with(cx, |app, _| {
            let repos = &app.report().unwrap().repos;
            assert_eq!(repos.len(), 1);
            assert_eq!(repos[0].root, root);
            assert!(repos[0].objects.loose >= 15);
            assert_eq!(repos[0].merged[0].name, "done");
            assert_eq!(app.tab_count(Tab::Repos), Some(1));
            assert_eq!(app.cursor_target(), Some(Target::Repo(root.clone())));
        });
        assert!(cx.debug_bounds("repo-row-0").is_some());
    }

    #[gpui::test]
    fn one_click_runs_git_gc_and_updates_the_row(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let Some((view, cx, root)) = scanned(cx, dir.path()) else {
            return;
        };
        let button = cx
            .debug_bounds("repo-gc-0")
            .expect("the gc button is painted");
        cx.simulate_click(button.center(), Modifiers::default());
        cx.run_until_parked();
        assert!(view.read_with(cx, |app, _| app.dialog.is_none()));
        wait_for_git(&view, cx);
        assert!(
            toast(&view, cx).starts_with("Freed"),
            "{}",
            toast(&view, cx)
        );
        view.read_with(cx, |app, _| {
            let repos = &app.report().unwrap().repos;
            assert_eq!(repos.len(), 1, "the merged branch keeps it listed");
            assert_eq!(repos[0].root, root);
            assert_eq!(repos[0].objects.loose, 0);
            assert_eq!(
                app.gc_blocked(Some(&GitScope::One(root.clone()))),
                Some(NOTHING_TO_PACK)
            );
        });
    }

    #[gpui::test]
    fn merged_branches_go_only_after_the_confirmation(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let Some((view, cx, root)) = scanned(cx, dir.path()) else {
            return;
        };
        cx.simulate_keystrokes("b");
        assert!(view.read_with(cx, |app, _| matches!(
            &app.dialog,
            Some(Dialog::Branches(GitScope::One(shown))) if *shown == root
        )));
        cx.simulate_keystrokes("escape");
        assert!(view.read_with(cx, |app, _| app.dialog.is_none()));
        assert!(git(
            &root,
            &["show-ref", "--verify", "--quiet", "refs/heads/done"]
        ));

        cx.simulate_keystrokes("shift-b");
        cx.update(|window, cx| window.draw(cx).clear());
        let confirm = cx
            .debug_bounds("confirm-branches")
            .expect("the confirmation is painted");
        cx.simulate_click(confirm.center(), Modifiers::default());
        cx.run_until_parked();
        wait_for_git(&view, cx);
        assert_eq!(toast(&view, cx), "Deleted 1 merged branch");
        assert!(!git(
            &root,
            &["show-ref", "--verify", "--quiet", "refs/heads/done"]
        ));
        view.read_with(cx, |app, _| {
            let repos = &app.report().unwrap().repos;
            assert!(repos.iter().all(|repository| repository.merged.is_empty()));
            assert!(
                app.toast
                    .as_ref()
                    .is_some_and(|toast| toast.notice.lingers())
            );
        });
    }

    #[gpui::test]
    fn a_busy_repository_is_never_touched(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let Some((view, cx, root)) = scanned(cx, dir.path()) else {
            return;
        };
        std::fs::create_dir(root.join(".git/rebase-merge")).unwrap();
        view.update(cx, |app, _| {
            if let Phase::Ready(report) = &mut app.phase {
                report.repos[0].busy = repos::busy(&root.join(".git"));
            }
        });
        cx.simulate_keystrokes("c");
        cx.simulate_keystrokes("b");
        view.read_with(cx, |app, _| {
            assert!(!app.git_running());
            assert!(app.dialog.is_none());
            let reason = app.gc_blocked(Some(&GitScope::One(root.clone())));
            assert_eq!(reason, Some("A rebase is in progress in this repository"));
        });
        assert!(git(
            &root,
            &["show-ref", "--verify", "--quiet", "refs/heads/done"]
        ));
    }
}
