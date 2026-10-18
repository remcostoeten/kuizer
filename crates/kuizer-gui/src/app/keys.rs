//! Keyboard control: the key router, the list cursor, list filters and running commands from
//! the command table.

use gpui::{Keystroke, ScrollStrategy};

use super::*;
use crate::commands::Spec;
use crate::palette;
use crate::treemap;

const START_FIRST: &str = "Start a scan first";
const NEEDS_SCAN: &str = "Available once a scan is shown";
const WAIT_FOR_SCAN: &str = "Available once the scan finishes";
const WAIT_FOR_CLEANUP: &str = "Available once the cleanup finishes";
const SELECT_LISTS: &str = "Available in Project artifacts and Global caches";
const LISTS_ONLY: &str = "Available in a list section";
const NO_ROWS: &str = "This list has no rows";

/// How a list key moves the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Motion {
    Down,
    Up,
    First,
    Last,
    PageDown,
    PageUp,
}

/// Where the cursor lands after a motion in a list of `count` rows, never past either end.
fn step(motion: Motion, current: usize, count: usize, page: usize) -> usize {
    let last = count.saturating_sub(1);
    match motion {
        Motion::Down => (current + 1).min(last),
        Motion::Up => current.saturating_sub(1),
        Motion::First => 0,
        Motion::Last => last,
        Motion::PageDown => (current + page).min(last),
        Motion::PageUp => current.saturating_sub(page),
    }
}

/// Whether any field contains the lowercase filter text, or there is no filter.
pub(super) fn found(needle: Option<&str>, fields: &[&str]) -> bool {
    needle.is_none_or(|needle| {
        fields
            .iter()
            .any(|field| field.to_lowercase().contains(needle))
    })
}

pub(super) fn found_path(needle: Option<&str>, path: &Path) -> bool {
    found(needle, &[path.to_string_lossy().as_ref()])
}

pub(super) fn shows_run(needle: Option<&str>, run: &kuizer::history::Run) -> bool {
    found(
        needle,
        &[
            run.session().as_str(),
            run.command.as_str(),
            run.started_at.as_str(),
        ],
    )
}

/// Scrolls a list so the cursor row is inside it. Called while the row is laid out, so the
/// offset takes effect on the next frame.
pub(super) fn keep_visible(scroll: &ScrollHandle, row: Bounds<Pixels>, window: &mut Window) {
    let view = scroll.bounds();
    let mut offset = scroll.offset();
    if row.top() < view.top() {
        offset.y += view.top() - row.top();
    } else if row.bottom() > view.bottom() {
        offset.y -= row.bottom() - view.bottom();
    } else {
        return;
    }
    scroll.set_offset(offset);
    window.on_next_frame(|window, _| window.refresh());
}

/// One palette row: the command and, when it cannot run now, why.
pub(super) struct Entry {
    pub spec: Spec,
    pub blocked: Option<&'static str>,
}

impl Kuizer {
    /// Whether a text field other than the start screen's folder field has the keyboard.
    pub(super) fn typing(&self) -> bool {
        self.palette.is_some() || self.filtering
    }

    /// The current filter text of a list, lowercased, or `None` when it is empty.
    pub(super) fn needle(&self, tab: Tab) -> Option<String> {
        let text = self.filters.get(&tab)?.trim();
        (!text.is_empty()).then(|| text.to_lowercase())
    }

    pub(super) fn shows_cache(
        &self,
        cache: &kuizer::caches::KnownCache,
        candidate: &Candidate,
    ) -> bool {
        found(
            self.needle(Tab::Caches).as_deref(),
            &[
                candidate.path.to_string_lossy().as_ref(),
                cache.name,
                cache.tool,
            ],
        )
    }

    pub(super) fn scroll_for(&self, tab: Tab) -> ScrollHandle {
        self.scrolls.get(&tab).cloned().unwrap_or_default()
    }

