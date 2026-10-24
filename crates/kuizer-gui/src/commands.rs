use std::path::PathBuf;
use std::sync::OnceLock;

use gpui::{App, KeyBinding, Keystroke, SharedString, actions};
use kuizer::output::display_path;

use crate::squarify::Direction;
use crate::tab::Tab;

actions!(
    kuizer,
    [ZoomIn, ZoomOut, ResetZoom, Undo, TogglePalette, Quit]
);

/// Everything the keyboard, the command palette and the shortcuts overlay can do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Palette,
    Quit,
    Shortcuts,
    GoTo(Tab),
    NextTab,
    PreviousTab,
    Rescan,
    NewScan,
    Undo,
    Down,
    Up,
    ExtendDown,
    ExtendUp,
    First,
    Last,
    PageDown,
    PageUp,
    Toggle,
    SelectAll,
    SelectNone,
    Trash,
    Reveal,
    CopyPath,
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

/// One row of the command table: what it does, how it is called and which keys run it.
///
/// `keys` are shown and matched, `extra` are only matched. Keys use GPUI's keystroke syntax,
/// with `secondary` for Cmd on macOS and Ctrl elsewhere. A `global` command is bound as a GPUI
/// action, so it also works while a text field has the keyboard.
#[derive(Debug, Clone)]
pub struct Spec {
    pub command: Command,
    pub label: SharedString,
    pub keys: &'static [&'static str],
    pub extra: &'static [&'static str],
    pub group: Group,
    pub palette: bool,
    pub global: bool,
    parsed: Vec<Keystroke>,
}

impl Spec {
    fn new(command: Command, label: impl Into<SharedString>, group: Group) -> Self {
        Self {
            command,
            label: label.into(),
            keys: &[],
            extra: &[],
            group,
            palette: true,
            global: false,
            parsed: Vec::new(),
        }
    }

    fn keys(mut self, keys: &'static [&'static str]) -> Self {
        self.keys = keys;
        self.parse()
    }

    fn extra(mut self, extra: &'static [&'static str]) -> Self {
        self.extra = extra;
        self.parse()
    }

    fn parse(mut self) -> Self {
        self.parsed = self
            .keys
            .iter()
            .chain(self.extra)
            .filter_map(|pattern| Keystroke::parse(pattern).ok())
            .collect();
        self
    }

    fn hidden(mut self) -> Self {
        self.palette = false;
        self
    }

    fn global(mut self) -> Self {
        self.global = true;
        self
    }

    /// Whether one of this command's keys is the keystroke.
    pub fn answers(&self, keystroke: &Keystroke) -> bool {
        self.parsed.iter().any(|want| names(want, keystroke))
    }
}

/// @name table
/// @description The command table. The palette, the key handling, the GPUI key bindings and the
/// shortcuts overlay are all generated from it, so they cannot drift apart.
///
/// @example
/// let spec = table().iter().find(|spec| spec.answers(&keystroke));
pub fn table() -> &'static [Spec] {
    static TABLE: OnceLock<Vec<Spec>> = OnceLock::new();
    TABLE.get_or_init(build_table)
}

