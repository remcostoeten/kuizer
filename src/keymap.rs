use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

pub type Overrides = BTreeMap<String, Override>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Override {
    One(String),
    Many(Vec<String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scope {
    Gui,
    GuiBack,
    Map,
    Control,
    StartControl,
    Folder,
    Palette,
    GuiFilter,
    Settings,
    PathEditor,
    Field,
    Menu,
    Dialog,
    Shortcuts,
    Tui,
    TuiFilter,
    TuiConfirm,
    TuiQuit,
}

impl Scope {
    fn terminal(self) -> bool {
        matches!(
            self,
            Self::Tui | Self::TuiFilter | Self::TuiConfirm | Self::TuiQuit
        )
    }

    fn intercepts(self, action: Action, modifiers: Modifiers, other: Scope) -> bool {
        match self {
            Self::Gui => {
                action.global_gui() && !other.terminal()
                    || action == Action::Settings && matches!(other, Self::Settings | Self::Field)
            }
            Self::GuiBack => matches!(other, Self::Gui | Self::Map),
            Self::Control => {
                matches!(action, Action::ControlNext | Action::ControlPrevious)
                    && matches!(
                        other,
                        Self::Gui
                            | Self::GuiBack
                            | Self::Map
                            | Self::GuiFilter
                            | Self::Field
                            | Self::Folder
                    )
            }
            Self::Field => matches!(
                other,
                Self::GuiFilter | Self::Palette | Self::PathEditor | Self::Folder
            ),
            Self::Tui => {
                action == Action::Quit
                    && modifiers.modified()
                    && matches!(other, Self::TuiFilter | Self::TuiConfirm)
            }
            _ => false,
        }
    }

    fn exclusive_menu_items(self, left: Action, right: Action) -> bool {
        use Action::{MenuCopyCommand, MenuRestore};
        self == Self::Menu
            && ([left, right].contains(&MenuCopyCommand) || [left, right].contains(&MenuRestore))
            && ![left, right]
                .iter()
                .all(|action| matches!(action, MenuCopyCommand | MenuRestore))
            && [left, right].iter().all(|action| action.is_menu_item())
    }
}

macro_rules! actions {
    ($($action:ident => $name:literal, $($scope:ident: [$($key:literal),+]),+;)+) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        pub enum Action { $($action),+ }

        impl Action {
            /// @name Action::name
            /// @description Returns the stable name used in the config file.
            /// @example
            /// assert_eq!(kuizer::keymap::Action::Down.name(), "nav.down");
            pub const fn name(self) -> &'static str {
                match self { $(Self::$action => $name),+ }
            }

            fn named(name: &str) -> Option<Self> {
                match name { $($name => Some(Self::$action)),+, _ => None }
            }
        }

        const DEFAULTS: &[(Action, Scope, &[&str])] = &[
            $($((Action::$action, Scope::$scope, &[$($key),+])),+),+
        ];
    }
}
actions! {
    Palette => "palette.open", Gui: ["secondary-k", "secondary-shift-p"];
    Quit => "app.quit", Gui: ["secondary-q"], Tui: ["q", "ctrl-q", "ctrl-c"];
    Help => "help.open", Gui: ["?"], Tui: ["?"];
    Settings => "settings.open", Gui: ["secondary-,"];
    Rescan => "scan.refresh", Gui: ["secondary-r"];
    NewScan => "scan.new", Gui: ["secondary-n"];
    StopScan => "scan.stop", Gui: ["s"];
    FindDuplicates => "duplicates.find", Gui: ["D"];
    StopDuplicates => "duplicates.stop", Gui: ["escape"];
    Overview => "view.overview", Gui: ["shift-1"];
    Artifacts => "view.artifacts", Gui: ["shift-2"], Tui: ["1"];
    Caches => "view.caches", Gui: ["shift-3"], Tui: ["2"];
    Toolchains => "view.toolchains", Gui: ["shift-4"];
    Docker => "view.docker", Gui: ["shift-5"];
    Repos => "view.repos", Gui: ["shift-6"];
    Duplicates => "view.duplicates", Gui: ["shift-7"], Tui: ["3"];
    History => "view.history", Gui: ["shift-8"];
    Issues => "view.issues", Gui: ["shift-9"], Tui: ["4"];
    SpaceMap => "map.toggle", Gui: ["m"];
    NextTab => "view.next", Gui: ["ctrl-tab"];
    PreviousTab => "view.previous", Gui: ["ctrl-shift-tab"];
    Down => "nav.down", Gui: ["j", "down"], Tui: ["j", "down"];
    Up => "nav.up", Gui: ["k", "up"], Tui: ["k", "up"];
    ExtendDown => "select.extend_down", Gui: ["J", "shift-down"], Tui: ["J", "shift-down"];
    ExtendUp => "select.extend_up", Gui: ["K", "shift-up"], Tui: ["K", "shift-up"];
    Visual => "select.visual", Gui: ["v"];
    First => "nav.top", Gui: ["g", "home"], Tui: ["g", "home"];
    Last => "nav.bottom", Gui: ["G", "end"], Tui: ["G", "end"];
    PageDown => "nav.page_down", Gui: ["pagedown", "ctrl-d"], Tui: ["pagedown"];
    PageUp => "nav.page_up", Gui: ["pageup", "ctrl-u"], Tui: ["pageup"];
    Toggle => "select.toggle", Gui: ["space"], Tui: ["space"];
    SelectAll => "select.safe", Gui: ["secondary-a"], Tui: ["a"];
    SelectNone => "select.none", Gui: ["secondary-shift-a"], Tui: ["u"];
    SelectWins => "select.wins", Gui: ["w"];
    Delete => "delete.selected", Gui: ["enter", "delete", "d"], Tui: ["enter"];
    Reveal => "file.reveal", Gui: ["o"];
    CopyPath => "file.copy_path", Gui: ["y"];
    OpenMenu => "menu.open", Gui: ["menu", "shift-f10"];
    FilterFocus => "filter.focus", Gui: ["/"], Tui: ["/"];
    Gc => "git.gc", Gui: ["c"];
    GcAll => "git.gc_all", Gui: ["C"];
    Branches => "git.branches", Gui: ["b"];
    BranchesAll => "git.branches_all", Gui: ["B"];
    ZoomIn => "zoom.in", Gui: ["secondary-=", "secondary-+", "secondary-shift-="];
    ZoomOut => "zoom.out", Gui: ["secondary--"];
    ResetZoom => "zoom.reset", Gui: ["secondary-0"];
    Sidebar => "sidebar.toggle", Gui: ["secondary-b"];
    Restore => "history.restore", Gui: ["r"];
    Exclude => "file.exclude", Gui: ["x"];
    CopyCommand => "history.copy_command", Gui: ["shift-y"];
    Back => "nav.back", GuiBack: ["escape"], Control: ["escape"], Palette: ["escape"], GuiFilter: ["escape"], Settings: ["escape"], PathEditor: ["escape"], Dialog: ["escape"], Tui: ["escape"], TuiFilter: ["escape"];
    MapIn => "map.open", Map: ["enter"];
    MapOut => "map.back", Map: ["backspace", "escape"];
    MapTop => "map.top", Map: ["home"];
    MapToggle => "map.select", Map: ["space"];
    MapLeft => "map.left", Map: ["left", "h"];
    MapRight => "map.right", Map: ["right", "l"];
    MapUp => "map.up", Map: ["up", "k"];
    MapDown => "map.down", Map: ["down", "j"];
    ControlNext => "control.next", Control: ["tab"];
    ControlPrevious => "control.previous", Control: ["shift-tab"];
    ControlForward => "control.forward", Control: ["right", "down", "l", "j"], StartControl: ["right", "down"];
    ControlBack => "control.backward", Control: ["left", "up", "h", "k"], StartControl: ["left", "up"];
    ControlPress => "control.press", Control: ["enter", "space"];
    StartScan => "scan.start", Folder: ["enter"];
    PaletteRun => "palette.run", Palette: ["enter"];
    PaletteNext => "palette.next", Palette: ["down", "tab", "ctrl-n"];
    PalettePrevious => "palette.previous", Palette: ["up", "shift-tab", "ctrl-p"];
    ApplyFilter => "filter.apply", GuiFilter: ["enter"], TuiFilter: ["enter"];
    FilterDown => "filter.down", GuiFilter: ["down"];
    FilterUp => "filter.up", GuiFilter: ["up"];
    SettingsNext => "settings.next_row", Settings: ["down", "j"];
    SettingsPrevious => "settings.previous_row", Settings: ["up", "k"];
    SettingsFirst => "settings.first_row", Settings: ["g", "home"];
    SettingsLast => "settings.last_row", Settings: ["G", "end"];
    SettingsRight => "settings.right", Settings: ["right", "l"];
    SettingsLeft => "settings.left", Settings: ["left", "h"];
    SettingsNextTab => "settings.next_page", Settings: ["tab"];
    SettingsPreviousTab => "settings.previous_page", Settings: ["shift-tab"];
    SettingsChange => "settings.change", Settings: ["space", "enter"];
    MenuDown => "menu.next", Menu: ["down", "j", "tab"];
    MenuUp => "menu.previous", Menu: ["up", "k", "shift-tab"];
    MenuFirst => "menu.first", Menu: ["home"];
    MenuLast => "menu.last", Menu: ["end"];
    MenuRun => "menu.run", Menu: ["enter", "space"];
    MenuClose => "menu.close", Menu: ["escape", "menu", "shift-f10"];
    MenuReveal => "menu.reveal", Menu: ["r", "R"];
    MenuCopyPath => "menu.copy_path", Menu: ["c", "C"];
    MenuExclude => "menu.exclude", Menu: ["x", "X"];
    MenuToggle => "menu.select", Menu: ["s", "S"];
    MenuDelete => "menu.delete", Menu: ["delete"];
    MenuRestore => "menu.restore", Menu: ["r", "R"];
    MenuCopyCommand => "menu.copy_command", Menu: ["c", "C"];
    MenuGc => "menu.git_gc", Menu: ["g", "G"];
    MenuBranches => "menu.git_branches", Menu: ["b", "B"];
    DialogToggle => "confirm.focus_next", Dialog: ["tab", "shift-tab", "left", "right", "up", "down", "h", "j", "k", "l"];
    Confirm => "confirm.accept", Dialog: ["enter"], TuiConfirm: ["y", "enter"];
    HalfPageDown => "nav.half_page_down", Tui: ["ctrl-d"];
    HalfPageUp => "nav.half_page_up", Tui: ["ctrl-u"];
    DeleteCursor => "delete.cursor", Tui: ["d", "delete"];
    CycleSort => "sort.cycle", Tui: ["s"];
    Group => "group.toggle", Tui: ["p"];
    SelectProject => "select.project", Tui: ["P"];
    Category => "filter.category", Tui: ["c"];
    SafeOnly => "filter.safe_only", Tui: ["f"];
    ResetFilters => "filter.reset", Tui: ["x"];
    Cancel => "confirm.cancel", TuiConfirm: ["n", "escape"];
    QuitAccept => "quit.accept", TuiQuit: ["y", "enter"];
    QuitCancel => "quit.cancel", TuiQuit: ["n", "escape"];
    FieldBackspace => "field.backspace", Field: ["backspace"], TuiFilter: ["backspace"];
    FieldDeleteWord => "field.delete_word", Field: ["ctrl-backspace", "cmd-backspace", "alt-backspace"];
    FieldClear => "field.clear", Field: ["ctrl-u", "cmd-u"];
    FieldPaste => "field.paste", Field: ["ctrl-v", "cmd-v"];
    SaveProtectedPath => "settings.save_path", PathEditor: ["enter"];
    ShortcutsClose => "help.close", Shortcuts: ["escape", "enter", "?"];
}