    pub(super) fn on_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keystroke = &event.keystroke;
        if self.menu.is_some() {
            self.menu_key(event, window, cx);
            return;
        }
        if self.dialog.is_some() {
            self.dialog_key(keystroke, cx);
            return;
        }
        if self.palette.is_some() {
            self.palette_key(keystroke, window, cx);
            return;
        }
        if self.shortcuts {
            if matches!(keystroke.key.as_str(), "escape" | "enter")
                || commands::matches("?", keystroke)
            {
                self.shortcuts = false;
                cx.notify();
            }
            return;
        }
        if self.filtering {
            self.filter_key(keystroke, cx);
            return;
        }
        let escape = keystroke.key == "escape";
        match self.phase {
            Phase::Start => {
                self.type_folder(event, cx);
                return;
            }
            Phase::Loading(_) if escape => {
                self.cancel_scan(cx);
                return;
            }
            Phase::Ready(_) if escape => {
                self.escape(cx);
                return;
            }
            _ => {}
        }
        if let Some(command) = commands::lookup(keystroke, self.map_active()) {
            self.run(command, window, cx);
        }
    }

    fn escape(&mut self, cx: &mut Context<Self>) {
        if self.map_active() && !self.map.path.is_empty() {
            self.map_back(self.map.path.len() - 1);
            cx.notify();
        } else if self.refreshing() {
            self.cancel_scan(cx);
        } else if self.toast.is_some() && !self.restoring() {
            self.dismiss_toast(cx);
        } else if self.filters.remove(&self.tab).is_some() {
            self.filtered();
            cx.notify();
        }
    }

    fn dialog_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) {
        let key = keystroke.key.as_str();
        match &mut self.dialog {
            Some(Dialog::Confirm { typed, .. }) => match key {
                "escape" => self.dialog = None,
                "enter" => {
                    self.accept(cx);
                    return;
                }
                "backspace" => {
                    typed.pop();
                }
                _ => {
                    if let Some(ch) = keystroke.key_char.as_deref()
                        && ch.chars().all(|c| c.is_ascii_digit())
                        && typed.len() < 9
                    {
                        typed.push_str(ch);
                    }
                }
            },
            Some(Dialog::Finished(_)) if matches!(key, "escape" | "enter") => self.dialog = None,
            Some(Dialog::Branches(_)) => match key {
                "escape" => self.dialog = None,
                "enter" => {
                    self.accept_branches(cx);
                    return;
                }
                _ => return,
            },
            _ => return,
        }
        cx.notify();
    }

    pub(super) fn toggle_palette(
        &mut self,
        _: &TogglePalette,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.palette.take().is_some() {
            cx.notify();
        } else {
            self.open_palette(cx);
        }
    }

    pub(super) fn open_palette(&mut self, cx: &mut Context<Self>) {
        if self.dialog.is_some() {
            return;
        }
        self.menu = None;
        self.shortcuts = false;
        self.filtering = false;
        self.palette = Some(Palette::default());
        cx.notify();
    }

    /// The palette's rows for what is typed: matching commands and recent folders, best first.
    pub(super) fn palette_entries(&self) -> Vec<Entry> {
        let query = self
            .palette
            .as_ref()
            .map_or("", |palette| palette.query.as_str());
        let specs = commands::table()
            .into_iter()
            .filter(|spec| spec.palette)
            .chain(commands::scans(&self.recent.paths))
            .collect();
        palette::rank(query, specs)
            .into_iter()
            .map(|spec| Entry {
                blocked: self.blocked(&spec.command),
                spec,
            })
            .collect()
    }

    fn palette_key(&mut self, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) {
        let entries = self.palette_entries();
        let count = entries.len();
        let modifiers = keystroke.modifiers;
        let ctrl = modifiers.control && !modifiers.shift && !modifiers.alt;
        let Some(palette) = &mut self.palette else {
            return;
        };
        match keystroke.key.as_str() {
            "escape" => self.palette = None,
            "enter" => {
                let chosen = entries.into_iter().nth(palette.active);
                if let Some(entry) = chosen
                    && entry.blocked.is_none()
                {
                    self.palette = None;
                    self.run(entry.spec.command, window, cx);
                }
                return;
            }
            "down" => palette.step(1, count),
            "up" => palette.step(-1, count),
            "n" if ctrl => palette.step(1, count),
            "p" if ctrl => palette.step(-1, count),
            _ => {
                let paste = || cx.read_from_clipboard().and_then(|item| item.text());
                if !field::edit(&mut palette.query, keystroke, field::WORDS, paste) {
                    return;
                }
                palette.active = 0;
            }
        }
        if let Some(palette) = &self.palette {
            palette.scroll.scroll_to_item(palette.active);
        }
        cx.notify();
    }

    /// Runs the palette row at `index`, unless it cannot run now.
    pub(super) fn choose_entry(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = self.palette_entries().into_iter().nth(index) else {
            return;
        };
        if entry.blocked.is_none() {
            self.palette = None;
            self.run(entry.spec.command, window, cx);
        }
    }

    fn filter_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) {
        match keystroke.key.as_str() {
            "escape" => {
                self.filters.remove(&self.tab);
                self.filtering = false;
                self.filtered();
            }
            "enter" => self.filtering = false,
            "down" => self.move_cursor(Motion::Down),
            "up" => self.move_cursor(Motion::Up),
            _ => {
                let text = self.filters.entry(self.tab).or_default();
                let paste = || cx.read_from_clipboard().and_then(|item| item.text());
                if !field::edit(text, keystroke, field::WORDS, paste) {
                    return;
                }
                self.filtered();
            }
        }
        cx.notify();
    }

    /// Applies a changed filter to the rows on screen.
    fn filtered(&mut self) {
        self.refresh_rows();
        self.reveal_cursor.set(false);
        if self.tab == Tab::Artifacts {
            self.artifact_scroll.scroll_to_item(0, ScrollStrategy::Top);
        } else {
            self.scroll_for(self.tab).set_offset(point(px(0.), px(0.)));
        }
    }

    pub(super) fn focus_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.blocked(&Command::FocusFilter).is_none() {
            window.focus(&self.focus);
            self.menu = None;
            self.filtering = true;
            cx.notify();
        }
    }

    pub(super) fn go_to(&mut self, tab: Tab, cx: &mut Context<Self>) {
        if self.tab != tab {
            self.filtering = false;
            self.reveal_cursor.set(false);
        }
        self.tab = tab;
        cx.notify();
    }

    /// Why a command cannot run right now, or `None` when it can.
    pub(super) fn blocked(&self, command: &Command) -> Option<&'static str> {
        let started = !matches!(self.phase, Phase::Start);
        let shown = self.report().is_some();
        match command {
            Command::Palette
            | Command::Shortcuts
            | Command::ZoomIn
            | Command::ZoomOut
            | Command::ResetZoom => None,
            Command::GoTo(_) | Command::NextTab | Command::PreviousTab => {
                (!started).then_some(START_FIRST)
            }
            Command::Rescan if !started || self.roots.is_empty() => Some(START_FIRST),
            Command::NewScan if !started => Some("The start screen is already open"),
            Command::Rescan | Command::NewScan => self.wait(),
            Command::Undo => {
                let trashed = matches!(
                    self.toast,
                    Some(Toast {
                        notice: Notice::Trashed { .. },
                        ..
                    })
                );
                if !trashed {
                    Some("Nothing to undo right now")
                } else if self.restoring() {
                    Some(WAIT_FOR_RESTORE)
                } else if !self.can_restore() {
                    Some(WAIT_FOR_REFRESH)
                } else {
                    None
                }
            }
            Command::Scan(path) => self
                .wait()
                .or_else(|| (!path.is_dir()).then_some("This folder is missing")),
            _ if !shown => Some(NEEDS_SCAN),
            Command::SpaceMap => None,
            Command::MapIn
            | Command::MapOut
            | Command::MapTop
            | Command::MapMove(_)
            | Command::MapToggle
                if !self.map_active() =>
            {
                Some(map::MAP_ONLY)
            }
            Command::MapIn => (!self.map_focused().is_some_and(treemap::Node::opens))
                .then_some("Only a folder tile opens"),
            Command::MapOut | Command::MapTop => self
                .map
                .path
                .is_empty()
                .then_some("The map is already at the top"),
            Command::MapMove(_) => self
                .map_focused()
                .is_none()
                .then_some("This level has no tiles"),
            Command::MapToggle => {
                if self.map_focused().and_then(treemap::Node::item).is_some() {
                    self.wait()
                } else {
                    Some("Only an item tile can be selected")
                }
            }
            Command::Trash => {
                let item = matches!(self.cursor_target(), Some(Target::Item(..)));
                self.trash_lock().or_else(|| self.wait()).or_else(|| {
                    (self.selected().is_empty() && !item).then_some("Select items first")
                })
            }
            Command::SelectAll | Command::SelectNone | Command::Toggle => {
                if matches!(self.tab, Tab::Artifacts | Tab::Caches) {
                    self.wait()
                } else {
                    Some(SELECT_LISTS)
                }
            }
            Command::FocusFilter => (self.tab == Tab::Overview).then_some(LISTS_ONLY),
            Command::Gc => self.gc_blocked(self.cursor_repo().map(GitScope::One).as_ref()),
            Command::GcAll => self.gc_blocked(self.in_repos().then_some(&GitScope::All)),
            Command::Branches => {
                self.branches_blocked(self.cursor_repo().map(GitScope::One).as_ref())
            }
            Command::BranchesAll => {
                self.branches_blocked(self.in_repos().then_some(&GitScope::All))
            }
            Command::Reveal | Command::CopyPath | Command::OpenMenu if self.map_active() => self
                .cursor_target()
                .is_none()
                .then_some("Focus an item tile first"),
            Command::Down
            | Command::Up
            | Command::First
            | Command::Last
            | Command::PageDown
            | Command::PageUp
            | Command::Reveal
            | Command::CopyPath
            | Command::OpenMenu => {
                if self.tab == Tab::Overview {
                    Some(LISTS_ONLY)
                } else {
                    self.cursor_target().is_none().then_some(NO_ROWS)
                }
            }
        }
    }

    fn in_repos(&self) -> bool {
        self.tab == Tab::Repos
    }

    /// Why work that replaces the results has to wait, or `None` when nothing is running.
    pub(super) fn wait(&self) -> Option<&'static str> {
        if self.restoring() {
            Some(WAIT_FOR_RESTORE)
        } else if matches!(self.dialog, Some(Dialog::Running)) {
            Some(WAIT_FOR_CLEANUP)
        } else if self.git_running() {
            Some(repos::GIT_RUNNING)
        } else if matches!(self.phase, Phase::Loading(_)) {
            Some(WAIT_FOR_SCAN)
        } else if self.refreshing() {
            Some(WAIT_FOR_REFRESH)
        } else {
            None
        }
    }

    /// Runs a command from a key or the palette. Does nothing when it is blocked.
    pub(super) fn run(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        if self.blocked(&command).is_some() {
            return;
        }
        match command {
            Command::Palette => self.open_palette(cx),
            Command::Shortcuts => {
                self.palette = None;
                self.shortcuts = true;
            }
            Command::GoTo(tab) => self.go_to(tab, cx),
            Command::NextTab => self.go_to(self.tab.next(), cx),
            Command::PreviousTab => self.go_to(self.tab.previous(), cx),
            Command::Rescan => self.rescan(cx),
            Command::NewScan => self.new_scan(cx),
            Command::Undo => self.undo_last(cx),
            Command::Down => self.move_cursor(Motion::Down),
            Command::Up => self.move_cursor(Motion::Up),
            Command::First => self.move_cursor(Motion::First),
            Command::Last => self.move_cursor(Motion::Last),
            Command::PageDown => self.move_cursor(Motion::PageDown),
            Command::PageUp => self.move_cursor(Motion::PageUp),
            Command::Toggle => {
                if let Some(Target::Item(source, id, _)) = self.cursor_target() {
                    self.toggle(source, id, cx);
                }
            }
            Command::SelectAll => {
                let keys = self.list_keys();
                self.selection.extend(keys);
            }
            Command::SelectNone => {
                for key in self.list_keys() {
                    self.selection.remove(&key);
                }
            }
            Command::Trash => {
                if !self.selected().is_empty() {
                    self.confirm(None, window, cx);
                } else if let Some(Target::Item(source, id, _)) = self.cursor_target() {
                    self.confirm(Some((source, id)), window, cx);
                }
            }
            Command::Reveal => {
                if let Some(path) = self.cursor_target().as_ref().and_then(Target::path) {
                    self.reveal_path(path.to_path_buf(), cx);
                }
            }
            Command::CopyPath => {
                let text = match self.cursor_target() {
                    Some(Target::Run(_, command)) => Some(command),
                    Some(target) => target
                        .path()
                        .map(|path| path.to_string_lossy().into_owned()),
                    None => None,
                };
                if let Some(text) = text {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
            }
            Command::OpenMenu => self.open_menu_at_cursor(window, cx),
            Command::FocusFilter => self.focus_filter(window, cx),
            Command::ZoomIn => {
                self.zoom.zoom_in();
            }
            Command::ZoomOut => {
                self.zoom.zoom_out();
            }
            Command::ResetZoom => {
                self.zoom.reset();
            }
            Command::SpaceMap => {
                if self.tab == Tab::Overview {
                    self.map.shown = !self.map.shown;
                } else {
                    self.go_to(Tab::Overview, cx);
                    self.map.shown = true;
                }
            }
            Command::MapIn => {
                if let Some(key) = self.map_focused().map(|node| node.key.clone()) {
                    self.map_open(&key);
                }
            }
            Command::MapOut => self.map_back(self.map.path.len().saturating_sub(1)),
            Command::MapTop => self.map_back(0),
            Command::MapMove(direction) => self.map_move(direction),
            Command::MapToggle => self.map_toggle(cx),
            Command::Scan(path) => self.scan_folder(&path, cx),
            Command::Gc => {
                if let Some(root) = self.cursor_repo() {
                    self.collect_garbage(&GitScope::One(root), cx);
                }
            }
            Command::GcAll => self.collect_garbage(&GitScope::All, cx),
            Command::Branches => {
                if let Some(root) = self.cursor_repo() {
                    self.ask_branches(GitScope::One(root), cx);
                }
            }
            Command::BranchesAll => self.ask_branches(GitScope::All, cx),
        }
        cx.notify();
    }

    /// Leaves the results on screen and scans a recent folder instead.
    fn scan_folder(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.selection.clear();
        self.rows.clear();
        self.toast = None;
        self.stale = None;
        self.phase = Phase::Start;
        self.folder = display_path(path);
        self.whole_home = false;
        self.folder_error = None;
        self.start_scan(cx);
    }

    /// Every row of a list in the order it is drawn, after its filters.
    fn targets(&self, tab: Tab) -> Vec<Target> {
        let Some(report) = self.report() else {
            return Vec::new();
        };
        let needle = self.needle(tab);
        let needle = needle.as_deref();
        match tab {
            Tab::Overview => Vec::new(),
            Tab::Artifacts => self
                .rows
                .iter()
                .map(|&index| {
                    let candidate = &report.candidates[index];
                    Target::Item(Source::Artifact, candidate.id, candidate.path.clone())
                })
                .collect(),
            Tab::Caches => report
                .caches
                .iter()
                .filter(|(cache, candidate)| self.shows_cache(cache, candidate))
                .map(|(_, candidate)| {
                    Target::Item(Source::Cache, candidate.id, candidate.path.clone())
                })
                .collect(),
            Tab::Duplicates => report
                .duplicates
                .iter()
                .flat_map(|found| found.groups.iter().take(DUPLICATE_GROUPS))
                .flat_map(|group| &group.members)
                .filter(|member| found_path(needle, &member.path))
                .map(|member| Target::Duplicate(member.path.clone()))
                .collect(),
            Tab::Issues => report
                .issues
                .iter()
                .take(ISSUE_LIMIT)
                .filter(|issue| {
                    found(
                        needle,
                        &[
                            issue.path.to_string_lossy().as_ref(),
                            issue.message.as_str(),
                        ],
                    )
                })
                .map(|issue| Target::Issue(issue.path.clone()))
                .collect(),
            Tab::Repos => report
                .repos
                .iter()
                .filter(|repository| self.shows_repo(repository))
                .map(|repository| Target::Repo(repository.root.clone()))
                .collect(),
            Tab::History => report
                .history
                .as_ref()
                .map(|view| {
                    view.delete_runs()
                        .rev()
                        .filter(|run| shows_run(needle, run))
                        .map(|run| Target::Run(run.id, run.command.clone()))
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    pub(super) fn target_exists(&self, target: &Target) -> bool {
        let Some(report) = self.report() else {
            return false;
        };
        match target {
            Target::Item(Source::Artifact, id, _) => self
                .rows
                .iter()
                .any(|&index| report.candidates[index].id == *id),
            _ => self.targets(target.tab()).contains(target),
        }
    }

    fn first_target(&self) -> Option<Target> {
        let report = self.report()?;
        match self.tab {
            Tab::Artifacts => self.rows.first().map(|&index| {
                let candidate = &report.candidates[index];
                Target::Item(Source::Artifact, candidate.id, candidate.path.clone())
            }),
            tab => self.targets(tab).into_iter().next(),
        }
    }

    /// The keyboard cursor of the current list: the last row moved to or used, else its first
    /// row. The item menu's Menu key and Shift+F10 open on it.
    pub(super) fn cursor_target(&self) -> Option<Target> {
        if self.tab == Tab::Overview {
            return self.map_target().filter(|_| self.map_active());
        }
        self.cursors
            .get(&self.tab)
            .filter(|target| self.target_exists(target))
            .cloned()
            .or_else(|| self.first_target())
    }

    fn move_cursor(&mut self, motion: Motion) {
        let targets = self.targets(self.tab);
        if targets.is_empty() {
            return;
        }
        let current = self
            .cursor_target()
            .and_then(|cursor| targets.iter().position(|target| *target == cursor))
            .unwrap_or(0);
        let next = step(motion, current, targets.len(), self.page_size());
        self.cursors.insert(self.tab, targets[next].clone());
        if self.tab == Tab::Artifacts {
            let strategy = if next < current {
                ScrollStrategy::Top
            } else {
                ScrollStrategy::Bottom
            };
            self.artifact_scroll.scroll_to_item(next, strategy);
        } else {
            self.reveal_cursor.set(true);
        }
    }

    /// How many rows PageUp and PageDown move: the rows that fit in the list, less one.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a positive row count far below usize::MAX"
    )]
    fn page_size(&self) -> usize {
        let view = if self.tab == Tab::Artifacts {
            self.artifact_scroll.0.borrow().base_handle.bounds()
        } else {
            self.scroll_for(self.tab).bounds()
        };
        match self.cursor_bounds.get() {
            Some(row) if row.size.height > px(0.) => {
                ((view.size.height / row.size.height).floor().max(2.) as usize) - 1
            }
            _ => 10,
        }
    }

    /// The selectable items of the current list, after its filters.
    fn list_keys(&self) -> Vec<(Source, u64)> {
        self.targets(self.tab)
            .into_iter()
            .filter_map(|target| match target {
                Target::Item(source, id, _) => Some((source, id)),
                _ => None,
            })
            .collect()
    }

    /// Rows shown in a list and rows it has without its text filter.
    pub(super) fn counts(&self, tab: Tab) -> (usize, usize) {
        let Some(report) = self.report() else {
            return (0, 0);
        };
        let shown = if tab == Tab::Artifacts {
            self.rows.len()
        } else {
            self.targets(tab).len()
        };
        let total = match tab {
            Tab::Overview => 0,
            Tab::Artifacts => report.candidates.len(),
            Tab::Caches => report.caches.len(),
            Tab::Repos => report.repos.len(),
            Tab::Duplicates => report.duplicates.as_ref().map_or(0, |found| {
                found
                    .groups
                    .iter()
                    .take(DUPLICATE_GROUPS)
                    .map(|group| group.members.len())
                    .sum()
            }),
            Tab::Issues => report.issues.len().min(ISSUE_LIMIT),
            Tab::History => report
                .history
                .as_ref()
                .map_or(0, |view| view.delete_runs().count()),
        };
        (shown, total)
    }
}

#[cfg(test)]
mod tests {
    use gpui::{Entity, TestAppContext, VisualTestContext, size};

    use super::*;
    use crate::app::tests::{menu_target, open, project, scanned, type_text, wait_for_refresh};

    #[test]
    fn motions_stop_at_the_ends() {
        assert_eq!(step(Motion::Down, 0, 3, 10), 1);
        assert_eq!(step(Motion::Down, 2, 3, 10), 2);
        assert_eq!(step(Motion::Up, 0, 3, 10), 0);
        assert_eq!(step(Motion::Last, 0, 3, 10), 2);
        assert_eq!(step(Motion::First, 2, 3, 10), 0);
        assert_eq!(step(Motion::PageDown, 1, 30, 10), 11);
        assert_eq!(step(Motion::PageDown, 25, 30, 10), 29);
        assert_eq!(step(Motion::PageUp, 5, 30, 10), 0);
    }

    #[test]
    fn filters_ignore_case_and_match_any_field() {
        assert!(found(None, &["anything"]));
        assert!(found(Some("node"), &["/x/Node_Modules"]));
        assert!(found(Some("cargo"), &["/x/target", "Cargo build output"]));
        assert!(!found(Some("pip"), &["/x/target", "cargo"]));
    }

    /// A folder with `count` projects, each with its own node_modules folder.
    fn projects(dir: &Path, count: usize) -> PathBuf {
        let root = dir.join("work");
        for index in 0..count {
            let project = root.join(format!("project-{index:02}"));
            let modules = project.join("node_modules").join("dep");
            std::fs::create_dir_all(&modules).unwrap();
            std::fs::write(project.join("package.json"), "{}").unwrap();
            std::fs::write(modules.join("index.js"), vec![b'x'; 4096 + index]).unwrap();
        }
        root
    }

    fn scan_root<'a>(
        cx: &'a mut TestAppContext,
        dir: &Path,
        root: &Path,
        tab: Tab,
    ) -> (Entity<Kuizer>, &'a mut VisualTestContext) {
        cx.update(bind_keys);
        let (view, cx) = open(cx, dir.join("recent"));
        type_text(cx, &root.display().to_string());
        cx.simulate_keystrokes("enter");
        wait_for_refresh(&view, cx);
        view.update(cx, |app, cx| app.go_to(tab, cx));
        cx.update(|window, cx| window.draw(cx).clear());
        (view, cx)
    }

    fn cursor_row(view: &Entity<Kuizer>, cx: &mut VisualTestContext) -> usize {
        view.read_with(cx, |app, _| {
            let cursor = app.cursor_target().expect("a cursor row");
            app.targets(app.tab)
                .iter()
                .position(|target| *target == cursor)
                .unwrap()
        })
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.update(|window, cx| window.draw(cx).clear());
    }

    /// `debug_bounds` takes a `&'static str`; a test can afford to leak a few selectors.
    fn painted(cx: &mut VisualTestContext, selector: String) -> bool {
        cx.debug_bounds(Box::leak(selector.into_boxed_str()))
            .is_some()
    }

    fn inside(cx: &mut VisualTestContext, row: &'static str, list: &'static str) {
        let row = cx.debug_bounds(row).expect("the row is painted");
        let list = cx.debug_bounds(list).expect("the list is painted");
        assert!(
            row.top() >= list.top() - px(1.) && row.bottom() <= list.bottom() + px(1.),
            "{row:?} is outside {list:?}"
        );
    }

    #[gpui::test]
    fn ctrl_k_opens_the_palette_and_enter_runs_a_command(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        cx.update(bind_keys);
        let (view, cx) = scanned(cx, dir.path(), Tab::Overview);
        cx.simulate_keystrokes("ctrl-k");
        view.read_with(cx, |app, _| assert!(app.palette.is_some()));
        draw(cx);
        let panel = cx.debug_bounds("palette").expect("the palette is painted");
        let window = cx.update(|window, _| window.viewport_size());
        assert!(
            (panel.center().x - window.width / 2.).abs() < px(1.),
            "centered"
        );
        assert!(panel.top() > px(0.) && panel.bottom() < window.height);
        let rescan = cx.debug_bounds("palette-Rescan").expect("a command row");
        assert!(panel.contains(&rescan.center()));
        let everything = view.read_with(cx, |app, _| app.palette_entries().len());

        type_text(cx, "go hist");
        view.read_with(cx, |app, _| {
            let entries = app.palette_entries();
            assert!(entries.len() < everything, "typing narrows the list");
            assert_eq!(entries[0].spec.command, Command::GoTo(Tab::History));
            assert_eq!(app.palette.as_ref().unwrap().query, "go hist");
        });
        cx.simulate_keystrokes("enter");
        view.read_with(cx, |app, _| {
            assert!(
                app.palette.is_none(),
                "running a command closes the palette"
            );
            assert_eq!(app.tab, Tab::History);
        });

        cx.simulate_keystrokes("ctrl-k");
        type_text(cx, "zoom");
        cx.simulate_keystrokes("down ctrl-n ctrl-p");
        view.read_with(cx, |app, _| {
            let palette = app.palette.as_ref().unwrap();
            assert_eq!(palette.active, 1);
            assert_eq!(
                app.palette_entries()[palette.active].spec.command,
                Command::ZoomOut
            );
        });
        cx.simulate_keystrokes("enter");
        view.read_with(cx, |app, _| assert_eq!(app.zoom.percent, 90));
    }

    #[gpui::test]
    fn a_blocked_command_is_grey_and_does_nothing(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        cx.update(bind_keys);
        let (view, cx) = scanned(cx, dir.path(), Tab::Artifacts);
        view.update(cx, |app, cx| {
            let id = app.report().unwrap().candidates[0].id;
            app.toggle(Source::Artifact, id, cx);
            app.stale = Some(SystemTime::now());
        });
        cx.simulate_keystrokes("ctrl-k");
        type_text(cx, "move selected");
        view.read_with(cx, |app, _| {
            let entry = &app.palette_entries()[0];
            assert_eq!(entry.spec.command, Command::Trash);
            assert_eq!(entry.blocked, app.trash_lock());
            assert!(entry.blocked.is_some());
        });
        draw(cx);
        assert!(cx.debug_bounds("palette-Move selected to trash").is_some());
        cx.simulate_keystrokes("enter");
        view.read_with(cx, |app, _| {
            assert!(app.dialog.is_none(), "a blocked command does not run");
            assert!(app.palette.is_some(), "the palette stays open");
        });

        view.update(cx, |app, _| app.stale = None);
        cx.simulate_keystrokes("enter");
        view.read_with(cx, |app, _| {
            assert!(app.palette.is_none());
            assert!(matches!(
                app.dialog,
                Some(Dialog::Confirm { only: None, .. })
            ));
        });
    }

    #[gpui::test]
    fn escape_closes_only_the_palette(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        cx.update(bind_keys);
        let (view, cx) = scanned(cx, dir.path(), Tab::Artifacts);
        view.update(cx, |app, cx| {
            app.show_toast(Notice::Info("hello".to_owned()), cx)
        });
        cx.simulate_keystrokes("ctrl-k");
        type_text(cx, "re");
        cx.simulate_keystrokes("escape");
        view.read_with(cx, |app, _| {
            assert!(app.palette.is_none());
            assert!(app.toast.is_some(), "Escape went to the palette only");
        });
        cx.simulate_keystrokes("ctrl-k");
        view.read_with(cx, |app, _| {
            assert_eq!(app.palette.as_ref().unwrap().query, "", "it opens empty");
        });
        cx.simulate_keystrokes("ctrl-k");
        view.read_with(cx, |app, _| {
            assert!(app.palette.is_none(), "Ctrl+K toggles")
        });
        cx.simulate_keystrokes("escape");
        view.read_with(cx, |app, _| assert!(app.toast.is_none()));
    }

    #[gpui::test]
    fn recent_folders_are_in_the_palette(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        cx.update(bind_keys);
        let first = projects(&dir.path().join("a"), 1);
        let second = projects(&dir.path().join("b"), 2);
        let (view, cx) = scan_root(cx, dir.path(), &first, Tab::Artifacts);
        view.update(cx, |app, _| {
            app.recent
                .remember(&[std::fs::canonicalize(&second).unwrap()]);
        });
        cx.simulate_keystrokes("ctrl-k");
        type_text(cx, &format!("scan {}", second.display()));
        view.read_with(cx, |app, _| {
            assert!(matches!(
                &app.palette_entries()[0].spec.command,
                Command::Scan(path) if path.ends_with("b/work")
            ));
        });
        cx.simulate_keystrokes("enter");
        wait_for_refresh(&view, cx);
        view.read_with(cx, |app, _| {
            assert!(app.roots[0].ends_with("b/work"));
            assert_eq!(app.report().unwrap().candidates.len(), 2);
        });
    }

    #[gpui::test]
    fn keys_do_not_fire_while_typing(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let root = projects(dir.path(), 3);
        let (view, cx) = scan_root(cx, dir.path(), &root, Tab::Artifacts);
        cx.simulate_keystrokes("ctrl-k");
        type_text(cx, "2j ");
        view.read_with(cx, |app, _| {
            assert_eq!(app.tab, Tab::Artifacts);
            assert_eq!(app.palette.as_ref().unwrap().query, "2j ");
            assert!(app.selection.is_empty());
        });
        cx.simulate_keystrokes("escape");

        cx.simulate_keystrokes("/");
        view.read_with(cx, |app, _| assert!(app.filtering));
        type_text(cx, "1");
        cx.simulate_keystrokes("j k space ? ctrl-a");
        view.read_with(cx, |app, _| {
            assert_eq!(app.tab, Tab::Artifacts);
            assert_eq!(app.filters.get(&Tab::Artifacts).unwrap(), "1jk ?");
            assert!(app.selection.is_empty());
            assert!(!app.shortcuts);
            assert!(app.cursors.is_empty(), "the cursor never moved");
        });
    }

    /// The rows of the artifact list that are fully scrolled into view.
    fn visible_rows(view: &Entity<Kuizer>, cx: &mut VisualTestContext) -> (usize, usize) {
        view.read_with(cx, |app, _| {
            let state = app.artifact_scroll.0.borrow();
            let top = -state.base_handle.offset().y;
            let height = state.base_handle.bounds().size.height;
            let content = state.base_handle.max_offset().height + height;
            #[allow(clippy::cast_precision_loss, reason = "a handful of rows")]
            let row = content / app.rows.len() as f32;
            let first = (top / row).ceil();
            let last = ((top + height) / row).floor() - 1.;
            (first as usize, last as usize)
        })
    }

    #[gpui::test]
    fn j_and_k_move_the_cursor_and_the_list_follows(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let root = projects(dir.path(), 40);
        let (view, cx) = scan_root(cx, dir.path(), &root, Tab::Artifacts);
        cx.simulate_resize(size(px(900.), px(520.)));
        draw(cx);
        assert_eq!(cursor_row(&view, cx), 0);
        cx.simulate_keystrokes("j j down");
        assert_eq!(cursor_row(&view, cx), 3);
        cx.simulate_keystrokes("k");
        assert_eq!(cursor_row(&view, cx), 2);
        draw(cx);
        assert!(
            cx.debug_bounds("cursor").is_some(),
            "the cursor row is marked"
        );

        let (first, last) = visible_rows(&view, cx);
        assert_eq!(first, 0);
        assert!(last < 39, "the list is longer than the window");
        cx.simulate_keystrokes("shift-g");
        assert_eq!(cursor_row(&view, cx), 39);
        draw(cx);
        inside(cx, "artifact-row-39", "artifact-list");
        assert_eq!(visible_rows(&view, cx).1, 39);
        for _ in 0..30 {
            cx.simulate_keystrokes("k");
        }
        assert_eq!(cursor_row(&view, cx), 9);
        draw(cx);
        let (first, last) = visible_rows(&view, cx);
        assert!(
            first <= 9 && 9 <= last,
            "row 9 is in view: {first}..={last}"
        );
        assert!(first > 0, "the list only scrolled as far as needed");
        cx.simulate_keystrokes("g");
        assert_eq!(cursor_row(&view, cx), 0);
        cx.simulate_keystrokes("pagedown");
        let page = cursor_row(&view, cx);
        assert!(page > 3 && page < 20, "a page is what fits, here {page}");
        cx.simulate_keystrokes("pageup end home");
        assert_eq!(cursor_row(&view, cx), 0);
    }

    #[gpui::test]
    fn a_long_duplicate_list_scrolls_to_the_cursor(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("photos");
        for group in 0..30 {
            for copy in ["a", "b"] {
                let folder = root.join(copy);
                std::fs::create_dir_all(&folder).unwrap();
                std::fs::write(
                    folder.join(format!("{group}.bin")),
                    vec![u8::try_from(group).unwrap(); 8192 + group],
                )
                .unwrap();
            }
        }
        let (view, cx) = scan_root(cx, dir.path(), &root, Tab::Duplicates);
        cx.simulate_resize(size(px(900.), px(520.)));
        draw(cx);
        let last = view.read_with(cx, |app, _| app.targets(Tab::Duplicates).len() - 1);
        assert_eq!(last, 59);
        cx.simulate_keystrokes("end");
        assert_eq!(cursor_row(&view, cx), last);
        draw(cx);
        draw(cx);
        inside(cx, "duplicate-29-1", "duplicate-list");
        cx.simulate_keystrokes("home");
        draw(cx);
        draw(cx);
        inside(cx, "duplicate-0-0", "duplicate-list");
    }

    #[gpui::test]
    fn space_toggles_and_enter_trashes_the_cursor_item(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let root = projects(dir.path(), 2);
        let (view, cx) = scan_root(cx, dir.path(), &root, Tab::Artifacts);
        cx.simulate_keystrokes("j space");
        let second = view.read_with(cx, |app, _| {
            let Some(Target::Item(source, id, _)) = app.cursor_target() else {
                panic!("an item");
            };
            assert_eq!(app.selection, HashSet::from([(source, id)]));
            (source, id)
        });
        cx.simulate_keystrokes("space");
        view.read_with(cx, |app, _| assert!(app.selection.is_empty()));
        cx.simulate_keystrokes("enter");
        view.read_with(cx, |app, _| {
            assert!(matches!(
                app.dialog,
                Some(Dialog::Confirm { only: Some(key), .. }) if key == second
            ));
        });
        cx.simulate_keystrokes("escape k space delete");
        view.read_with(cx, |app, _| {
            assert!(matches!(
                app.dialog,
                Some(Dialog::Confirm { only: None, .. })
            ));
            assert_eq!(app.confirming().len(), 1);
        });
    }

    #[gpui::test]
    fn the_filter_narrows_rows_and_select_all_follows_it(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let root = projects(dir.path(), 12);
        let (view, cx) = scan_root(cx, dir.path(), &root, Tab::Artifacts);
        assert_eq!(
            view.read_with(cx, |app, _| app.counts(Tab::Artifacts)),
            (12, 12)
        );
        cx.simulate_keystrokes("/");
        type_text(cx, "PROJECT-1");
        view.read_with(cx, |app, _| {
            assert_eq!(
                app.counts(Tab::Artifacts),
                (2, 12),
                "project-10 and project-11"
            );
            assert!(app.filtering);
        });
        draw(cx);
        assert!(cx.debug_bounds("filter-count").is_some());
        cx.simulate_keystrokes("enter");
        view.read_with(cx, |app, _| {
            assert!(!app.filtering, "Enter keeps the filter")
        });
        cx.simulate_keystrokes("ctrl-a");
        view.read_with(cx, |app, _| {
            assert_eq!(app.selection.len(), 2, "only the visible rows");
            assert!(
                app.selected().iter().all(|(_, candidate)| {
                    candidate.path.to_string_lossy().contains("project-1")
                })
            );
        });
        cx.simulate_keystrokes("ctrl-shift-a");
        view.read_with(cx, |app, _| assert!(app.selection.is_empty()));

        cx.simulate_keystrokes("/");
        cx.simulate_keystrokes("escape");
        view.read_with(cx, |app, _| {
            assert!(!app.filtering, "Escape leaves the field");
            assert!(!app.filters.contains_key(&Tab::Artifacts), "and clears it");
            assert_eq!(app.counts(Tab::Artifacts), (12, 12));
        });

        cx.simulate_keystrokes("/");
        type_text(cx, "nothing-like-this");
        cx.simulate_keystrokes("enter escape");
        view.read_with(cx, |app, _| {
            assert_eq!(app.rows.len(), 12, "Escape outside the field clears it too");
        });
    }

    #[gpui::test]
    fn the_menu_key_opens_on_the_cursor_row(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let root = projects(dir.path(), 3);
        let (view, cx) = scan_root(cx, dir.path(), &root, Tab::Artifacts);
        cx.simulate_keystrokes("j j");
        let cursor = view.read_with(cx, |app, _| app.cursor_target());
        draw(cx);
        cx.simulate_keystrokes("menu");
        assert_eq!(menu_target(&view, cx), cursor);
        draw(cx);
        let row = cx.debug_bounds("artifact-row-2").unwrap();
        let menu = cx.debug_bounds("menu").unwrap();
        assert!(
            menu.top() >= row.bottom() - px(1.),
            "it opens below the row"
        );
        cx.simulate_keystrokes("escape shift-f10");
        assert_eq!(menu_target(&view, cx), cursor);
    }

    #[gpui::test]
    fn number_keys_and_ctrl_tab_switch_sections(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let root = project(dir.path());
        let (view, cx) = scan_root(cx, dir.path(), &root, Tab::Overview);
        let tab = |cx: &mut VisualTestContext| view.read_with(cx, |app, _| app.tab);
        cx.simulate_keystrokes("3");
        assert_eq!(tab(cx), Tab::Caches);
        cx.simulate_keystrokes("ctrl-tab");
        assert_eq!(tab(cx), Tab::Duplicates);
        cx.simulate_keystrokes("ctrl-shift-tab ctrl-shift-tab ctrl-shift-tab");
        assert_eq!(tab(cx), Tab::Overview);
        cx.simulate_keystrokes("ctrl-shift-tab");
        assert_eq!(tab(cx), Tab::Issues, "it wraps around");
        cx.simulate_keystrokes("1");
        assert_eq!(tab(cx), Tab::Overview);
    }

    #[gpui::test]
    fn the_shortcuts_overlay_lists_the_command_table(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let root = project(dir.path());
        let (view, cx) = scan_root(cx, dir.path(), &root, Tab::Artifacts);
        cx.simulate_keystrokes("?");
        view.read_with(cx, |app, _| assert!(app.shortcuts));
        draw(cx);
        assert!(cx.debug_bounds("shortcuts").is_some());
        for spec in commands::table() {
            assert!(
                painted(cx, format!("shortcut-{}", spec.label)),
                "{} is listed",
                spec.label
            );
        }
        for (label, _) in commands::HINTS {
            assert!(
                painted(cx, format!("shortcut-{label}")),
                "{label} is listed"
            );
        }
        cx.simulate_keystrokes("j");
        view.read_with(cx, |app, _| {
            assert!(app.shortcuts, "other keys are ignored")
        });
        cx.simulate_keystrokes("escape");
        view.read_with(cx, |app, _| assert!(!app.shortcuts));

        cx.simulate_keystrokes("ctrl-k");
        type_text(cx, "keyboard");
        cx.simulate_keystrokes("enter");
        view.read_with(cx, |app, _| {
            assert!(app.shortcuts, "the palette opens it too");
            assert!(app.palette.is_none());
        });
    }
}