fn build_table() -> Vec<Spec> {
    use Group::{General, Git, Lists, Map, Sections, View};
    let mut specs = vec![
        Spec::new(Command::Palette, "Command palette", General)
            .keys(&["secondary-k"])
            .extra(&["secondary-shift-p"])
            .hidden()
            .global(),
        Spec::new(Command::Quit, "Quit", General)
            .keys(&["secondary-q"])
            .global(),
        Spec::new(Command::Shortcuts, "Keyboard shortcuts", General).keys(&["?"]),
        Spec::new(Command::Rescan, "Rescan", General).keys(&["secondary-r"]),
        Spec::new(Command::NewScan, "New scan", General).keys(&["secondary-n"]),
        Spec::new(Command::FindDuplicates, "Find duplicate files", General).keys(&["D"]),
        Spec::new(Command::Undo, "Undo last cleanup", General)
            .keys(&["secondary-z"])
            .global(),
    ];
    specs.extend(Tab::ALL.into_iter().map(|tab| {
        Spec::new(
            Command::GoTo(tab),
            format!("Go to {}", tab.label()),
            Sections,
        )
        .keys(tab.keys())
    }));
    specs.extend([
        Spec::new(Command::SpaceMap, "Show or hide the space map", Sections).keys(&["m"]),
        Spec::new(Command::NextTab, "Next section", Sections)
            .keys(&["ctrl-tab"])
            .hidden(),
        Spec::new(Command::PreviousTab, "Previous section", Sections)
            .keys(&["ctrl-shift-tab"])
            .hidden(),
        Spec::new(Command::Down, "Move down", Lists)
            .keys(&["j", "down"])
            .hidden(),
        Spec::new(Command::Up, "Move up", Lists)
            .keys(&["k", "up"])
            .hidden(),
        Spec::new(Command::ExtendDown, "Extend the selection down", Lists)
            .keys(&["J", "shift-down"])
            .hidden(),
        Spec::new(Command::ExtendUp, "Extend the selection up", Lists)
            .keys(&["K", "shift-up"])
            .hidden(),
        Spec::new(Command::First, "First row", Lists)
            .keys(&["g", "home"])
            .hidden(),
        Spec::new(Command::Last, "Last row", Lists)
            .keys(&["G", "end"])
            .hidden(),
        Spec::new(Command::PageDown, "Page down", Lists)
            .keys(&["pagedown"])
            .extra(&["ctrl-d"])
            .hidden(),
        Spec::new(Command::PageUp, "Page up", Lists)
            .keys(&["pageup"])
            .extra(&["ctrl-u"])
            .hidden(),
        Spec::new(Command::Toggle, "Select or deselect the row", Lists)
            .keys(&["space"])
            .hidden(),
        Spec::new(Command::SelectAll, "Select all in this list", Lists).keys(&["secondary-a"]),
        Spec::new(Command::SelectNone, "Select none in this list", Lists)
            .keys(&["secondary-shift-a"]),
        Spec::new(Command::Trash, "Move selected to trash", Lists).keys(&["enter", "delete"]),
        Spec::new(Command::Reveal, "Reveal in file manager", Lists).keys(&["o"]),
        Spec::new(Command::CopyPath, "Copy path", Lists).keys(&["y"]),
        Spec::new(Command::OpenMenu, "Open item menu", Lists).keys(&["menu", "shift-f10"]),
        Spec::new(Command::FocusFilter, "Filter this list", Lists).keys(&["/"]),
        Spec::new(Command::MapIn, "Open the folder tile", Map).keys(&["enter"]),
        Spec::new(Command::MapOut, "Zoom out one level", Map).keys(&["backspace", "escape"]),
        Spec::new(Command::MapTop, "Zoom out to the top", Map).keys(&["home"]),
        Spec::new(Command::MapToggle, "Select or deselect the tile", Map)
            .keys(&["space"])
            .hidden(),
        Spec::new(
            Command::MapMove(Direction::Left),
            "Focus the tile to the left",
            Map,
        )
        .keys(&["left"])
        .extra(&["h"])
        .hidden(),
        Spec::new(
            Command::MapMove(Direction::Right),
            "Focus the tile to the right",
            Map,
        )
        .keys(&["right"])
        .extra(&["l"])
        .hidden(),
        Spec::new(Command::MapMove(Direction::Up), "Focus the tile above", Map)
            .keys(&["up"])
            .hidden(),
        Spec::new(
            Command::MapMove(Direction::Down),
            "Focus the tile below",
            Map,
        )
        .keys(&["down"])
        .hidden(),
        Spec::new(Command::Gc, "Run git gc in the repository", Git).keys(&["c"]),
        Spec::new(Command::GcAll, "Run git gc in every listed repository", Git).keys(&["C"]),
        Spec::new(
            Command::Branches,
            "Delete merged branches of the repository",
            Git,
        )
        .keys(&["b"]),
        Spec::new(
            Command::BranchesAll,
            "Delete every listed merged branch",
            Git,
        )
        .keys(&["B"]),
        Spec::new(Command::ZoomIn, "Zoom in", View)
            .keys(&["secondary-="])
            .extra(&["secondary-+", "secondary-shift-="])
            .global(),
        Spec::new(Command::ZoomOut, "Zoom out", View)
            .keys(&["secondary--"])
            .global(),
        Spec::new(Command::ResetZoom, "Reset zoom", View)
            .keys(&["secondary-0"])
            .global(),
        Spec::new(Command::Sidebar, "Collapse or expand the sidebar", View).keys(&["secondary-b"]),
    ]);
    specs
}

/// @name lookup
/// @description The command a keystroke runs. While the space map has the keyboard its own keys
/// come first, so Enter opens a tile there instead of moving the selection to the trash; outside
/// the map its keys do nothing.
///
/// @example
/// let command = lookup(&keystroke, app.map_active());
pub fn lookup(keystroke: &Keystroke, map: bool) -> Option<Command> {
    let specs = table();
    let scoped = specs.iter().filter(|spec| map && spec.group == Group::Map);
    let general = specs.iter().filter(|spec| spec.group != Group::Map);
    scoped
        .chain(general)
        .find(|spec| spec.answers(keystroke))
        .map(|spec| spec.command.clone())
}