impl Action {
    fn is_menu_item(self) -> bool {
        matches!(
            self,
            Self::MenuReveal
                | Self::MenuCopyPath
                | Self::MenuExclude
                | Self::MenuToggle
                | Self::MenuDelete
                | Self::MenuRestore
                | Self::MenuCopyCommand
                | Self::MenuGc
                | Self::MenuBranches
        )
    }

    fn global_gui(self) -> bool {
        matches!(
            self,
            Self::Quit | Self::Palette | Self::ZoomIn | Self::ZoomOut | Self::ResetZoom
        )
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub cmd: bool,
    pub function: bool,
}

impl Modifiers {
    /// @name Modifiers::modified
    /// @description Returns whether a modifier besides Shift is held.
    /// @example
    /// assert!(!kuizer::keymap::Modifiers::default().modified());
    pub fn modified(self) -> bool {
        self.ctrl || self.alt || self.cmd || self.function
    }
}

#[derive(Clone, Copy)]
pub struct Stroke<'a> {
    pub key: &'a str,
    pub typed: Option<&'a str>,
    pub modifiers: Modifiers,
}

#[derive(Debug, Clone)]
struct Binding {
    text: String,
    key: String,
    modifiers: Modifiers,
}

fn function_key(key: &str) -> bool {
    key.strip_prefix('f')
        .filter(|number| !number.starts_with('0'))
        .and_then(|number| number.parse::<u8>().ok())
        .is_some_and(|number| (1..=24).contains(&number))
}

