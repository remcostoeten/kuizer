use std::path::PathBuf;
use std::sync::OnceLock;

use gpui::{App, KeyBinding, Keystroke, SharedString, actions};
use kuizer::keymap::{Action, Keymap, Modifiers, Overrides, Scope, Stroke};
use kuizer::platform::display_path;

use crate::squarify::Direction;
use crate::tab::Tab;

actions!(kuizer, [ZoomIn, ZoomOut, ResetZoom, TogglePalette, Quit]);

/// Everything the keyboard, the command palette and the shortcuts overlay can do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Palette,
    Quit,
    Shortcuts,
    Settings,
    GoTo(Tab),
    NextTab,
    PreviousTab,
    Rescan,
    NewScan,
    StopScan,
    Down,
    Up,
    ExtendDown,
    ExtendUp,
    Visual,
    First,
    Last,
    PageDown,
    PageUp,
    Toggle,
    SelectAll,
    SelectNone,
    SelectWins,
    Trash,
    Reveal,
    CopyPath,
    CopyRerun,
    Exclude,
    Restore,
    OpenMenu,
    FocusFilter,
    ZoomIn,
    ZoomOut,
    ResetZoom,
    Sidebar,
    SpaceMap,
    MapIn,
    MapOut,
    MapTop,
    MapMove(Direction),
    MapToggle,
    Scan(PathBuf),
    Gc,
    GcAll,
    Branches,
    BranchesAll,
    FindDuplicates,
    StopDuplicates,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    General,
    Sections,
    Lists,
    Map,
    Git,
    View,
}

