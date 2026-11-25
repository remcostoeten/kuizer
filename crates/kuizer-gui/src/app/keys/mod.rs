//! Keyboard control: the key router, the list cursor, list filters and running commands from
//! the command table.

use std::cell::OnceCell;

use gpui::{Keystroke, ScrollStrategy};

use super::*;
use crate::commands::Spec;
use crate::palette;
use crate::treemap;

const START_FIRST: &str = "Start a scan first";
const NEEDS_SCAN: &str = "Available once a scan is shown";
const WAIT_FOR_SCAN: &str = "Available once the scan finishes";
pub(super) const WAIT_FOR_CLEANUP: &str = "Available once the cleanup finishes";
const SELECT_LISTS: &str = "Available in Project artifacts, Global caches, Toolchains and Docker";
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
    Forward(usize),
    Back(usize),
    Row(usize),
}

/// The longest count prefix a motion takes, in digits.
const COUNT_DIGITS: usize = 6;

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
        Motion::Forward(rows) => current.saturating_add(rows).min(last),
        Motion::Back(rows) => current.saturating_sub(rows),
        Motion::Row(row) => row.min(last),
    }
}

fn scrolled(
    command: &Command,
    from: Pixels,
    end: Pixels,
    line: Pixels,
    page: Pixels,
) -> Option<Pixels> {
    let to = match command {
        Command::Down => from + line,
        Command::Up => from - line,
        Command::PageDown => from + page,
        Command::PageUp => from - page,
        Command::First => px(0.),
        Command::Last => end,
        _ => return None,
    };
    Some(to.min(end).max(px(0.)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BulkStep {
    Safe,
    Ask,
    Everything,
    Clear,
    Nothing,
}

#[derive(Debug, Default, Clone, Copy)]
struct Coverage {
    safe: usize,
    safe_on: usize,
    review: usize,
    review_on: usize,
}

impl Coverage {
    fn press(self, asked: bool) -> BulkStep {
        if self.safe_on < self.safe {
            BulkStep::Safe
        } else if self.review_on < self.review {
            if self.safe == 0 && !asked {
                BulkStep::Ask
            } else {
                BulkStep::Everything
            }
        } else if self.safe + self.review > 0 {
            BulkStep::Clear
        } else {
            BulkStep::Nothing
        }
    }
}

pub(super) struct Asked {
    tab: Tab,
    report: u64,
    selection: u64,
    left: usize,
    keys: Vec<(Source, u64)>,
}

/// Whether any field contains the lowercase filter text, or there is no filter.
pub(super) fn found(needle: Option<&str>, fields: &[&str]) -> bool {
    needle.is_none_or(|needle| fields.iter().any(|field| contains(field, needle)))
}

/// Whether `field` contains the lowercase `needle`, ignoring case. ASCII text is compared in
/// place without allocating; anything else is lowercased first.
pub(super) fn contains(field: &str, needle: &str) -> bool {
    if !field.is_ascii() || !needle.is_ascii() {
        return field.to_lowercase().contains(needle);
    }
    let (field, needle) = (field.as_bytes(), needle.as_bytes());
    let Some((&first, rest)) = needle.split_first() else {
        return true;
    };
    field.len() >= needle.len()
        && (0..=field.len() - needle.len()).any(|start| {
            field[start].to_ascii_lowercase() == first
                && field[start + 1..start + needle.len()].eq_ignore_ascii_case(rest)
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
    window.request_animation_frame();
}

/// One palette row: the command and, when it cannot run now, why.
pub(super) struct Entry {
    pub spec: Spec,
    pub blocked: Option<&'static str>,
}

/// Everything the palette rows depend on: the query and the state the reasons for a blocked
/// command read. The rows are ranked and checked again only when one of these changes, so a
/// highlight move or a redraw reuses them.
#[derive(PartialEq)]
pub(super) struct PaletteKey {
    query: String,
    tab: Tab,
    phase: std::mem::Discriminant<Phase>,
    generations: [u64; 3],
    flags: [bool; 9],
    counts: [usize; 4],
    map: (usize, usize, Option<String>),
    cursor: Option<Target>,
}

/// How long a key counts as still down without a release. Longer than any key-repeat delay.
const HELD_FOR: Duration = Duration::from_millis(1200);

/// Tells a key repeat from a fresh press. GPUI 0.2.2 reports repeats on Wayland and macOS only,
/// and on X11 a held key arrives as presses with no release between them, so the last key down
/// is tracked here until its release.
#[derive(Default)]
pub(super) struct Held {
    key: Option<(String, Instant)>,
    releases: bool,
}

impl Held {
    pub(super) fn press(&mut self, event: &KeyDownEvent, now: Instant) -> bool {
        let key = &event.keystroke.key;
        let down = self.releases
            && self
                .key
                .as_ref()
                .is_some_and(|(held, since)| held == key && now.duration_since(*since) < HELD_FOR);
        self.key = Some((key.clone(), now));
        event.is_held || down
    }

    pub(super) fn release(&mut self, key: &str) {
        self.releases = true;
        if self.key.as_ref().is_some_and(|(held, _)| held == key) {
            self.key = None;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    Normal,
    Visual,
    Filter,
    Palette,
    Settings,
    Menu,
    Shortcuts,
    Confirm,
    Record,
}

impl Mode {
    pub(super) fn label(self) -> &'static str {
        match self {
            Mode::Normal => "Normal",
            Mode::Visual => "Visual",
            Mode::Filter => "Filter",
            Mode::Palette => "Palette",
            Mode::Settings => "Settings",
            Mode::Menu => "Menu",
            Mode::Shortcuts => "Shortcuts",
            Mode::Confirm => "Confirm",
            Mode::Record => "Record",
        }
    }
}

impl Kuizer {
    pub(super) fn mode(&self) -> Mode {
        if self.overlays.menu.is_some() {
            Mode::Menu
        } else if matches!(
            self.overlays.dialog,
            Some(Dialog::Branches { .. } | Dialog::Exclude { .. })
        ) {
            Mode::Confirm
        } else if self.overlays.settings.is_some() {
            self.settings_mode()
        } else if self.overlays.palette.is_some() {
            Mode::Palette
        } else if self.overlays.shortcuts {
            Mode::Shortcuts
        } else if self.list.filtering {
            Mode::Filter
        } else if self.list.visual {
            Mode::Visual
        } else {
            Mode::Normal
        }
    }

    /// Whether a text field other than the start screen's folder field has the keyboard.
    pub(super) fn typing(&self) -> bool {
        self.overlays.palette.is_some() || self.list.filtering
    }

    /// The current filter text of a list, lowercased, or `None` when it is empty.
    pub(super) fn needle(&self, tab: Tab) -> Option<String> {
        let text = self.list.filters.get(&tab)?.trim();
        (!text.is_empty()).then(|| text.to_lowercase())
    }

    pub(super) fn on_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keystroke = &event.keystroke;
        let repeat = self.list.held.press(event, Instant::now());
        if self.overlays.menu.is_some() {
            self.menu_key(event, repeat, cx);
            return;
        }
        if self.overlays.dialog.is_some() {
            self.dialog_key(keystroke, repeat, cx);
            return;
        }
        if self.overlays.settings.is_some() {
            self.settings_key(keystroke, cx);
            return;
        }
        if self.overlays.palette.is_some() {
            self.palette_key(keystroke, window, cx);
            return;
        }
        if self.overlays.shortcuts {
            self.shortcuts_key(keystroke, window, cx);
            return;
        }
        if self.list.filtering {
            if !self.is_control_cycle(keystroke) {
                self.filter_key(keystroke, cx);
                return;
            }
            self.list.filtering = false;
        }
        if self.ring_key(keystroke, repeat, window, cx) {
            return;
        }
        if matches!(self.session.phase, Phase::Start) && self.type_folder(event, cx) {
            return;
        }
        let escape = self
            .keymap
            .matches(Scope::GuiBack, Key::Back, commands::stroke(keystroke));
        match self.session.phase {
            Phase::Loading(_) if escape => {
                self.cancel_scan(cx);
                return;
            }
            Phase::Ready(_) if escape => {
                self.escape(cx);
                return;
            }
            Phase::Failed(_) if escape => {
                if self.overlays.toast.is_some() {
                    self.dismiss_toast(cx);
                } else {
                    self.new_scan(cx);
                }
                return;
            }
            _ => {}
        }
        if self.count_key(keystroke) {
            cx.notify();
            return;
        }
        if let Some(command) = commands::lookup(&self.keymap, keystroke, self.map_active())
            && (self.config.route_shortcuts.enabled || !matches!(command, Command::GoTo(_)))
            && !(repeat
                && matches!(
                    command,
                    Command::Trash | Command::SelectAll | Command::Restore | Command::Exclude
                ))
        {
            self.run(command, window, cx);
        }
        if !self.list.count.is_empty() {
            self.list.count.clear();
            cx.notify();
        }
    }

    fn shortcuts_key(&mut self, keystroke: &Keystroke, window: &Window, cx: &mut Context<Self>) {
        let stroke = commands::stroke(keystroke);
        if self
            .keymap
            .matches(Scope::Shortcuts, Key::ShortcutsClose, stroke)
        {
            self.overlays.shortcuts = false;
            cx.notify();
            return;
        }
        let scroll = &self.overlays.shortcuts_scroll;
        let line = unit(size::SEGMENT).to_pixels(window.rem_size());
        let page = (scroll.bounds().size.height - line).max(line);
        let (from, end) = (-scroll.offset().y, scroll.max_offset().height);
        let Some(to) = commands::lookup(&self.keymap, keystroke, false)
            .and_then(|command| scrolled(&command, from, end, line, page))
        else {
            return;
        };
        scroll.set_offset(point(px(0.), -to));
        cx.notify();
    }

    /// Takes a digit as part of a count before a motion, as in `10j`. A leading zero is not a
    /// count.
    fn count_key(&mut self, keystroke: &Keystroke) -> bool {
        let modifiers = keystroke.modifiers;
        let plain = !modifiers.modified() && !modifiers.shift;
        let digit = keystroke.key.len() == 1 && keystroke.key.as_bytes()[0].is_ascii_digit();
        let listing = matches!(self.session.phase, Phase::Ready(_))
            && self.list.tab != Tab::Overview
            && !self.map_active();
        if !plain || !digit || !listing || (self.list.count.is_empty() && keystroke.key == "0") {
            return false;
        }
        if self.list.count.len() < COUNT_DIGITS {
            self.list.count.push_str(&keystroke.key);
        }
        true
    }

    /// The motion a command stands for, with the typed count applied: `10j` moves ten rows,
    /// `3` then Page Down three pages, and `5G` or `5gg` goes to the fifth row.
    fn motion(&self, command: &Command, times: Option<usize>) -> Option<Motion> {
        let page = || self.page_size();
        Some(match (command, times) {
            (Command::Down | Command::ExtendDown, None) => Motion::Down,
            (Command::Up | Command::ExtendUp, None) => Motion::Up,
            (Command::Down | Command::ExtendDown, Some(times)) => Motion::Forward(times),
            (Command::Up | Command::ExtendUp, Some(times)) => Motion::Back(times),
            (Command::First, None) => Motion::First,
            (Command::Last, None) => Motion::Last,
            (Command::First | Command::Last, Some(row)) => Motion::Row(row.saturating_sub(1)),
            (Command::PageDown, None) => Motion::PageDown,
            (Command::PageUp, None) => Motion::PageUp,
            (Command::PageDown, Some(times)) => Motion::Forward(page().saturating_mul(times)),
            (Command::PageUp, Some(times)) => Motion::Back(page().saturating_mul(times)),
            _ => return None,
        })
    }

    /// Turns visual mode on or off. On, the cursor row is selected and every motion selects
    /// the rows it passes, from this row.
    fn toggle_visual(&mut self) {
        self.list.visual = !self.list.visual;
        self.list.range = None;
        if !self.list.visual {
            return;
        }
        let listed = self.listing(self.list.tab);
        let listed = listed.as_deref();
        let Some(cursor) = self.cursor_target() else {
            return;
        };
        let Some(row) = self.listed_row(listed, &cursor, self.row_hint(self.list.tab)) else {
            return;
        };
        let mut added = HashSet::new();
        if let Some(key) = self.listed_key(listed, row)
            && self.select(key)
        {
            added.insert(key);
        }
        self.list.range = Some(Span {
            tab: self.list.tab,
            anchor: cursor,
            anchor_row: Cell::new(row),
            added,
        });
    }

    fn escape(&mut self, cx: &mut Context<Self>) {
        self.list.count.clear();
        if self.asking().is_some() {
            self.list.asked = None;
            cx.notify();
        } else if self.list.visual {
            self.list.visual = false;
            self.list.range = None;
            cx.notify();
        } else if self.map_active() && !self.list.map.path.is_empty() {
            self.map_back(self.list.map.path.len() - 1);
            cx.notify();
        } else if self.session.search.is_some() {
            self.stop_duplicates(cx);
        } else if self.refreshing() {
            self.cancel_scan(cx);
        } else if self.overlays.toast.is_some() && !self.restoring() {
            self.dismiss_toast(cx);
        } else if self.list.filters.remove(&self.list.tab).is_some() {
            self.filtered();
            cx.notify();
        }
    }

    pub(super) fn on_key_up(&mut self, event: &KeyUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.list.held.release(&event.keystroke.key);
    }

    fn dialog_key(&mut self, keystroke: &Keystroke, repeat: bool, cx: &mut Context<Self>) {
        let action = self
            .keymap
            .lookup(Scope::Dialog, commands::stroke(keystroke));
        match &mut self.overlays.dialog {
            Some(Dialog::Finished(_)) if matches!(action, Some(Key::Back | Key::Confirm)) => {
                self.overlays.dialog = None;
            }
            Some(Dialog::Branches { cancel, .. }) => match action {
                Some(Key::Back) => self.close_branches(cx),
                Some(Key::DialogToggle) => *cancel = !*cancel,
                Some(Key::Confirm) if *cancel => self.close_branches(cx),
                Some(Key::Confirm) => {
                    if !repeat {
                        self.accept_branches(cx);
                    }
                    return;
                }
                _ => return,
            },
            Some(Dialog::Exclude { cancel, .. }) => match action {
                Some(Key::Back) => self.close_exclude(cx),
                Some(Key::DialogToggle) => *cancel = !*cancel,
                Some(Key::Confirm) if *cancel => self.close_exclude(cx),
                Some(Key::Confirm) => {
                    if !repeat {
                        self.accept_exclude(cx);
                    }
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
        if self.recording_keys() {
            cx.propagate();
            return;
        }
        if self.overlays.palette.take().is_some() {
            cx.notify();
        } else {
            self.open_palette(cx);
        }
    }

    pub(super) fn open_palette(&mut self, cx: &mut Context<Self>) {
        if self.overlays.dialog.is_some() {
            return;
        }
        self.overlays.menu = None;
        self.overlays.shortcuts = false;
        self.list.filtering = false;
        self.overlays.palette = Some(Palette::default());
        self.overlays.palette_cache.take();
        Self::probe_recent(self.start.recent.paths.clone(), cx);
        cx.notify();
    }

    /// The palette's rows for what is typed: matching commands and recent folders, best first.
    /// Cached until the query or the state behind a blocked reason changes.
    pub(super) fn palette_entries(&self) -> Rc<[Entry]> {
        let key = self.palette_state();
        if let Some((cached, entries)) = &*self.overlays.palette_cache.borrow()
            && *cached == key
        {
            return Rc::clone(entries);
        }
        let specs = commands::table()
            .iter()
            .filter(|spec| spec.palette)
            .cloned()
            .chain(commands::scans(&self.start.recent.paths))
            .collect();
        let cursor = OnceCell::new();
        let entries: Rc<[Entry]> = palette::rank(&key.query, specs)
            .into_iter()
            .map(|spec| Entry {
                blocked: self.blocked_with(&spec.command, &cursor),
                spec,
            })
            .collect();
        *self.overlays.palette_cache.borrow_mut() = Some((key, Rc::clone(&entries)));
        entries
    }

    fn palette_state(&self) -> PaletteKey {
        let query = self
            .overlays
            .palette
            .as_ref()
            .map_or_else(String::new, |palette| palette.query.clone());
        PaletteKey {
            query,
            tab: self.list.tab,
            phase: std::mem::discriminant(&self.session.phase),
            generations: [
                self.session.report_generation,
                self.list.rows_generation,
                self.list.selection.generation(),
            ],
            flags: [
                self.session.streamed,
                self.session.cancel.is_some(),
                self.session.stale.is_some(),
                self.session.cleaning.is_some(),
                self.session.search.is_some(),
                self.restoring(),
                self.overlays.dialog.is_some(),
                self.list.map.shown,
                self.session.roots.is_empty(),
            ],
            counts: [
                self.session.pending.len(),
                self.session.git.roots.len(),
                self.start.recent.paths.len(),
                self.start
                    .recent
                    .paths
                    .iter()
                    .filter(|path| self.start.recent.is_gone(path))
                    .count(),
            ],
            map: (
                self.list.map.built,
                self.list.map.path.len(),
                self.list.map.focus.clone(),
            ),
            cursor: self.list.cursors.get(&self.list.tab).cloned(),
        }
    }

    fn palette_key(&mut self, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) {
        let entries = self.palette_entries();
        let count = entries.len();
        let action = self
            .keymap
            .lookup(Scope::Palette, commands::stroke(keystroke));
        let Some(palette) = &mut self.overlays.palette else {
            return;
        };
        match action {
            Some(Key::Back) => self.overlays.palette = None,
            Some(Key::PaletteRun) => {
                let chosen = entries
                    .get(palette.active)
                    .filter(|entry| entry.blocked.is_none())
                    .map(|entry| entry.spec.command.clone());
                if let Some(command) = chosen {
                    self.overlays.palette = None;
                    self.run(command, window, cx);
                }
                return;
            }
            Some(Key::PaletteNext) => palette.step(1, count),
            Some(Key::PalettePrevious) => palette.step(-1, count),
            _ => {
                let paste = || cx.read_from_clipboard().and_then(|item| item.text());
                if !field::edit(
                    &self.keymap,
                    &mut palette.query,
                    keystroke,
                    field::WORDS,
                    paste,
                ) {
                    return;
                }
                palette.active = 0;
            }
        }
        if let Some(palette) = &self.overlays.palette {
            palette.scroll.scroll_to_item(palette.active);
        }
        self.redraw_chrome(cx);
    }

    /// Runs the palette row at `index`, unless it cannot run now.
    pub(super) fn choose_entry(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let chosen = self
            .palette_entries()
            .get(index)
            .filter(|entry| entry.blocked.is_none())
            .map(|entry| entry.spec.command.clone());
        if let Some(command) = chosen {
            self.overlays.palette = None;
            self.run(command, window, cx);
        }
    }

    fn filter_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) {
        match self
            .keymap
            .lookup(Scope::GuiFilter, commands::stroke(keystroke))
        {
            Some(Key::Back) => {
                self.list.filters.remove(&self.list.tab);
                self.list.filtering = false;
                self.filtered();
            }
            Some(Key::ApplyFilter) => self.list.filtering = false,
            Some(Key::FilterDown) => self.move_cursor(Motion::Down),
            Some(Key::FilterUp) => self.move_cursor(Motion::Up),
            _ => {
                let text = self.list.filters.entry(self.list.tab).or_default();
                let paste = || cx.read_from_clipboard().and_then(|item| item.text());
                if !field::edit(&self.keymap, text, keystroke, field::WORDS, paste) {
                    return;
                }
                self.filtered();
            }
        }
        cx.notify();
    }

    /// Applies a changed filter to the rows on screen.
    pub(super) fn filtered(&mut self) {
        self.list.asked = None;
        if self.list.tab == Tab::Artifacts {
            self.refresh_rows();
        }
        if let Some(scroll) = self.list_scroll(self.list.tab) {
            scroll.scroll_to_item(0, ScrollStrategy::Top);
        }
    }

    pub(super) fn focus_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.blocked(&Command::FocusFilter).is_none() {
            window.focus(&self.chrome.focus);
            self.overlays.menu = None;
            self.list.filtering = true;
            cx.notify();
        }
    }

    pub(super) fn go_to(&mut self, tab: Tab, cx: &mut Context<Self>) {
        if self.list.tab != tab {
            self.list.filtering = false;
            self.list.asked = None;
        }
        self.list.tab = tab;
        self.ensure_map(cx);
        if tab == Tab::Duplicates {
            self.load_duplicates(cx);
        }
        cx.notify();
    }

    /// Why a command cannot run right now, or `None` when it can.
    pub(super) fn blocked(&self, command: &Command) -> Option<&'static str> {
        self.blocked_with(command, &OnceCell::new())
    }

    /// Like `blocked`, finding the cursor row at most once in `cursor`, so a caller asking about
    /// many commands looks it up once.
    fn blocked_with(
        &self,
        command: &Command,
        cursor: &OnceCell<Option<Target>>,
    ) -> Option<&'static str> {
        let started = !matches!(self.session.phase, Phase::Start);
        let shown = self.report().is_some();
        match command {
            Command::Palette
            | Command::Quit
            | Command::Shortcuts
            | Command::Settings
            | Command::ZoomIn
            | Command::ZoomOut
            | Command::ResetZoom => None,
            Command::GoTo(_)
            | Command::NextTab
            | Command::PreviousTab
            | Command::Sidebar
            | Command::Rescan
            | Command::NewScan
            | Command::StopScan
            | Command::FindDuplicates
            | Command::StopDuplicates
            | Command::Scan(_) => self.blocked_session(command, started, shown),
            _ if !shown => Some(NEEDS_SCAN),
            Command::SpaceMap
            | Command::MapIn
            | Command::MapOut
            | Command::MapTop
            | Command::MapMove(_)
            | Command::MapToggle => self.blocked_map(command),
            Command::Trash
            | Command::ExtendDown
            | Command::ExtendUp
            | Command::Visual
            | Command::SelectAll
            | Command::SelectNone
            | Command::Toggle
            | Command::SelectWins => self.blocked_selection(command, cursor),
            Command::FocusFilter
            | Command::Gc
            | Command::GcAll
            | Command::Branches
            | Command::BranchesAll
            | Command::Restore
            | Command::Exclude
            | Command::CopyRerun
            | Command::Down
            | Command::Up
            | Command::First
            | Command::Last
            | Command::PageDown
            | Command::PageUp
            | Command::Reveal
            | Command::CopyPath
            | Command::OpenMenu => self.blocked_row_or_git(command, cursor),
        }
    }

    fn blocked_session(
        &self,
        command: &Command,
        started: bool,
        shown: bool,
    ) -> Option<&'static str> {
        match command {
            Command::GoTo(_) | Command::NextTab | Command::PreviousTab | Command::Sidebar => {
                (!started).then_some(START_FIRST)
            }
            Command::Rescan if !started || self.session.roots.is_empty() => Some(START_FIRST),
            Command::NewScan if !started => Some("The start screen is already open"),
            Command::Rescan | Command::NewScan => self.wait(),
            Command::StopScan => (!matches!(self.session.phase, Phase::Loading(_))
                && !self.refreshing())
            .then_some("No scan is running"),
            Command::FindDuplicates => (!shown).then_some(NEEDS_SCAN),
            Command::StopDuplicates => self
                .session
                .search
                .is_none()
                .then_some("No duplicate search is running"),
            Command::Scan(path) => self.wait().or_else(|| {
                self.start
                    .recent
                    .is_gone(path)
                    .then_some("This folder is missing")
            }),
            _ => None,
        }
    }

    fn blocked_map(&self, command: &Command) -> Option<&'static str> {
        match command {
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
                .list
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
                    self.hold()
                } else {
                    Some("Only an item tile can be selected")
                }
            }
            _ => None,
        }
    }

    fn blocked_selection(
        &self,
        command: &Command,
        cursor: &OnceCell<Option<Target>>,
    ) -> Option<&'static str> {
        let cursor = || cursor.get_or_init(|| self.cursor_target()).as_ref();
        match command {
            Command::Trash => self.trash_lock().or_else(|| self.hold()).or_else(|| {
                self.list
                    .selection
                    .is_empty()
                    .then_some("Select items first")
            }),
            Command::ExtendDown | Command::ExtendUp | Command::Visual => {
                if !self.list.tab.selects() {
                    Some(SELECT_LISTS)
                } else if self.map_active() {
                    Some(LISTS_ONLY)
                } else {
                    self.hold()
                        .or_else(|| cursor().is_none().then_some(NO_ROWS))
                }
            }
            Command::SelectAll | Command::SelectNone | Command::Toggle => {
                if self.list.tab.selects() {
                    self.hold()
                } else {
                    Some(SELECT_LISTS)
                }
            }
            Command::SelectWins => self.hold().or_else(|| self.wins_blocked()),
            _ => None,
        }
    }

    fn blocked_row_or_git(
        &self,
        command: &Command,
        cursor: &OnceCell<Option<Target>>,
    ) -> Option<&'static str> {
        let cursor = || cursor.get_or_init(|| self.cursor_target()).as_ref();
        let repo = || match cursor() {
            Some(Target::Repo(root)) if self.list.tab == Tab::Repos => {
                Some(GitScope::One(root.clone()))
            }
            _ => None,
        };
        match command {
            Command::FocusFilter => (self.list.tab == Tab::Overview).then_some(LISTS_ONLY),
            Command::Gc => self.gc_blocked(repo().as_ref()),
            Command::GcAll => self.gc_blocked(self.in_repos().then_some(&GitScope::All)),
            Command::Branches => self.branches_blocked(repo().as_ref()),
            Command::BranchesAll => {
                self.branches_blocked(self.in_repos().then_some(&GitScope::All))
            }
            Command::Restore | Command::Exclude | Command::CopyRerun => {
                let Some(target) = cursor() else {
                    return Some(NO_ROWS);
                };
                self.menu_unavailable(
                    target,
                    match command {
                        Command::Restore => Choice::Restore,
                        Command::Exclude => Choice::Exclude,
                        _ => Choice::CopyCommand,
                    },
                )
            }
            Command::Reveal | Command::CopyPath | Command::OpenMenu if self.map_active() => {
                cursor().is_none().then_some("Focus an item tile first")
            }
            Command::Reveal if matches!(cursor(), Some(Target::Item(Source::Docker, ..))) => {
                Some(NOT_ON_DISK)
            }
            Command::Down
            | Command::Up
            | Command::First
            | Command::Last
            | Command::PageDown
            | Command::PageUp
            | Command::Reveal
            | Command::CopyPath
            | Command::OpenMenu => {
                if self.list.tab == Tab::Overview {
                    Some(LISTS_ONLY)
                } else {
                    cursor().is_none().then_some(NO_ROWS)
                }
            }
            _ => None,
        }
    }

    fn in_repos(&self) -> bool {
        self.list.tab == Tab::Repos
    }

    /// Why work that replaces the results has to wait, or `None` when nothing is running.
    /// Why the lists cannot be selected in right now. A running scan or cleanup does not stand
    /// in the way, only a view without results.
    pub(super) fn hold(&self) -> Option<&'static str> {
        if self.restoring() {
            Some(WAIT_FOR_RESTORE)
        } else if matches!(self.session.phase, Phase::Loading(_)) {
            Some(WAIT_FOR_SCAN)
        } else {
            None
        }
    }

    pub(super) fn wait(&self) -> Option<&'static str> {
        if self.restoring() {
            Some(WAIT_FOR_RESTORE)
        } else if self.session.cleaning.is_some() {
            Some(WAIT_FOR_CLEANUP)
        } else if self.git_running() {
            Some(repos::GIT_RUNNING)
        } else if matches!(self.session.phase, Phase::Loading(_)) {
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
        if command != Command::SelectAll {
            self.list.asked = None;
        }
        let times = std::mem::take(&mut self.list.count).parse::<usize>().ok();
        let motion = self.motion(&command, times);
        let extends = matches!(command, Command::ExtendDown | Command::ExtendUp)
            || (self.list.visual && motion.is_some());
        if !extends && !matches!(command, Command::Visual) {
            self.list.range = None;
            self.list.visual &= motion.is_some();
        }
        if let Some(motion) = motion {
            if extends {
                self.extend(motion);
            } else {
                self.move_cursor(motion);
            }
            cx.notify();
            return;
        }
        match command {
            Command::Palette
            | Command::Shortcuts
            | Command::Settings
            | Command::GoTo(_)
            | Command::NextTab
            | Command::PreviousTab
            | Command::Rescan
            | Command::NewScan
            | Command::StopScan
            | Command::FindDuplicates
            | Command::StopDuplicates
            | Command::Quit
            | Command::ZoomIn
            | Command::ZoomOut
            | Command::ResetZoom
            | Command::Sidebar
            | Command::Scan(_) => self.run_app_command(command, cx),
            Command::Visual
            | Command::Toggle
            | Command::SelectAll
            | Command::SelectNone
            | Command::SelectWins
            | Command::Trash => self.run_selection_command(command, cx),
            Command::Reveal
            | Command::CopyPath
            | Command::Restore
            | Command::Exclude
            | Command::CopyRerun
            | Command::OpenMenu
            | Command::FocusFilter => self.run_row_command(command, window, cx),
            Command::SpaceMap
            | Command::MapIn
            | Command::MapOut
            | Command::MapTop
            | Command::MapMove(_)
            | Command::MapToggle => self.run_map_command(command, cx),
            Command::Gc | Command::GcAll | Command::Branches | Command::BranchesAll => {
                self.run_git_command(command, cx);
            }
            Command::Down
            | Command::Up
            | Command::ExtendDown
            | Command::ExtendUp
            | Command::First
            | Command::Last
            | Command::PageDown
            | Command::PageUp => {}
        }
        cx.notify();
    }

    fn run_app_command(&mut self, command: Command, cx: &mut Context<Self>) {
        match command {
            Command::Palette => self.open_palette(cx),
            Command::Shortcuts => {
                self.overlays.palette = None;
                self.overlays.shortcuts = true;
                self.overlays
                    .shortcuts_scroll
                    .set_offset(point(px(0.), px(0.)));
            }
            Command::Settings => self.open_settings(cx),
            Command::GoTo(tab) => self.go_to(tab, cx),
            Command::NextTab => self.go_to(self.list.tab.next(), cx),
            Command::PreviousTab => self.go_to(self.list.tab.previous(), cx),
            Command::Rescan => self.rescan(cx),
            Command::NewScan => self.new_scan(cx),
            Command::StopScan => self.stop_scan(cx),
            Command::FindDuplicates => {
                self.find_duplicates(cx);
                self.go_to(Tab::Duplicates, cx);
            }
            Command::StopDuplicates => self.stop_duplicates(cx),
            Command::Quit => cx.quit(),
            Command::ZoomIn => self.set_zoom(zoom::zoomed_in(self.config.appearance.zoom), cx),
            Command::ZoomOut => self.set_zoom(zoom::zoomed_out(self.config.appearance.zoom), cx),
            Command::ResetZoom => self.set_zoom(kuizer::config::DEFAULT_ZOOM, cx),
            Command::Sidebar => {
                self.chrome.collapsed = !self.chrome.collapsed;
            }
            Command::Scan(path) => self.scan_folder(&path, cx),
            _ => {}
        }
    }

    fn run_selection_command(&mut self, command: Command, cx: &mut Context<Self>) {
        match command {
            Command::Visual => self.toggle_visual(),
            Command::Toggle => {
                if let Some(Target::Item(source, id, _)) = self.cursor_target() {
                    self.toggle(source, id, cx);
                    self.advance_past_selected();
                }
            }
            Command::SelectAll => self.select_step(cx),
            Command::SelectNone => {
                for (key, _) in self.list_items() {
                    self.list.selection.remove(&key);
                }
            }
            Command::SelectWins => self.select_wins(cx),
            Command::Trash => self.trash(cx),
            _ => {}
        }
    }

    fn run_row_command(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        match command {
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
            Command::Restore | Command::Exclude | Command::CopyRerun => {
                if let Some(target) = self.cursor_target() {
                    let choice = match command {
                        Command::Restore => Choice::Restore,
                        Command::Exclude => Choice::Exclude,
                        _ => Choice::CopyCommand,
                    };
                    self.run_choice(choice, target, cx);
                }
            }
            Command::OpenMenu => self.open_menu_at_cursor(window, cx),
            Command::FocusFilter => self.focus_filter(window, cx),
            _ => {}
        }
    }

    fn run_map_command(&mut self, command: Command, cx: &mut Context<Self>) {
        match command {
            Command::SpaceMap => {
                if self.list.tab == Tab::Overview {
                    self.list.map.shown = !self.list.map.shown;
                } else {
                    self.go_to(Tab::Overview, cx);
                    self.list.map.shown = true;
                }
                self.ensure_map(cx);
            }
            Command::MapIn => {
                if let Some(key) = self.map_focused().map(|node| node.key.clone()) {
                    self.map_open(&key);
                }
            }
            Command::MapOut => self.map_back(self.list.map.path.len().saturating_sub(1)),
            Command::MapTop => self.map_back(0),
            Command::MapMove(direction) => self.map_move(direction),
            Command::MapToggle => self.map_toggle(cx),
            _ => {}
        }
    }

    fn run_git_command(&mut self, command: Command, cx: &mut Context<Self>) {
        match command {
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
            _ => {}
        }
    }

    /// Leaves the results on screen and scans a recent folder instead.
    fn scan_folder(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.list.selection.clear();
        self.list.rows.clear();
        self.overlays.toast = None;
        self.session.stale = None;
        self.set_phase(Phase::Start, cx);
        self.show_folder(path);
        self.start.whole_home = false;
        self.start.folder_error = None;
        self.start_scan(cx);
    }

    pub(super) fn target_exists(&self, target: &Target) -> bool {
        if self.report().is_none() {
            return false;
        }
        let tab = target.tab();
        let hint = self.row_hint(tab);
        match target {
            Target::Item(Source::Artifact, id, _) => self.artifact_row(*id, hint).is_some(),
            _ => self
                .listed_row(Some(&*self.targets(tab)), target, hint)
                .is_some(),
        }
    }

    /// The row the cursor of a list was last found at, checked first the next time it is
    /// looked up, so a cursor that has not moved is found in constant time in any list.
    pub(super) fn row_hint(&self, tab: Tab) -> &Cell<usize> {
        &self.list.row_hints[tab as usize]
    }

    /// Where an artifact sits in the rows on screen. `hint` is checked first and remembers the
    /// answer, so finding a cursor or an anchor that has not moved costs the same in any list.
    fn artifact_row(&self, id: u64, hint: &Cell<usize>) -> Option<usize> {
        let report = self.report()?;
        let at = |row: usize| {
            self.list
                .rows
                .get(row)
                .is_some_and(|&index| report.candidates[index].id == id)
        };
        if at(hint.get()) {
            return Some(hint.get());
        }
        let row = (0..self.list.rows.len()).find(|&row| at(row))?;
        hint.set(row);
        Some(row)
    }

    /// The rows of a list for a cursor move: `None` for the artifact list, which is read through
    /// its row indices instead, else every target.
    fn listing(&self, tab: Tab) -> Option<Rc<[Target]>> {
        (tab != Tab::Artifacts).then(|| self.targets(tab))
    }

    fn listed_count(&self, listed: Option<&[Target]>) -> usize {
        listed.map_or(self.list.rows.len(), <[Target]>::len)
    }

    pub(super) fn listed_row(
        &self,
        listed: Option<&[Target]>,
        target: &Target,
        hint: &Cell<usize>,
    ) -> Option<usize> {
        match (listed, target) {
            (Some(listed), _) => {
                if listed.get(hint.get()) == Some(target) {
                    return Some(hint.get());
                }
                let row = listed.iter().position(|row| row == target)?;
                hint.set(row);
                Some(row)
            }
            (None, Target::Item(Source::Artifact, id, _)) => self.artifact_row(*id, hint),
            (None, _) => None,
        }
    }

    fn listed_target(&self, listed: Option<&[Target]>, row: usize) -> Option<Target> {
        match listed {
            Some(listed) => listed.get(row).cloned(),
            None => {
                let candidate = &self.report()?.candidates[*self.list.rows.get(row)?];
                Some(Target::Item(
                    Source::Artifact,
                    candidate.id,
                    candidate.path.clone(),
                ))
            }
        }
    }

    fn listed_key(&self, listed: Option<&[Target]>, row: usize) -> Option<(Source, u64)> {
        match listed {
            Some(listed) => match listed.get(row)? {
                Target::Item(source, id, _) => Some((*source, *id)),
                _ => None,
            },
            None => Some((
                Source::Artifact,
                self.report()?.candidates[*self.list.rows.get(row)?].id,
            )),
        }
    }

    fn first_target(&self) -> Option<Target> {
        let report = self.report()?;
        match self.list.tab {
            Tab::Artifacts => self.list.rows.first().map(|&index| {
                let candidate = &report.candidates[index];
                Target::Item(Source::Artifact, candidate.id, candidate.path.clone())
            }),
            tab => self.targets(tab).first().cloned(),
        }
    }

    /// The keyboard cursor of the current list: the last row moved to or used, else its first
    /// row. The item menu's Menu key and Shift+F10 open on it.
    pub(super) fn cursor_target(&self) -> Option<Target> {
        if self.list.tab == Tab::Overview {
            return self.map_target().filter(|_| self.map_active());
        }
        self.list
            .cursors
            .get(&self.list.tab)
            .filter(|target| self.target_exists(target))
            .cloned()
            .or_else(|| self.first_target())
    }

    /// A click on a row toggles it, with or without Ctrl. A Shift+click already selected its
    /// range when the button went down, so it toggles nothing.
    pub(super) fn click_row(
        &mut self,
        source: Source,
        id: u64,
        event: &ClickEvent,
        cx: &mut Context<Self>,
    ) {
        if !self.busy() && !event.modifiers().shift {
            self.toggle(source, id, cx);
        }
    }

    /// Selects every row from the cursor row to this one, as a Shift+click does.
    pub(super) fn select_to(&mut self, target: &Target) {
        let listed = self.listing(target.tab());
        let listed = listed.as_deref();
        let rows = (
            self.cursor_target()
                .and_then(|cursor| self.listed_row(listed, &cursor, self.row_hint(self.list.tab))),
            self.listed_row(listed, target, &Cell::new(0)),
        );
        let (Some(from), Some(to)) = rows else {
            return;
        };
        for row in from.min(to)..=from.max(to) {
            if let Some(key) = self.listed_key(listed, row) {
                self.select(key);
            }
        }
    }

    fn move_cursor(&mut self, motion: Motion) {
        let tab = self.list.tab;
        let listed = self.listing(tab);
        let listed = listed.as_deref();
        let count = self.listed_count(listed);
        if count == 0 {
            return;
        }
        let current = self
            .cursor_target()
            .and_then(|cursor| self.listed_row(listed, &cursor, self.row_hint(self.list.tab)))
            .unwrap_or(0);
        let next = step(motion, current, count, self.page_size());
        self.place_cursor(listed, current, next);
    }

    fn advance_past_selected(&mut self) {
        let listed = self.listing(self.list.tab);
        let listed = listed.as_deref();
        let count = self.listed_count(listed);
        let Some(current) = self
            .cursor_target()
            .and_then(|cursor| self.listed_row(listed, &cursor, self.row_hint(self.list.tab)))
        else {
            return;
        };
        if current + 1 >= count {
            return;
        }
        let next = (current + 1..count)
            .find(|&row| {
                self.listed_key(listed, row)
                    .is_some_and(|key| !self.list.selection.contains(&key))
            })
            .unwrap_or(current + 1);
        self.place_cursor(listed, current, next);
    }

    fn place_cursor(&mut self, listed: Option<&[Target]>, current: usize, next: usize) {
        let Some(target) = self.listed_target(listed, next) else {
            return;
        };
        self.list.cursors.insert(self.list.tab, target);
        self.row_hint(self.list.tab).set(next);
        if let Some(scroll) = self.list_scroll(self.list.tab) {
            let (line, strategy) = if next < current {
                (self.heading_line(next), ScrollStrategy::Top)
            } else {
                (self.drawn_line(next), ScrollStrategy::Bottom)
            };
            scroll.scroll_to_item(line, strategy);
        }
    }

    /// Moves the cursor and grows or shrinks the Shift selection to span the anchor and the
    /// cursor. Only the rows between the old and the new cursor change, so a step costs the
    /// same in any list size.
    fn extend(&mut self, motion: Motion) {
        let tab = self.list.tab;
        let listed = self.listing(tab);
        let listed = listed.as_deref();
        let count = self.listed_count(listed);
        let Some(cursor) = self.cursor_target().filter(|_| count > 0) else {
            return;
        };
        let current = self
            .listed_row(listed, &cursor, self.row_hint(tab))
            .unwrap_or(0);
        let mut range = self
            .list
            .range
            .take()
            .filter(|range| {
                range.tab == tab
                    && self
                        .listed_row(listed, &range.anchor, &range.anchor_row)
                        .is_some()
            })
            .unwrap_or_else(|| Span {
                tab,
                anchor: cursor.clone(),
                anchor_row: Cell::new(current),
                added: HashSet::new(),
            });
        let anchor = self
            .listed_row(listed, &range.anchor, &range.anchor_row)
            .unwrap_or(current);
        let next = step(motion, current, count, self.page_size());
        self.place_cursor(listed, current, next);
        let spans = |end: usize, index: usize| anchor.min(end) <= index && index <= anchor.max(end);
        for index in current.min(next)..=current.max(next) {
            let Some(key) = self.listed_key(listed, index) else {
                continue;
            };
            if spans(next, index) {
                if self.select(key) {
                    range.added.insert(key);
                }
            } else if spans(current, index) && range.added.remove(&key) {
                self.list.selection.remove(&key);
            }
        }
        self.list.range = Some(range);
    }

    /// The drawn line of a list row. Only the duplicate list draws more lines than rows, as
    /// it puts a heading above each group.
    fn drawn_line(&self, row: usize) -> usize {
        if self.list.tab != Tab::Duplicates {
            return row;
        }
        self.listed(Tab::Duplicates)
            .line_of
            .get(row)
            .copied()
            .unwrap_or(row)
    }

    /// The line to scroll to the top for a row: its group's heading when the row is the first
    /// of its group, so moving up never hides which group a duplicate belongs to.
    fn heading_line(&self, row: usize) -> usize {
        let line = self.drawn_line(row);
        let heading = line.checked_sub(1).filter(|&above| {
            self.list.tab == Tab::Duplicates
                && matches!(
                    self.listed(Tab::Duplicates).lines.get(above),
                    Some(Line::Group(_))
                )
        });
        heading.unwrap_or(line)
    }

    /// How many rows PageUp and PageDown move: the rows that fit in the list, less one.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a positive row count far below usize::MAX"
    )]
    fn page_size(&self) -> usize {
        let Some(view) = self
            .list_scroll(self.list.tab)
            .map(|scroll| scroll.0.borrow().base_handle.bounds())
        else {
            return 10;
        };
        match self.list.cursor_bounds.get() {
            Some(row) if row.size.height > px(0.) => {
                ((view.size.height / row.size.height).floor().max(2.) as usize) - 1
            }
            _ => 10,
        }
    }

    /// The selectable items of the current list, after its filters.
    pub(super) fn list_items(&self) -> Vec<((Source, u64), Safety)> {
        let Some(report) = self.report() else {
            return Vec::new();
        };
        let item = |source, candidate: &Candidate| ((source, candidate.id), candidate.rule.safety);
        match self.list.tab {
            Tab::Artifacts => self
                .list
                .rows
                .iter()
                .map(|&index| item(Source::Artifact, &report.candidates[index]))
                .collect(),
            Tab::Caches => self
                .listed(Tab::Caches)
                .slots
                .iter()
                .filter_map(|&(at, _)| report.caches.get(at))
                .map(|(_, candidate)| item(Source::Cache, candidate))
                .collect(),
            Tab::Toolchains => self
                .listed(Tab::Toolchains)
                .slots
                .iter()
                .filter_map(|&(at, _)| report.toolchains.get(at))
                .map(|(_, candidate)| item(Source::Toolchain, candidate))
                .collect(),
            Tab::Docker => self
                .listed(Tab::Docker)
                .slots
                .iter()
                .filter_map(|&(at, _)| report.docker.get(at))
                .map(|(_, candidate)| item(Source::Docker, candidate))
                .collect(),
            _ => Vec::new(),
        }
    }

    fn asked_holds(&self, asked: &Asked) -> bool {
        self.report().is_some()
            && asked.tab == self.list.tab
            && asked.report == self.session.report_generation
            && asked.selection == self.list.selection.generation()
    }

    pub(super) fn asking(&self) -> Option<usize> {
        self.list
            .asked
            .as_ref()
            .filter(|asked| self.asked_holds(asked))
            .map(|asked| asked.left)
    }

    pub(super) fn select_step(&mut self, cx: &mut Context<Self>) {
        let items = self.list_items();
        let asked = self.list.asked.take().is_some_and(|asked| {
            self.asked_holds(&asked) && items.iter().map(|(key, _)| key).eq(&asked.keys)
        });
        let mut coverage = Coverage::default();
        for (key, safety) in &items {
            let selected = usize::from(self.list.selection.contains(key));
            match safety {
                Safety::Safe => {
                    coverage.safe += 1;
                    coverage.safe_on += selected;
                }
                Safety::Review => {
                    coverage.review += 1;
                    coverage.review_on += selected;
                }
            }
        }
        match coverage.press(asked) {
            BulkStep::Safe => {
                for (key, safety) in items {
                    if safety == Safety::Safe {
                        self.select(key);
                    }
                }
            }
            BulkStep::Everything => {
                for (key, _) in items {
                    self.select(key);
                }
            }
            BulkStep::Clear => {
                for (key, _) in &items {
                    self.list.selection.remove(key);
                }
            }
            BulkStep::Ask => {
                self.list.asked = Some(Asked {
                    tab: self.list.tab,
                    report: self.session.report_generation,
                    selection: self.list.selection.generation(),
                    left: coverage.review - coverage.review_on,
                    keys: items.into_iter().map(|(key, _)| key).collect(),
                });
            }
            BulkStep::Nothing => {}
        }
        cx.notify();
    }

    /// Rows shown in a list and rows it has without its text filter.
    pub(super) fn counts(&self, tab: Tab) -> (usize, usize) {
        let Some(report) = self.report() else {
            return (0, 0);
        };
        let shown = if tab == Tab::Artifacts {
            self.list.rows.len()
        } else {
            self.listed(tab).slots.len()
        };
        let total = match tab {
            Tab::Overview => 0,
            Tab::Artifacts => report.candidates.len(),
            Tab::Caches => report.caches.len(),
            Tab::Toolchains => report.toolchains.len(),
            Tab::Docker => report.docker.len(),
            Tab::Repos => report.repos.len(),
            Tab::Duplicates => report.duplicates.as_ref().map_or(0, |found| {
                found.groups.iter().map(|group| group.members.len()).sum()
            }),
            Tab::Issues => report.issues.len(),
            Tab::History => report
                .history
                .as_ref()
                .map_or(0, |view| view.delete_runs().count()),
        };
        (shown, total)
    }
}

#[cfg(test)]
mod tests;