fn valid_key(key: &str) -> bool {
    (key.len() == 1 && key.as_bytes()[0].is_ascii_graphic())
        || matches!(
            key,
            "space"
                | "enter"
                | "escape"
                | "tab"
                | "backspace"
                | "delete"
                | "insert"
                | "up"
                | "down"
                | "left"
                | "right"
                | "home"
                | "end"
                | "pageup"
                | "pagedown"
                | "menu"
        )
        || function_key(key)
}

fn shifted_digit(key: &str) -> Option<&'static str> {
    match key {
        "1" => Some("!"),
        "2" => Some("@"),
        "3" => Some("#"),
        "4" => Some("$"),
        "5" => Some("%"),
        "6" => Some("^"),
        "7" => Some("&"),
        "8" => Some("*"),
        "9" => Some("("),
        "0" => Some(")"),
        _ => None,
    }
}

impl Binding {
    fn parse(text: &str, scope: Scope) -> Option<Self> {
        let mut modifiers = Modifiers::default();
        let mut rest = text;
        while rest != "-" {
            let Some((modifier, key)) = rest.split_once('-') else {
                break;
            };
            let flag = match modifier {
                "ctrl" => &mut modifiers.ctrl,
                "alt" => &mut modifiers.alt,
                "shift" => &mut modifiers.shift,
                "cmd" => &mut modifiers.cmd,
                "fn" => &mut modifiers.function,
                "secondary" if cfg!(target_os = "macos") && !scope.terminal() => &mut modifiers.cmd,
                "secondary" => &mut modifiers.ctrl,
                _ => return None,
            };
            if *flag {
                return None;
            }
            *flag = true;
            rest = key;
        }
        let key = rest.to_ascii_lowercase();
        if !valid_key(&key) || (scope.terminal() && (key == "menu" || modifiers.function)) {
            return None;
        }
        if rest.len() == 1 && rest.as_bytes()[0].is_ascii_uppercase() {
            modifiers.shift = true;
        }
        Some(Self {
            text: text.to_owned(),
            key,
            modifiers,
        })
    }