/// Keys that only work inside the palette, a filter field or a dialog, listed in the shortcuts
/// overlay next to the table.
pub const HINTS: [(&str, &[&str]); 4] = [
    ("Move in the palette", &["up", "down", "ctrl-p", "ctrl-n"]),
    ("Run the command", &["enter"]),
    ("Leave the filter, keep it", &["enter"]),
    ("Close, clear the filter or cancel", &["escape"]),
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

/// @name shortcut
/// @description The first key of a command as it is written on screen, for hints next to
/// buttons.
///
/// @example
/// let key = shortcut(&Command::Palette); // Some("Ctrl+K")
pub fn shortcut(command: &Command) -> Option<String> {
    table()
        .iter()
        .find(|spec| spec.command == *command)
        .and_then(|spec| spec.keys.first().map(|key| label(key)))
}

/// @name bind_keys
/// @description Binds the table's global commands as GPUI actions, the ones that work in every
/// mode, including while typing.
///
/// @example
/// app::bind_keys(cx);
pub fn bind_keys(cx: &mut App) {
    let mut bindings = Vec::new();
    for spec in table().iter().filter(|spec| spec.global) {
        for key in spec.keys.iter().chain(spec.extra) {
            bindings.push(match spec.command {
                Command::ZoomIn => KeyBinding::new(key, ZoomIn, None),
                Command::ZoomOut => KeyBinding::new(key, ZoomOut, None),
                Command::ResetZoom => KeyBinding::new(key, ResetZoom, None),
                Command::Undo => KeyBinding::new(key, Undo, None),
                Command::Palette => KeyBinding::new(key, TogglePalette, None),
                Command::Quit => KeyBinding::new(key, Quit, None),
                _ => continue,
            });
        }
    }
    cx.bind_keys(bindings);
}

/// @name matches
/// @description Whether a keystroke is the key a pattern names. Symbols such as `?` and `/`
/// compare the typed character, so they work whatever Shift does on the keyboard layout.
///
/// @example
/// assert!(matches("secondary-k", &Keystroke::parse("ctrl-k").unwrap()));
pub fn matches(pattern: &str, keystroke: &Keystroke) -> bool {
    Keystroke::parse(pattern).is_ok_and(|want| names(&want, keystroke))
}

fn names(want: &Keystroke, keystroke: &Keystroke) -> bool {
    let have = keystroke.modifiers;
    let need = want.modifiers;
    if have.control != need.control || have.alt != need.alt || have.platform != need.platform {
        return false;
    }
    let symbol = want.key.chars().count() == 1 && !want.key.chars().all(char::is_alphanumeric);
    if symbol && !need.control && !need.platform && !need.alt {
        return match keystroke.key_char.as_deref() {
            Some(typed) => typed == want.key,
            None => keystroke.key == want.key,
        };
    }
    if symbol {
        return keystroke.key == want.key;
    }
    keystroke.key == want.key && have.shift == need.shift
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
    fn patterns_match_their_keystrokes_only() {
        assert!(matches("secondary-k", &key("secondary-k")));
        assert!(!matches("secondary-k", &key("k")));
        assert!(matches("j", &key("j")));
        assert!(!matches("j", &key("shift-j")));
        assert!(matches("G", &key("shift-g")));
        assert!(!matches("G", &key("g")));
        assert!(matches("?", &key("?")));
        assert!(matches("/", &key("/")));
        assert!(!matches("/", &key("?")));
        assert!(matches("shift-f10", &key("shift-f10")));
        assert!(!matches("shift-f10", &key("f10")));
        assert!(matches("ctrl-shift-tab", &key("ctrl-shift-tab")));
        assert!(!matches("ctrl-tab", &key("ctrl-shift-tab")));
    }

    #[test]
    fn a_shifted_slash_is_a_question_mark() {
        let shifted = Keystroke {
            modifiers: gpui::Modifiers::shift(),
            key: "/".to_owned(),
            key_char: Some("?".to_owned()),
        };
        assert!(matches("?", &shifted));
        assert!(!matches("/", &shifted));
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
    fn every_key_is_used_once() {
        let specs = table();
        let mut seen: Vec<&str> = Vec::new();
        let mut map: Vec<&str> = Vec::new();
        for spec in specs {
            let seen = if spec.group == Group::Map {
                &mut map
            } else {
                &mut seen
            };
            assert!(
                !spec.keys.is_empty(),
                "{} needs a key to be listed",
                spec.label
            );
            for key in spec.keys.iter().chain(spec.extra) {
                assert!(Keystroke::parse(key).is_ok(), "{key} parses");
                assert!(!seen.contains(key), "{key} is bound twice");
                seen.push(key);
            }
        }
    }

    #[test]
    fn vim_motions_run_the_arrow_and_page_commands() {
        assert_eq!(lookup(&key("ctrl-d"), false), Some(Command::PageDown));
        assert_eq!(lookup(&key("ctrl-u"), false), Some(Command::PageUp));
        assert_eq!(
            lookup(&key("h"), true),
            Some(Command::MapMove(Direction::Left))
        );
        assert_eq!(
            lookup(&key("l"), true),
            Some(Command::MapMove(Direction::Right))
        );
        assert_eq!(lookup(&key("h"), false), None);
    }

    #[test]
    fn map_keys_win_only_inside_the_map() {
        let enter = key("enter");
        assert_eq!(lookup(&enter, true), Some(Command::MapIn));
        assert_eq!(lookup(&enter, false), Some(Command::Trash));
        assert_eq!(
            lookup(&key("left"), true),
            Some(Command::MapMove(Direction::Left))
        );
        assert_eq!(lookup(&key("left"), false), None);
        assert_eq!(lookup(&key("backspace"), false), None);
        assert_eq!(lookup(&key("j"), true), Some(Command::Down));
        assert_eq!(lookup(&key("m"), false), Some(Command::SpaceMap));
        assert_eq!(label("backspace"), "Backspace");
    }
}