impl Group {
    pub const ALL: [Group; 6] = [
        Self::General,
        Self::Sections,
        Self::Lists,
        Self::Map,
        Self::Git,
        Self::View,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Sections => "Sections",
            Self::Lists => "Lists",
            Self::Map => "Space map",
            Self::Git => "Git repositories",
            Self::View => "View",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Spec {
    pub command: Command,
    pub label: SharedString,
    pub group: Group,
    pub palette: bool,
    pub global: bool,
}

impl Spec {
    fn new(command: Command, label: impl Into<SharedString>, group: Group) -> Self {
        Self {
            command,
            label: label.into(),
            group,
            palette: true,
            global: false,
        }
    }

    fn hidden(mut self) -> Self {
        self.palette = false;
        self
    }

    fn global(mut self) -> Self {
        self.global = true;
        self
    }
}

pub fn table() -> &'static [Spec] {
    static TABLE: OnceLock<Vec<Spec>> = OnceLock::new();
    TABLE.get_or_init(build_table)
}

fn build_table() -> Vec<Spec> {
    use Group::{General, Git, Lists, Map, Sections, View};
    let mut specs = vec![
        Spec::new(Command::Palette, "Command palette", General)
            .hidden()
            .global(),
        Spec::new(Command::Quit, "Quit", General).global(),
        Spec::new(Command::Shortcuts, "Keyboard shortcuts", General),
        Spec::new(Command::Settings, "Settings", General),
        Spec::new(Command::Rescan, "Rescan", General),
        Spec::new(Command::NewScan, "New scan", General),
        Spec::new(
            Command::StopScan,
            "Stop the scan and keep what it found",
            General,
        ),
        Spec::new(Command::FindDuplicates, "Find duplicate files", General),
        Spec::new(
            Command::StopDuplicates,
            "Stop the duplicate search",
            General,
        ),
    ];
    specs.extend(Tab::ALL.into_iter().map(|tab| {
        Spec::new(
            Command::GoTo(tab),
            format!("Go to {}", tab.label()),
            Sections,
        )
    }));
    specs.extend([
        Spec::new(Command::SpaceMap, "Show or hide the space map", Sections),
        Spec::new(Command::NextTab, "Next section", Sections).hidden(),
        Spec::new(Command::PreviousTab, "Previous section", Sections).hidden(),
        Spec::new(Command::Down, "Move down", Lists).hidden(),
        Spec::new(Command::Up, "Move up", Lists).hidden(),
        Spec::new(Command::ExtendDown, "Extend the selection down", Lists).hidden(),
        Spec::new(Command::ExtendUp, "Extend the selection up", Lists).hidden(),
        Spec::new(Command::Visual, "Visual mode: motions select rows", Lists),
        Spec::new(Command::First, "First row", Lists).hidden(),
        Spec::new(Command::Last, "Last row", Lists).hidden(),
        Spec::new(Command::PageDown, "Page down", Lists).hidden(),
        Spec::new(Command::PageUp, "Page up", Lists).hidden(),
        Spec::new(Command::Toggle, "Select or deselect the row", Lists).hidden(),
        Spec::new(Command::SelectAll, "Select all, safe first", Lists),
        Spec::new(Command::SelectNone, "Select none in this list", Lists),
        Spec::new(
            Command::SelectWins,
            "Select the biggest safe artifacts",
            Lists,
        ),
        Spec::new(Command::Trash, "Delete selected", Lists),
        Spec::new(Command::Reveal, "Reveal in file manager", Lists),
        Spec::new(Command::CopyPath, "Copy path", Lists),
        Spec::new(Command::CopyRerun, "Copy command", Lists),
        Spec::new(Command::Exclude, "Always exclude", Lists),
        Spec::new(Command::Restore, "Restore", Lists),
        Spec::new(Command::OpenMenu, "Open item menu", Lists),
        Spec::new(Command::FocusFilter, "Filter this list", Lists),
        Spec::new(Command::MapIn, "Open the folder tile", Map),
        Spec::new(Command::MapOut, "Zoom out one level", Map),
        Spec::new(Command::MapTop, "Zoom out to the top", Map),
        Spec::new(Command::MapToggle, "Select or deselect the tile", Map).hidden(),
        Spec::new(
            Command::MapMove(Direction::Left),
            "Focus the tile to the left",
            Map,
        )
        .hidden(),
        Spec::new(
            Command::MapMove(Direction::Right),
            "Focus the tile to the right",
            Map,
        )
        .hidden(),
        Spec::new(Command::MapMove(Direction::Up), "Focus the tile above", Map).hidden(),
        Spec::new(
            Command::MapMove(Direction::Down),
            "Focus the tile below",
            Map,
        )
        .hidden(),
        Spec::new(Command::Gc, "Run git gc in the repository", Git),
        Spec::new(Command::GcAll, "Run git gc in every listed repository", Git),
        Spec::new(
            Command::Branches,
            "Delete merged branches of the repository",
            Git,
        ),
        Spec::new(
            Command::BranchesAll,
            "Delete every listed merged branch",
            Git,
        ),
        Spec::new(Command::ZoomIn, "Zoom in", View).global(),
        Spec::new(Command::ZoomOut, "Zoom out", View).global(),
        Spec::new(Command::ResetZoom, "Reset zoom", View).global(),
        Spec::new(Command::Sidebar, "Collapse or expand the sidebar", View),
    ]);
    specs
}

impl Command {
    pub fn binding(&self) -> Option<(Scope, Action)> {
        use Action as Key;
        let action = match *self {
            Self::Palette => Key::Palette,
            Self::Quit => Key::Quit,
            Self::Shortcuts => Key::Help,
            Self::Settings => Key::Settings,
            Self::GoTo(tab) => tab.action(),
            Self::NextTab => Key::NextTab,
            Self::PreviousTab => Key::PreviousTab,
            Self::Rescan => Key::Rescan,
            Self::NewScan => Key::NewScan,
            Self::StopScan => Key::StopScan,
            Self::Down => Key::Down,
            Self::Up => Key::Up,
            Self::ExtendDown => Key::ExtendDown,
            Self::ExtendUp => Key::ExtendUp,
            Self::Visual => Key::Visual,
            Self::First => Key::First,
            Self::Last => Key::Last,
            Self::PageDown => Key::PageDown,
            Self::PageUp => Key::PageUp,
            Self::Toggle => Key::Toggle,
            Self::SelectAll => Key::SelectAll,
            Self::SelectNone => Key::SelectNone,
            Self::SelectWins => Key::SelectWins,
            Self::Trash => Key::Delete,
            Self::Reveal => Key::Reveal,
            Self::CopyPath => Key::CopyPath,
            Self::CopyRerun => Key::CopyCommand,
            Self::Exclude => Key::Exclude,
            Self::Restore => Key::Restore,
            Self::OpenMenu => Key::OpenMenu,
            Self::FocusFilter => Key::FilterFocus,
            Self::ZoomIn => Key::ZoomIn,
            Self::ZoomOut => Key::ZoomOut,
            Self::ResetZoom => Key::ResetZoom,
            Self::Sidebar => Key::Sidebar,
            Self::SpaceMap => Key::SpaceMap,
            Self::MapIn => return Some((Scope::Map, Key::MapIn)),
            Self::MapOut => return Some((Scope::Map, Key::MapOut)),
            Self::MapTop => return Some((Scope::Map, Key::MapTop)),
            Self::MapMove(Direction::Left) => return Some((Scope::Map, Key::MapLeft)),
            Self::MapMove(Direction::Right) => return Some((Scope::Map, Key::MapRight)),
            Self::MapMove(Direction::Up) => return Some((Scope::Map, Key::MapUp)),
            Self::MapMove(Direction::Down) => return Some((Scope::Map, Key::MapDown)),
            Self::MapToggle => return Some((Scope::Map, Key::MapToggle)),
            Self::Scan(_) => return None,
            Self::Gc => Key::Gc,
            Self::GcAll => Key::GcAll,
            Self::Branches => Key::Branches,
            Self::BranchesAll => Key::BranchesAll,
            Self::FindDuplicates => Key::FindDuplicates,
            Self::StopDuplicates => Key::StopDuplicates,
        };
        Some((Scope::Gui, action))
    }
}

pub fn stroke(key: &Keystroke) -> Stroke<'_> {
    Stroke {
        key: &key.key,
        typed: key.key_char.as_deref(),
        modifiers: Modifiers {
            ctrl: key.modifiers.control,
            alt: key.modifiers.alt,
            shift: key.modifiers.shift,
            cmd: key.modifiers.platform,
            function: key.modifiers.function,
        },
    }
}