    fn answers(&self, stroke: Stroke<'_>) -> bool {
        let need = self.modifiers;
        let have = stroke.modifiers;
        if have.ctrl != need.ctrl
            || have.alt != need.alt
            || have.cmd != need.cmd
            || have.function != need.function
        {
            return false;
        }
        if need.shift
            && !need.modified()
            && let Some(symbol) = shifted_digit(&self.key)
        {
            return (stroke.key == self.key && have.shift)
                || stroke.typed.unwrap_or(stroke.key) == symbol;
        }
        let symbol = self.key.len() == 1 && !self.key.as_bytes()[0].is_ascii_alphanumeric();
        if symbol && !need.modified() && !need.shift {
            return stroke.typed.unwrap_or(stroke.key) == self.key;
        }
        if symbol && need.modified() && !need.shift {
            return stroke.key == self.key;
        }
        stroke.key == self.key && have.shift == need.shift
    }

    fn conflicts(&self, other: &Self) -> bool {
        let stroke = |key: &Self| {
            self.answers(Stroke {
                key: &key.key,
                typed: None,
                modifiers: key.modifiers,
            })
        };
        stroke(other)
            || other.answers(Stroke {
                key: &self.key,
                typed: None,
                modifiers: self.modifiers,
            })
    }
}

#[derive(Debug, thiserror::Error)]
#[error("keys.{name}: {message}")]
pub struct KeyError {
    pub name: String,
    pub message: String,
}

