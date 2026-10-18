//! Keyboard control: the key router, the list cursor, list filters and running commands from
//! the command table.

use gpui::{Keystroke, ScrollStrategy};

use super::*;
use crate::commands::Spec;
use crate::palette;

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
        let command = commands::table()
            .into_iter()
            .find(|spec| spec.answers(keystroke))
            .map(|spec| spec.command);
        if let Some(command) = command {
            self.run(command, window, cx);
        }
    }

    fn escape(&mut self, cx: &mut Context<Self>) {
        if self.refreshing() {
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

    /// Why work that replaces the results has to wait, or `None` when nothing is running.
    fn wait(&self) -> Option<&'static str> {
        if self.restoring() {
            Some(WAIT_FOR_RESTORE)
        } else if matches!(self.dialog, Some(Dialog::Running)) {
            Some(WAIT_FOR_CLEANUP)
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
            Command::Scan(path) => self.scan_folder(&path, cx),
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
    use super::*;

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
}