pub fn lookup(keys: &Keymap, keystroke: &Keystroke, map: bool) -> Option<Command> {
    let stroke = stroke(keystroke);
    let hit = if map {
        keys.lookup(Scope::Map, stroke)
            .map(|action| (Scope::Map, action))
    } else {
        None
    };
    let hit = hit.or_else(|| {
        keys.lookup(Scope::Gui, stroke)
            .map(|action| (Scope::Gui, action))
    })?;
    table()
        .iter()
        .find(|spec| spec.command.binding() == Some(hit))
        .map(|spec| spec.command.clone())
}

type Hint = (&'static str, Scope, &'static [Action]);

pub const HINTS: &[Hint] = &[
    (
        "Move in the palette",
        Scope::Palette,
        &[Action::PalettePrevious, Action::PaletteNext],
    ),
    (
        "Scroll this overlay",
        Scope::Gui,
        &[Action::Down, Action::Up, Action::PageDown, Action::PageUp],
    ),
    ("Run the command", Scope::Palette, &[Action::PaletteRun]),
    (
        "Leave the filter, keep it",
        Scope::GuiFilter,
        &[Action::ApplyFilter],
    ),
    (
        "Close, clear the filter or cancel",
        Scope::GuiFilter,
        &[Action::Back],
    ),
];

pub const CONTROLS: &[Hint] = &[
    (
        "Next or previous control",
        Scope::Control,
        &[Action::ControlNext, Action::ControlPrevious],
    ),
    (
        "Move along the controls",
        Scope::Control,
        &[Action::ControlForward, Action::ControlBack],
    ),
    ("Press the control", Scope::Control, &[Action::ControlPress]),
    ("Back to the list", Scope::Control, &[Action::Back]),
];

/// @name scans
/// @description One palette entry per recent folder, labelled with the folder.
///
/// @example
/// let entries = scans(&recent.paths);
pub fn scans(paths: &[PathBuf]) -> Vec<Spec> {
    paths
        .iter()
        .map(|path| {
            Spec::new(
                Command::Scan(path.clone()),
                format!("Scan {}", display_path(path)),
                Group::General,
            )
        })
        .collect()
}

pub fn bind_configured_keys(cx: &mut App, overrides: &Overrides) {
    let (keymap, _) = Keymap::resolve(overrides);
    let mut bindings = Vec::new();
    for spec in table().iter().filter(|spec| spec.global) {
        let Some((scope, action)) = spec.command.binding() else {
            continue;
        };
        for key in keymap.keys(scope, action) {
            bindings.push(match spec.command {
                Command::ZoomIn => KeyBinding::new(key, ZoomIn, None),
                Command::ZoomOut => KeyBinding::new(key, ZoomOut, None),
                Command::ResetZoom => KeyBinding::new(key, ResetZoom, None),
                Command::Palette => KeyBinding::new(key, TogglePalette, None),
                Command::Quit => KeyBinding::new(key, Quit, None),
                _ => continue,
            });
        }
    }
    cx.bind_keys(bindings);
}