fn actions_conflict(left: (Scope, Action, &Binding), right: (Scope, Action, &Binding)) -> bool {
    let (scope, action, binding) = left;
    let (other_scope, other, other_binding) = right;
    if action == other || scope == other_scope && scope.exclusive_menu_items(action, other) {
        return false;
    }
    if matches!(
        (action, other),
        (Action::Back, Action::MapOut | Action::StopDuplicates)
            | (Action::MapOut | Action::StopDuplicates, Action::Back)
    ) && binding.key == "escape"
        && other_binding.key == "escape"
    {
        return false;
    }
    (scope == other_scope
        || scope.intercepts(action, binding.modifiers, other_scope)
        || other_scope.intercepts(other, other_binding.modifiers, scope))
        && binding.conflicts(other_binding)
}

type Bindings = BTreeMap<(Scope, Action), Vec<Binding>>;

#[derive(Debug, Clone)]
pub struct Keymap {
    bindings: Bindings,
}

static DEFAULT_KEYMAP: LazyLock<Keymap> = LazyLock::new(|| Keymap {
    bindings: DEFAULTS
        .iter()
        .map(|&(action, scope, keys)| {
            (
                (scope, action),
                keys.iter()
                    .map(|key| Binding::parse(key, scope).expect("valid default shortcut"))
                    .collect(),
            )
        })
        .collect(),
});

impl Default for Keymap {
    fn default() -> Self {
        DEFAULT_KEYMAP.clone()
    }
}

fn parse_override(action: Action, keys: &[String], scope: Scope) -> Result<Vec<Binding>, String> {
    if keys.is_empty() {
        return Err("provide at least one binding".to_owned());
    }
    keys.iter()
        .map(|key| {
            let parsed =
                Binding::parse(key, scope).ok_or_else(|| format!("invalid binding `{key}`"))?;
            if scope == Scope::Gui
                && action.global_gui()
                && !parsed.modifiers.modified()
                && !function_key(&parsed.key)
            {
                return Err(format!("`{key}` needs a modifier to work while typing"));
            }
            if scope == Scope::Gui
                && parsed.key.len() == 1
                && parsed.key.as_bytes()[0].is_ascii_digit()
                && parsed.key != "0"
                && !parsed.modifiers.modified()
                && !parsed.modifiers.shift
            {
                return Err(format!("`{key}` is reserved for motion counts"));
            }
            Ok(parsed)
        })
        .collect()
}