#[cfg(test)]
pub fn bind_keys(cx: &mut App) {
    bind_configured_keys(cx, &Overrides::default());
}

/// @name label
/// @description How a key pattern is written on screen, such as `Ctrl+K`, `Shift+G` or `PgDn`.
///
/// @example
/// assert_eq!(label("pagedown"), "PgDn");
pub fn label(pattern: &str) -> String {
    let Ok(keystroke) = Keystroke::parse(pattern) else {
        return pattern.to_owned();
    };
    let modifiers = keystroke.modifiers;
    let mut parts: Vec<String> = Vec::new();
    if modifiers.control {
        parts.push("Ctrl".to_owned());
    }
    if modifiers.platform {
        parts.push("Cmd".to_owned());
    }
    if modifiers.alt {
        parts.push("Alt".to_owned());
    }
    if modifiers.shift {
        parts.push("Shift".to_owned());
    }
    let key = match keystroke.key.as_str() {
        "down" => "Down".to_owned(),
        "up" => "Up".to_owned(),
        "left" => "Left".to_owned(),
        "right" => "Right".to_owned(),
        "backspace" => "Backspace".to_owned(),
        "pagedown" => "PgDn".to_owned(),
        "pageup" => "PgUp".to_owned(),
        "home" => "Home".to_owned(),
        "end" => "End".to_owned(),
        "space" => "Space".to_owned(),
        "enter" => "Enter".to_owned(),
        "delete" => "Del".to_owned(),
        "escape" => "Esc".to_owned(),
        "tab" => "Tab".to_owned(),
        "menu" => "Menu".to_owned(),
        other => other.to_uppercase(),
    };
    parts.push(key);
    parts.join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(source: &str) -> Keystroke {
        Keystroke::parse(source).unwrap().with_simulated_ime()
    }

    #[test]
    fn labels_read_like_keys() {
        assert_eq!(label("pagedown"), "PgDn");
        assert_eq!(label("G"), "Shift+G");
        assert_eq!(label("j"), "J");
        assert_eq!(label("?"), "?");
        assert_eq!(label("shift-f10"), "Shift+F10");
        let palette = if cfg!(target_os = "macos") {
            "Cmd+K"
        } else {
            "Ctrl+K"
        };
        assert_eq!(label("secondary-k"), palette);
    }

    #[test]
    fn vim_motions_run_the_arrow_and_page_commands() {
        assert_eq!(
            lookup(&Keymap::default(), &key("ctrl-d"), false),
            Some(Command::PageDown)
        );
        assert_eq!(
            lookup(&Keymap::default(), &key("ctrl-u"), false),
            Some(Command::PageUp)
        );
        assert_eq!(
            lookup(&Keymap::default(), &key("h"), true),
            Some(Command::MapMove(Direction::Left))
        );
        assert_eq!(
            lookup(&Keymap::default(), &key("l"), true),
            Some(Command::MapMove(Direction::Right))
        );
        assert_eq!(lookup(&Keymap::default(), &key("h"), false), None);
    }

    #[test]
    fn map_keys_win_only_inside_the_map() {
        let enter = key("enter");
        assert_eq!(
            lookup(&Keymap::default(), &enter, true),
            Some(Command::MapIn)
        );
        assert_eq!(
            lookup(&Keymap::default(), &enter, false),
            Some(Command::Trash)
        );
        assert_eq!(
            lookup(&Keymap::default(), &key("left"), true),
            Some(Command::MapMove(Direction::Left))
        );
        assert_eq!(lookup(&Keymap::default(), &key("left"), false), None);
        assert_eq!(lookup(&Keymap::default(), &key("backspace"), false), None);
        assert_eq!(
            lookup(&Keymap::default(), &key("j"), true),
            Some(Command::MapMove(Direction::Down))
        );
        assert_eq!(
            lookup(&Keymap::default(), &key("k"), true),
            Some(Command::MapMove(Direction::Up))
        );
        assert_eq!(
            lookup(&Keymap::default(), &key("j"), false),
            Some(Command::Down)
        );
        assert_eq!(
            lookup(&Keymap::default(), &key("m"), false),
            Some(Command::SpaceMap)
        );
        assert_eq!(label("backspace"), "Backspace");
    }
}