impl Keymap {
    /// @name Keymap::resolve
    /// @description Resolves overrides for the GUI and TUI, returning rejected keys alongside
    /// the usable keymap. Rejected actions keep their defaults.
    /// @example
    /// use kuizer::config::Config;
    /// let (keys, warnings) = kuizer::keymap::Keymap::resolve(&Config::default().keys);
    /// assert!(warnings.is_empty());
    pub fn resolve(overrides: &Overrides) -> (Self, Vec<KeyError>) {
        let mut warnings = Vec::new();
        let mut accepted = BTreeMap::new();
        for (name, config) in overrides {
            let Some(action) = Action::named(name) else {
                warnings.push(KeyError {
                    name: name.clone(),
                    message: "unknown action".to_owned(),
                });
                continue;
            };
            let keys = match config {
                Override::One(key) => std::slice::from_ref(key),
                Override::Many(keys) => keys.as_slice(),
            };
            let bindings: Result<Vec<_>, _> = DEFAULTS
                .iter()
                .filter(|(candidate, _, _)| *candidate == action)
                .map(|&(_, scope, _)| {
                    parse_override(action, keys, scope).map(|keys| ((scope, action), keys))
                })
                .collect();
            match bindings {
                Ok(bindings) => {
                    accepted.insert(action, bindings);
                }
                Err(message) => warnings.push(KeyError {
                    name: name.clone(),
                    message,
                }),
            }
        }
        loop {
            let mut map = Self::default();
            for bindings in accepted.values() {
                map.bindings.extend(bindings.iter().cloned());
            }
            let mut rejected = BTreeSet::new();
            for (&action, bindings) in &accepted {
                for ((scope, _), keys) in bindings {
                    let conflict =
                        map.bindings
                            .iter()
                            .find(|&(&(other_scope, other), other_keys)| {
                                keys.iter().any(|key| {
                                    other_keys.iter().any(|other_key| {
                                        actions_conflict(
                                            (*scope, action, key),
                                            (other_scope, other, other_key),
                                        )
                                    })
                                })
                            });
                    if let Some(((_, other), _)) = conflict {
                        rejected.insert((action, *other));
                        break;
                    }
                }
            }
            if rejected.is_empty() {
                return (map, warnings);
            }
            for (action, other) in rejected {
                accepted.remove(&action);
                warnings.push(KeyError {
                    name: action.name().to_owned(),
                    message: format!("binding conflicts with `{}`", other.name()),
                });
            }
        }
    }

    /// @name Keymap::keys
    /// @description Lists the effective key labels for an action in its active scope.
    /// @example
    /// use kuizer::keymap::{Action, Keymap, Scope};
    /// assert_eq!(Keymap::default().keys(Scope::Tui, Action::Down).next(), Some("j"));
    pub fn keys(&self, scope: Scope, action: Action) -> impl Iterator<Item = &str> {
        self.bindings
            .get(&(scope, action))
            .into_iter()
            .flatten()
            .map(|binding| binding.text.as_str())
    }

    /// @name Keymap::matches
    /// @description Tests a stroke against one action in its active scope.
    /// @example
    /// use kuizer::keymap::{Action, Keymap, Modifiers, Scope, Stroke};
    /// let stroke = Stroke { key: "down", typed: None, modifiers: Modifiers::default() };
    /// assert!(Keymap::default().matches(Scope::Tui, Action::Down, stroke));
    pub fn matches(&self, scope: Scope, action: Action, stroke: Stroke<'_>) -> bool {
        self.bindings
            .get(&(scope, action))
            .is_some_and(|bindings| bindings.iter().any(|binding| binding.answers(stroke)))
    }

    /// @name Keymap::lookup
    /// @description Finds the action associated with a stroke in one active scope.
    /// @example
    /// use kuizer::keymap::{Action, Keymap, Modifiers, Scope, Stroke};
    /// let stroke = Stroke { key: "j", typed: None, modifiers: Modifiers::default() };
    /// assert_eq!(Keymap::default().lookup(Scope::Tui, stroke), Some(Action::Down));
    pub fn lookup(&self, scope: Scope, stroke: Stroke<'_>) -> Option<Action> {
        self.bindings
            .range((scope, Action::Palette)..=(scope, Action::ShortcutsClose))
            .find(|(_, bindings)| bindings.iter().any(|binding| binding.answers(stroke)))
            .map(|(&(_, action), _)| action)
    }
}
