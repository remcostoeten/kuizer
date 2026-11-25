//! The settings popover: pages of switches that read and write the config file.

use std::sync::LazyLock;

use gpui::{Keystroke, canvas};
use kuizer::config::{
    self, Accent, Config, DEFAULT_ZOOM, DeleteMode, Density, EditError, MAX_ZOOM, MIN_ZOOM,
    SettingValue, Sidebar, Theme,
};
use kuizer::keymap::{self, KeyError, Override, Overrides};

use super::*;
use crate::zoom;

static DEFAULT_KEYS: LazyLock<Keymap> = LazyLock::new(Keymap::default);

#[derive(Default)]
pub(super) struct SettingWrites {
    saved: Vec<(Setting, usize)>,
    queued: Vec<(Setting, usize)>,
    writing: bool,
    field: Option<Field>,
    keys: Option<Key>,
}

impl SettingWrites {
    fn queue(&mut self, setting: Setting, was: usize, value: usize) {
        if !self.saved.iter().any(|(known, _)| *known == setting) {
            self.saved.push((setting, was));
        }
        self.queued.retain(|(known, _)| *known != setting);
        self.queued.push((setting, value));
    }

    fn next(&mut self) -> Option<(Setting, usize)> {
        if self.writing || self.queued.is_empty() {
            return None;
        }
        self.writing = true;
        Some(self.queued.remove(0))
    }

    fn finished(&mut self, setting: Setting, value: usize, written: bool) -> Option<usize> {
        self.writing = false;
        let at = self.saved.iter().position(|(known, _)| *known == setting)?;
        if written {
            self.saved[at].1 = value;
        }
        if self.queued.iter().any(|(known, _)| *known == setting) {
            return None;
        }
        let (_, saved) = self.saved.remove(at);
        (!written).then_some(saved)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Setting {
    Theme,
    Accent,
    Density,
    Motion,
    SidebarCollapsed,
    Zoom,
    UpdateCheck,
    RoutesEnabled,
    RouteHints,
    Trash,
    CrossDevice,
    DuplicatesAuto,
    DuplicatesVerify,
    DuplicatesHidden,
    GitAggressive,
    ToolchainPins,
}

const ZOOM_STEPS: usize = ((MAX_ZOOM - MIN_ZOOM) / zoom::STEP) as usize + 1;

fn zoom_step(percent: u16) -> usize {
    usize::from((zoom::snap(percent) - MIN_ZOOM) / zoom::STEP)
}

impl Setting {
    fn label(self) -> &'static str {
        match self {
            Self::Theme => "Theme",
            Self::Accent => "Accent",
            Self::Density => "Density",
            Self::Motion => "Motion",
            Self::SidebarCollapsed => "Open the sidebar collapsed",
            Self::Zoom => "Interface size",
            Self::UpdateCheck => "Check for updates",
            Self::RoutesEnabled => "Section shortcuts",
            Self::RouteHints => "Show shortcut hints",
            Self::Trash => "Move to the trash",
            Self::CrossDevice => "Allow crossing filesystem boundaries",
            Self::DuplicatesAuto => "Search after every scan",
            Self::DuplicatesVerify => "Compare byte by byte",
            Self::DuplicatesHidden => "Include hidden folders",
            Self::GitAggressive => "Aggressive garbage collection",
            Self::ToolchainPins => "Look for pinned versions",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::Theme => "Dark, or light on an off-white background.",
            Self::Accent => "The color of the selection, the focus outline and the bars.",
            Self::Density => "Compact uses shorter list rows and less vertical padding.",
            Self::Motion => "Animate selection, expanding rows and items leaving after a cleanup.",
            Self::SidebarCollapsed => {
                "Start as a strip of icons. The sidebar shortcut switches it while the app runs."
            }
            Self::Zoom => {
                "Left and right change it in steps of ten, Enter goes back to 100%. The zoom shortcuts work anywhere in the app."
            }
            Self::UpdateCheck => {
                "Check GitHub releases on launch and show a notice when a newer version exists. Off by default; never installs anything automatically."
            }
            Self::RoutesEnabled => "Use keyboard shortcuts to open sidebar sections.",
            Self::RouteHints => "Key badges in the sidebar and in the section tooltips.",
            Self::Trash => {
                "Project artifacts and caches go to the trash and History can restore them. Off deletes them for good. Toolchains, Docker objects and git branches are always deleted for good."
            }
            Self::CrossDevice => {
                "Allow deletion from a different filesystem, including mounted folders."
            }
            Self::DuplicatesAuto => {
                "Look for duplicate files once the rest of the results are on screen. Off searches only when you ask."
            }
            Self::DuplicatesVerify => {
                "Read every match in full before it is listed. Slower, and rules out hash collisions."
            }
            Self::DuplicatesHidden => {
                "Also search hidden home folders such as ~/.cache and folders tools manage, such as Go modules."
            }
            Self::GitAggressive => {
                "Run git gc with --aggressive. It frees more space and takes much longer."
            }
            Self::ToolchainPins => {
                "Find versions a project pins with files such as .nvmrc or rust-toolchain, and mark them review."
            }
        }
    }

    fn choices(self) -> Option<Vec<&'static str>> {
        match self {
            Self::Theme => Some(Theme::ALL.into_iter().map(Theme::label).collect()),
            Self::Accent => Some(Accent::ALL.into_iter().map(Accent::label).collect()),
            Self::Density => Some(Density::ALL.into_iter().map(Density::label).collect()),
            _ => None,
        }
    }

    fn stepped(self) -> bool {
        matches!(
            self,
            Self::Theme | Self::Accent | Self::Density | Self::Zoom
        )
    }

    fn value(self, config: &Config) -> usize {
        match self {
            Self::Theme => Theme::ALL
                .iter()
                .position(|theme| *theme == config.appearance.theme)
                .unwrap_or(0),
            Self::Accent => Accent::ALL
                .iter()
                .position(|accent| *accent == config.appearance.accent)
                .unwrap_or(0),
            Self::Density => Density::ALL
                .iter()
                .position(|density| *density == config.appearance.density)
                .unwrap_or(0),
            Self::Motion => usize::from(config.appearance.motion),
            Self::SidebarCollapsed => usize::from(config.appearance.sidebar == Sidebar::Collapsed),
            Self::Zoom => zoom_step(config.appearance.zoom),
            Self::UpdateCheck => usize::from(config.appearance.update_check),
            Self::RoutesEnabled => usize::from(config.route_shortcuts.enabled),
            Self::RouteHints => usize::from(config.route_shortcuts.hints),
            Self::Trash => usize::from(config.delete.mode == DeleteMode::Trash),
            Self::CrossDevice => usize::from(config.delete.cross_device),
            Self::DuplicatesAuto => usize::from(config.duplicates.enabled),
            Self::DuplicatesVerify => usize::from(config.duplicates.verify),
            Self::DuplicatesHidden => usize::from(config.duplicates.include_hidden),
            Self::GitAggressive => usize::from(config.git.aggressive),
            Self::ToolchainPins => usize::from(config.toolchains.pins),
        }
    }

    fn count(self) -> usize {
        match self {
            Self::Theme => Theme::ALL.len(),
            Self::Accent => Accent::ALL.len(),
            Self::Density => Density::ALL.len(),
            Self::Zoom => ZOOM_STEPS,
            _ => 2,
        }
    }

    fn next(self, config: &Config) -> usize {
        match self {
            Self::Zoom => zoom_step(DEFAULT_ZOOM),
            _ => (self.value(config) + 1) % self.count(),
        }
    }

    fn step(self, config: &Config, delta: isize) -> usize {
        self.value(config)
            .saturating_add_signed(delta)
            .min(self.count() - 1)
    }

    fn set(self, config: &mut Config, value: usize) {
        let on = value == 1;
        match self {
            Self::Theme => {
                config.appearance.theme = Theme::ALL[value.min(Theme::ALL.len() - 1)];
                theme::configure(config.appearance.theme, config.appearance.accent);
            }
            Self::Accent => {
                config.appearance.accent = Accent::ALL[value.min(Accent::ALL.len() - 1)];
                theme::configure(config.appearance.theme, config.appearance.accent);
            }
            Self::Density => {
                config.appearance.density = Density::ALL[value.min(Density::ALL.len() - 1)];
                theme::set_density(config.appearance.density);
            }
            Self::Motion => {
                config.appearance.motion = on;
                motion::configure(on);
            }
            Self::SidebarCollapsed => {
                config.appearance.sidebar = if on {
                    Sidebar::Collapsed
                } else {
                    Sidebar::Expanded
                };
            }
            Self::Zoom => config.appearance.zoom = zoom_percent(value),
            Self::UpdateCheck => config.appearance.update_check = on,
            Self::RoutesEnabled => config.route_shortcuts.enabled = on,
            Self::RouteHints => config.route_shortcuts.hints = on,
            Self::Trash => {
                config.delete.mode = if on {
                    DeleteMode::Trash
                } else {
                    DeleteMode::Permanent
                };
            }
            Self::CrossDevice => config.delete.cross_device = on,
            Self::DuplicatesAuto => config.duplicates.enabled = on,
            Self::DuplicatesVerify => config.duplicates.verify = on,
            Self::DuplicatesHidden => config.duplicates.include_hidden = on,
            Self::GitAggressive => config.git.aggressive = on,
            Self::ToolchainPins => config.toolchains.pins = on,
        }
    }

    fn entry(self, value: usize) -> (&'static str, &'static str, SettingValue) {
        let on = value == 1;
        let text = |text: &str| SettingValue::Text(text.to_owned());
        match self {
            Self::Theme => (
                "appearance",
                "theme",
                text(Theme::ALL[value.min(Theme::ALL.len() - 1)].key()),
            ),
            Self::Accent => (
                "appearance",
                "accent",
                text(Accent::ALL[value.min(Accent::ALL.len() - 1)].key()),
            ),
            Self::Density => (
                "appearance",
                "density",
                text(Density::ALL[value.min(Density::ALL.len() - 1)].key()),
            ),
            Self::Motion => ("appearance", "motion", SettingValue::Bool(on)),
            Self::SidebarCollapsed => (
                "appearance",
                "sidebar",
                text(if on {
                    Sidebar::Collapsed.key()
                } else {
                    Sidebar::Expanded.key()
                }),
            ),
            Self::Zoom => (
                "appearance",
                "zoom",
                SettingValue::Integer(i64::from(zoom_percent(value))),
            ),
            Self::UpdateCheck => ("appearance", "update_check", SettingValue::Bool(on)),
            Self::RoutesEnabled => ("route_shortcuts", "enabled", SettingValue::Bool(on)),
            Self::RouteHints => ("route_shortcuts", "hints", SettingValue::Bool(on)),
            Self::Trash => (
                "delete",
                "mode",
                text(if on {
                    DeleteMode::Trash.key()
                } else {
                    DeleteMode::Permanent.key()
                }),
            ),
            Self::CrossDevice => ("delete", "cross_device", SettingValue::Bool(on)),
            Self::DuplicatesAuto => ("duplicates", "enabled", SettingValue::Bool(on)),
            Self::DuplicatesVerify => ("duplicates", "verify", SettingValue::Bool(on)),
            Self::DuplicatesHidden => ("duplicates", "include_hidden", SettingValue::Bool(on)),
            Self::GitAggressive => ("git", "aggressive", SettingValue::Bool(on)),
            Self::ToolchainPins => ("toolchains", "pins", SettingValue::Bool(on)),
        }
    }
}

fn zoom_percent(step: usize) -> u16 {
    let step = u16::try_from(step.min(ZOOM_STEPS - 1)).unwrap_or_default();
    MIN_ZOOM + step * zoom::STEP
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    List,
    Size,
    Number,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Field {
    Roots,
    Exclude,
    MinSize,
    SizeThreads,
    DuplicateMinSize,
    DuplicateThreads,
    MinGain,
    KeepBranches,
    GitThreads,
    Tools,
    Engines,
    DockerTimeout,
    RecentHours,
    Protected,
}

enum Write {
    Setting {
        table: &'static str,
        key: &'static str,
        value: SettingValue,
        parsed: Box<Config>,
    },
    Protected(Vec<PathBuf>),
}

impl Write {
    fn run(self, file: &Path) -> Result<Box<Config>, String> {
        match self {
            Self::Setting {
                table,
                key,
                value,
                parsed,
            } => config::set_setting(file, table, key, value)
                .map(|()| parsed)
                .map_err(|error| error.to_string()),
            Self::Protected(paths) => config::set_protected_paths(file, &paths)
                .map(|()| {
                    let mut parsed = Box::<Config>::default();
                    parsed.delete.protected = paths;
                    parsed
                })
                .map_err(|error| error.to_string()),
        }
    }
}

impl Field {
    fn label(self) -> &'static str {
        match self {
            Self::Roots => "Scan roots",
            Self::Exclude => "Excluded paths",
            Self::MinSize => "Smallest item",
            Self::SizeThreads => "Sizing threads",
            Self::DuplicateMinSize => "Smallest file",
            Self::DuplicateThreads => "Hashing threads",
            Self::MinGain => "Smallest gain",
            Self::KeepBranches => "Kept branches",
            Self::GitThreads => "Repository threads",
            Self::Tools => "Version managers",
            Self::Engines => "Engines",
            Self::DockerTimeout => "Timeout in seconds",
            Self::RecentHours => "Recent within hours",
            Self::Protected => "Protected paths",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::Roots => {
                "Folders a scan covers when none is given. The start screen opens on the first one."
            }
            Self::Exclude => "Glob patterns no scan enters, such as vendor or ~/archive/**.",
            Self::MinSize => "Items smaller than this are left out of the results.",
            Self::SizeThreads => {
                "Threads that measure folder sizes. 0 picks a number from the core count."
            }
            Self::DuplicateMinSize => "Files smaller than this are never compared.",
            Self::DuplicateThreads => {
                "Threads that hash files. 0 picks a number from the core count."
            }
            Self::MinGain => "List a repository when git gc would free at least this much.",
            Self::KeepBranches => "Branches that are never offered for deletion, even when merged.",
            Self::GitThreads => {
                "Repositories checked at the same time. 0 picks a number from the core count."
            }
            Self::Tools => {
                "Toolchain managers to look at: rustup, nvm, fnm, volta, pyenv, uv, sdkman and go."
            }
            Self::Engines => "Container engines asked for images, containers and volumes.",
            Self::DockerTimeout => "How long to wait for an engine to answer.",
            Self::RecentHours => {
                "Containers that stopped less than this many hours ago are marked recent."
            }
            Self::Protected => {
                "Folders deletion always refuses, on top of the system folders it never touches."
            }
        }
    }

    fn kind(self) -> Kind {
        match self {
            Self::Roots
            | Self::Exclude
            | Self::KeepBranches
            | Self::Tools
            | Self::Engines
            | Self::Protected => Kind::List,
            Self::MinSize | Self::DuplicateMinSize | Self::MinGain => Kind::Size,
            Self::SizeThreads
            | Self::DuplicateThreads
            | Self::GitThreads
            | Self::DockerTimeout
            | Self::RecentHours => Kind::Number,
        }
    }

    fn key(self) -> (&'static str, &'static str) {
        match self {
            Self::Roots => ("scan", "roots"),
            Self::Exclude => ("scan", "exclude"),
            Self::MinSize => ("scan", "min_size"),
            Self::SizeThreads => ("scan", "size_threads"),
            Self::DuplicateMinSize => ("duplicates", "min_size"),
            Self::DuplicateThreads => ("duplicates", "threads"),
            Self::MinGain => ("git", "min_gain"),
            Self::KeepBranches => ("git", "keep_branches"),
            Self::GitThreads => ("git", "threads"),
            Self::Tools => ("toolchains", "tools"),
            Self::Engines => ("docker", "engines"),
            Self::DockerTimeout => ("docker", "timeout_secs"),
            Self::RecentHours => ("docker", "recent_hours"),
            Self::Protected => ("delete", "protected"),
        }
    }

    fn items(self, config: &Config) -> Vec<String> {
        match self {
            Self::Roots => config
                .scan
                .roots
                .iter()
                .map(|root| config::home_relative(root))
                .collect(),
            Self::Exclude => config.scan.exclude.clone(),
            Self::KeepBranches => config.git.keep_branches.clone(),
            Self::Tools => config
                .toolchains
                .tools
                .iter()
                .map(|tool| tool.name().to_owned())
                .collect(),
            Self::Engines => config.docker.engines.clone(),
            Self::Protected => config
                .delete
                .protected
                .iter()
                .map(|path| config::home_relative(path))
                .collect(),
            _ => Vec::new(),
        }
    }

    fn text(self, config: &Config) -> String {
        match self {
            Self::MinSize => config.scan.min_size.to_string(),
            Self::SizeThreads => config.scan.size_threads.to_string(),
            Self::DuplicateMinSize => config.duplicates.min_size.to_string(),
            Self::DuplicateThreads => config.duplicates.threads.to_string(),
            Self::MinGain => config.git.min_gain.to_string(),
            Self::GitThreads => config.git.threads.to_string(),
            Self::DockerTimeout => config.docker.timeout_secs.to_string(),
            Self::RecentHours => config.docker.recent_hours.to_string(),
            _ => String::new(),
        }
    }

    fn summary(self, config: &Config) -> String {
        match self.kind() {
            Kind::List => {
                let items = self.items(config);
                if items.is_empty() {
                    "None".to_owned()
                } else {
                    items.join(", ")
                }
            }
            Kind::Number
                if self.text(config) == "0"
                    && matches!(
                        self,
                        Self::SizeThreads | Self::DuplicateThreads | Self::GitThreads
                    ) =>
            {
                "Automatic".to_owned()
            }
            _ => self.text(config),
        }
    }

    fn placeholder(self) -> &'static str {
        match self.kind() {
            Kind::List => match self {
                Self::Roots | Self::Protected => "Folder path",
                Self::Exclude => "Pattern",
                Self::KeepBranches => "Branch name",
                Self::Tools => "Version manager",
                _ => "Name",
            },
            Kind::Size => "Size, such as 4KiB or 1MB",
            Kind::Number => "Whole number",
        }
    }

    fn separators(self) -> &'static [char] {
        match self {
            Self::Roots | Self::Exclude | Self::Protected => field::PATH,
            _ => field::WORDS,
        }
    }

    fn request(self, config: &Config, input: &str) -> Result<Option<Write>, String> {
        let value = match self.kind() {
            Kind::Number => {
                let number: u32 = input
                    .parse()
                    .map_err(|_| "Enter a whole number, 0 or more".to_owned())?;
                SettingValue::Integer(i64::from(number))
            }
            Kind::Size if input.is_empty() => return Err("Enter a size".to_owned()),
            Kind::Size => SettingValue::Text(input.to_owned()),
            Kind::List if input.is_empty() => return Ok(None),
            Kind::List => {
                let (entry, remove) = input
                    .strip_prefix('-')
                    .map_or((input, false), |rest| (rest.trim(), true));
                if self == Self::Protected {
                    return protected(config, entry, remove).map(Some);
                }
                let entry = match self {
                    Self::Roots => config::home_relative(&config::expand_home(Path::new(entry))),
                    Self::Tools => entry.to_ascii_lowercase(),
                    _ => entry.to_owned(),
                };
                let mut items = self.items(config);
                let known = items.contains(&entry);
                match (remove, known) {
                    (true, false) => return Err(format!("{entry} is not in the list")),
                    (false, true) => return Err(format!("{entry} is already in the list")),
                    (true, true) => items.retain(|item| *item != entry),
                    (false, false) => items.push(entry),
                }
                SettingValue::List(items)
            }
        };
        let (table, key) = self.key();
        let parsed = config::check_setting(table, key, &value)?;
        Ok(Some(Write::Setting {
            table,
            key,
            value,
            parsed: Box::new(parsed),
        }))
    }

    fn apply(self, config: &mut Config, parsed: Config) {
        match self {
            Self::Roots => config.scan.roots = parsed.scan.roots,
            Self::Exclude => config.scan.exclude = parsed.scan.exclude,
            Self::MinSize => config.scan.min_size = parsed.scan.min_size,
            Self::SizeThreads => config.scan.size_threads = parsed.scan.size_threads,
            Self::DuplicateMinSize => config.duplicates.min_size = parsed.duplicates.min_size,
            Self::DuplicateThreads => config.duplicates.threads = parsed.duplicates.threads,
            Self::MinGain => config.git.min_gain = parsed.git.min_gain,
            Self::KeepBranches => config.git.keep_branches = parsed.git.keep_branches,
            Self::GitThreads => config.git.threads = parsed.git.threads,
            Self::Tools => config.toolchains.tools = parsed.toolchains.tools,
            Self::Engines => config.docker.engines = parsed.docker.engines,
            Self::DockerTimeout => config.docker.timeout_secs = parsed.docker.timeout_secs,
            Self::RecentHours => config.docker.recent_hours = parsed.docker.recent_hours,
            Self::Protected => config.delete.protected = parsed.delete.protected,
        }
    }
}

fn protected(config: &Config, entry: &str, remove: bool) -> Result<Write, String> {
    let path = config::expand_home(Path::new(entry));
    let mut paths = config.delete.protected.clone();
    let known = paths.contains(&path);
    match (remove, known) {
        (true, false) => return Err("This folder is absent from the list".to_owned()),
        (false, true) => return Err("This folder is already protected".to_owned()),
        (true, true) => paths.retain(|known| *known != path),
        (false, false) => paths.push(path),
    }
    Ok(Write::Protected(paths))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Setting(Setting),
    Field(Field),
    Key(Key),
}

struct Group {
    title: &'static str,
    hint: Option<&'static str>,
    rows: &'static [Row],
}

struct Page {
    title: &'static str,
    detail: &'static str,
    groups: &'static [Group],
}

const PAGES: [Page; 8] = [
    Page {
        title: "General",
        detail: "Choose how kuizer looks and opens.",
        groups: &[
            Group {
                title: "Theme",
                hint: Some("Applied to the whole window as soon as you pick it."),
                rows: &[
                    Row::Setting(Setting::Theme),
                    Row::Setting(Setting::Accent),
                    Row::Setting(Setting::Density),
                ],
            },
            Group {
                title: "Interface",
                hint: None,
                rows: &[
                    Row::Setting(Setting::Motion),
                    Row::Setting(Setting::SidebarCollapsed),
                    Row::Setting(Setting::Zoom),
                    Row::Setting(Setting::UpdateCheck),
                ],
            },
        ],
    },
    Page {
        title: "Deleting",
        detail: "Choose what happens to the items you remove.",
        groups: &[Group {
            title: "Removed items",
            hint: None,
            rows: &[
                Row::Setting(Setting::Trash),
                Row::Setting(Setting::CrossDevice),
                Row::Field(Field::Protected),
            ],
        }],
    },
    Page {
        title: "Scanning",
        detail: "Choose where scans look and what they skip. Changes apply to the next scan.",
        groups: &[
            Group {
                title: "Folders",
                hint: None,
                rows: &[Row::Field(Field::Roots), Row::Field(Field::Exclude)],
            },
            Group {
                title: "Limits",
                hint: None,
                rows: &[Row::Field(Field::MinSize), Row::Field(Field::SizeThreads)],
            },
        ],
    },
    Page {
        title: "Duplicates",
        detail: "Choose when and how kuizer looks for duplicate files.",
        groups: &[Group {
            title: "Search",
            hint: None,
            rows: &[
                Row::Setting(Setting::DuplicatesAuto),
                Row::Setting(Setting::DuplicatesVerify),
                Row::Setting(Setting::DuplicatesHidden),
                Row::Field(Field::DuplicateMinSize),
                Row::Field(Field::DuplicateThreads),
            ],
        }],
    },
    Page {
        title: "Git",
        detail: "Choose which repositories and branches are listed.",
        groups: &[Group {
            title: "Repositories",
            hint: None,
            rows: &[
                Row::Field(Field::MinGain),
                Row::Setting(Setting::GitAggressive),
                Row::Field(Field::KeepBranches),
                Row::Field(Field::GitThreads),
            ],
        }],
    },
    Page {
        title: "Tools",
        detail: "Choose which toolchains and container engines are listed.",
        groups: &[
            Group {
                title: "Toolchains",
                hint: None,
                rows: &[
                    Row::Field(Field::Tools),
                    Row::Setting(Setting::ToolchainPins),
                ],
            },
            Group {
                title: "Docker",
                hint: None,
                rows: &[
                    Row::Field(Field::Engines),
                    Row::Field(Field::DockerTimeout),
                    Row::Field(Field::RecentHours),
                ],
            },
        ],
    },
    Page {
        title: "Shortcuts",
        detail: "Keys that jump between sections.",
        groups: &[Group {
            title: "Sidebar sections",
            hint: None,
            rows: &[
                Row::Setting(Setting::RoutesEnabled),
                Row::Setting(Setting::RouteHints),
            ],
        }],
    },
    Page {
        title: "Keys",
        detail: "Every action with its keys. The window and the terminal UI read the same keys, so a change applies to both.",
        groups: &[],
    },
];

const KEYS_PAGE: usize = PAGES.len() - 1;

fn rows(state: &Settings, keys: &Keymap) -> Vec<Row> {
    if state.page == KEYS_PAGE {
        return key_actions(&state.keys.filter, keys)
            .into_iter()
            .map(Row::Key)
            .collect();
    }
    PAGES[state.page]
        .groups
        .iter()
        .flat_map(|group| group.rows.iter().copied())
        .collect()
}

const AREAS: [&str; 12] = [
    "Window",
    "Space map",
    "Controls",
    "Start screen",
    "Palette",
    "Filter",
    "Settings",
    "Menu",
    "Dialogs",
    "Text fields",
    "Shortcuts overlay",
    "Terminal",
];

fn area(scope: Scope) -> &'static str {
    if scope.terminal() {
        return "Terminal";
    }
    match scope {
        Scope::Map => "Space map",
        Scope::Control => "Controls",
        Scope::StartControl | Scope::Folder => "Start screen",
        Scope::Palette => "Palette",
        Scope::GuiFilter => "Filter",
        Scope::Settings => "Settings",
        Scope::Menu => "Menu",
        Scope::Dialog => "Dialogs",
        Scope::PathEditor | Scope::Field => "Text fields",
        Scope::Shortcuts => "Shortcuts overlay",
        _ => "Window",
    }
}

fn home_scope(action: Key) -> Scope {
    action.scopes().next().unwrap_or(Scope::Gui)
}

fn key_actions(filter: &str, keys: &Keymap) -> Vec<Key> {
    let needle = filter.trim().to_lowercase();
    let mut actions: Vec<Key> = Key::all()
        .filter(|&action| {
            needle.is_empty()
                || action.name().contains(&needle)
                || action.scopes().any(|scope| {
                    keys.keys(scope, action).any(|key| {
                        key.to_lowercase().contains(&needle)
                            || commands::label(key).to_lowercase().contains(&needle)
                    })
                })
        })
        .collect();
    actions.sort_by_key(|&action| {
        let area = area(home_scope(action));
        AREAS.iter().position(|known| *known == area)
    });
    actions
}

fn labels<'a>(keys: impl Iterator<Item = &'a str>) -> String {
    keys.map(commands::label).collect::<Vec<_>>().join(", ")
}

fn problem_text(action: Key, problem: &KeyError) -> String {
    if problem.name == action.name() {
        format!("Not saved: {}", problem.message)
    } else {
        format!("Not saved: {problem}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Record {
    Replace,
    Add,
}

/// The state of the Keys page: its filter, the action being recorded and the last refusal.
#[derive(Debug, Default)]
struct KeyEditor {
    filter: String,
    filtering: bool,
    recording: Option<(Key, Record)>,
    chip: usize,
    error: Option<(Key, String)>,
}

#[derive(Debug)]
struct Editor {
    field: Field,
    text: String,
    error: Option<String>,
}

/// Which page of the settings popover is open and which row has the cursor.
#[derive(Debug, Default)]
pub(super) struct Settings {
    page: usize,
    row: usize,
    editor: Option<Editor>,
    keys: KeyEditor,
    scroll: ScrollHandle,
    reveal: Rc<Cell<bool>>,
}

impl Kuizer {
    pub(super) fn open_settings(&mut self, cx: &mut Context<Self>) {
        self.overlays.palette = None;
        self.overlays.menu = None;
        self.overlays.shortcuts = false;
        self.list.filtering = false;
        self.overlays.settings = Some(Settings::default());
        cx.notify();
    }

    pub(super) fn settings_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) {
        if self
            .overlays
            .settings
            .as_ref()
            .is_some_and(|state| state.editor.is_some())
        {
            self.field_key(keystroke, cx);
            return;
        }
        if self.recording_keys() {
            self.record_key(keystroke, cx);
            return;
        }
        if self
            .overlays
            .settings
            .as_ref()
            .is_some_and(|state| state.keys.filtering)
        {
            self.key_filter_key(keystroke, cx);
            return;
        }
        if commands::lookup(&self.keymap, keystroke, false) == Some(Command::Settings) {
            self.overlays.settings = None;
            cx.notify();
            return;
        }
        let Some(state) = &mut self.overlays.settings else {
            return;
        };
        let action = self
            .keymap
            .lookup(Scope::Settings, commands::stroke(keystroke));
        let page = rows(state, &self.keymap);
        let last = page.len().saturating_sub(1);
        let row = page.get(state.row).copied();
        match action {
            Some(Key::Back) => self.overlays.settings = None,
            Some(Key::SettingsNext) => move_to(state, (state.row + 1).min(last)),
            Some(Key::SettingsPrevious) => move_to(state, state.row.saturating_sub(1)),
            Some(Key::SettingsFirst) => move_to(state, 0),
            Some(Key::SettingsLast) => move_to(state, last),
            Some(Key::SettingsRight | Key::SettingsLeft) => {
                let forward = action == Some(Key::SettingsRight);
                if let Some(Row::Setting(setting)) = row
                    && setting.stepped()
                {
                    let delta = if forward { 1 } else { -1 };
                    self.change(setting, setting.step(&self.config, delta), cx);
                    return;
                }
                if let Some(Row::Key(key)) = row {
                    let count = self.keymap.keys(home_scope(key), key).count();
                    let chip = state.keys.chip.min(count.saturating_sub(1));
                    state.keys.chip = if forward {
                        (chip + 1).min(count.saturating_sub(1))
                    } else {
                        chip.saturating_sub(1)
                    };
                } else {
                    switch_page(state, if forward { 1 } else { PAGES.len() - 1 });
                }
            }
            Some(Key::SettingsPreviousTab) => switch_page(state, PAGES.len() - 1),
            Some(Key::SettingsNextTab) => switch_page(state, 1),
            Some(Key::SettingsChange) => {
                match row {
                    Some(Row::Setting(setting)) => {
                        self.change(setting, setting.next(&self.config), cx);
                    }
                    Some(Row::Field(field)) => self.edit_field(field, cx),
                    Some(Row::Key(key)) => self.start_recording(key, Record::Replace, cx),
                    None => {}
                }
                return;
            }
            Some(Key::SettingsFilter) if state.page == KEYS_PAGE => {
                state.keys.filtering = true;
                state.keys.error = None;
            }
            Some(Key::SettingsAddKey) => {
                if let Some(Row::Key(key)) = row {
                    self.start_recording(key, Record::Add, cx);
                }
                return;
            }
            Some(Key::SettingsRemoveKey) => {
                if let Some(Row::Key(key)) = row {
                    self.remove_key(key, cx);
                }
                return;
            }
            Some(Key::SettingsResetKey) => {
                if let Some(Row::Key(key)) = row {
                    self.reset_key(key, cx);
                }
                return;
            }
            _ => return,
        }
        cx.notify();
    }

    /// Whether the Keys page waits for the key combination that becomes a binding.
    pub(super) fn recording_keys(&self) -> bool {
        self.overlays
            .settings
            .as_ref()
            .is_some_and(|state| state.keys.recording.is_some())
    }

    pub(super) fn settings_mode(&self) -> keys::Mode {
        match &self.overlays.settings {
            Some(state) if state.keys.recording.is_some() => keys::Mode::Record,
            Some(state) if state.keys.filtering => keys::Mode::Filter,
            _ => keys::Mode::Settings,
        }
    }

    fn key_filter_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) {
        let Some(state) = &mut self.overlays.settings else {
            return;
        };
        let last = rows(state, &self.keymap).len().saturating_sub(1);
        match self
            .keymap
            .lookup(Scope::GuiFilter, commands::stroke(keystroke))
        {
            Some(Key::ApplyFilter) => state.keys.filtering = false,
            Some(Key::Back) => {
                state.keys.filtering = false;
                state.keys.filter.clear();
                move_to(state, 0);
            }
            Some(Key::FilterDown) => move_to(state, (state.row + 1).min(last)),
            Some(Key::FilterUp) => move_to(state, state.row.saturating_sub(1)),
            _ => {
                let paste = || cx.read_from_clipboard().and_then(|item| item.text());
                if !field::edit(
                    &self.keymap,
                    &mut state.keys.filter,
                    keystroke,
                    field::WORDS,
                    paste,
                ) {
                    return;
                }
                move_to(state, 0);
            }
        }
        cx.notify();
    }

    fn start_recording(&mut self, key: Key, record: Record, cx: &mut Context<Self>) {
        if self.overlays.setting_writes.keys.is_some() {
            return;
        }
        let Some(state) = &mut self.overlays.settings else {
            return;
        };
        if self.config_path.is_none() {
            state.keys.error = Some((key, "This system has no config folder".to_owned()));
        } else {
            state.keys.recording = Some((key, record));
            state.keys.error = None;
        }
        cx.notify();
    }

    fn record_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) {
        let Some((key, record)) = self
            .overlays
            .settings
            .as_mut()
            .and_then(|state| state.keys.recording.take())
        else {
            return;
        };
        let stroke = commands::stroke(keystroke);
        if self.keymap.lookup(Scope::Settings, stroke) == Some(Key::Back) {
            cx.notify();
            return;
        }
        let Some(text) = keymap::binding_text(stroke) else {
            let label = commands::label(&keystroke.unparse());
            self.refuse_keys(key, format!("Not saved: {label} cannot be bound"), cx);
            return;
        };
        let mut list: Vec<String> = match record {
            Record::Replace => Vec::new(),
            Record::Add => self
                .keymap
                .keys(home_scope(key), key)
                .map(str::to_owned)
                .collect(),
        };
        if list.contains(&text) {
            let message = format!("{} is already one of its keys", commands::label(&text));
            self.refuse_keys(key, message, cx);
            return;
        }
        list.push(text);
        if let Some(state) = &mut self.overlays.settings {
            state.keys.chip = list.len() - 1;
        }
        self.write_keys(key, Some(list), cx);
    }

    fn remove_key(&mut self, key: Key, cx: &mut Context<Self>) {
        let Some(state) = &self.overlays.settings else {
            return;
        };
        let mut list: Vec<String> = self
            .keymap
            .keys(home_scope(key), key)
            .map(str::to_owned)
            .collect();
        if list.is_empty() {
            return;
        }
        let chip = state.keys.chip.min(list.len() - 1);
        list.remove(chip);
        if list.is_empty() {
            let message = format!(
                "Not saved: an action needs at least one key. {} resets it to the defaults.",
                self.key_hint(Scope::Settings, Key::SettingsResetKey)
            );
            self.refuse_keys(key, message, cx);
            return;
        }
        self.write_keys(key, Some(list), cx);
    }

    fn reset_key(&mut self, key: Key, cx: &mut Context<Self>) {
        if self.config.keys.contains_key(key.name()) {
            self.write_keys(key, None, cx);
        }
    }

    fn refuse_keys(&mut self, key: Key, message: String, cx: &mut Context<Self>) {
        if let Some(state) = &mut self.overlays.settings {
            state.keys.error = Some((key, message));
        }
        cx.notify();
    }

    fn write_keys(&mut self, key: Key, list: Option<Vec<String>>, cx: &mut Context<Self>) {
        if self.overlays.setting_writes.keys.is_some() {
            return;
        }
        let Some(file) = self.config_path.clone() else {
            self.refuse_keys(key, "This system has no config folder".to_owned(), cx);
            return;
        };
        let entry = list.map(|mut list| {
            if list.len() == 1 {
                Override::One(list.remove(0))
            } else {
                Override::Many(list)
            }
        });
        let next = match keymap::rebind(&self.config.keys, key.name(), entry.clone()) {
            Ok(next) => next,
            Err(problem) => {
                self.refuse_keys(key, problem_text(key, &problem), cx);
                return;
            }
        };
        if let Some(state) = &mut self.overlays.settings {
            state.keys.error = None;
        }
        self.overlays.setting_writes.keys = Some(key);
        let written = cx.background_spawn(async move {
            let _one_writer = CONFIG_WRITE
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            config::set_keys(&file, key.name(), entry.as_ref())
                .map(|()| next)
                .map_err(|error| match error {
                    EditError::Keys { source, .. } => problem_text(key, &source),
                    other => format!("Not saved: {other}"),
                })
        });
        cx.spawn(async move |this, cx| {
            let result = written.await;
            let _ = this.update(cx, |app, cx| app.keys_written(key, result, cx));
        })
        .detach();
        cx.notify();
    }

    fn keys_written(
        &mut self,
        key: Key,
        result: Result<Overrides, String>,
        cx: &mut Context<Self>,
    ) {
        self.overlays.setting_writes.keys = None;
        match result {
            Ok(keys) => self.apply_keys(keys, cx),
            Err(error) if self.overlays.settings.is_some() => self.refuse_keys(key, error, cx),
            Err(error) => self.tell(Notice::Failed(error), cx),
        }
        cx.notify();
    }

    /// Makes a new set of `[keys]` overrides the live keymap, including the keys GPUI dispatches
    /// itself, so a rebind works without a restart.
    fn apply_keys(&mut self, keys: Overrides, cx: &mut Context<Self>) {
        self.keymap = Keymap::resolve(&keys).0;
        self.config.keys = keys;
        cx.clear_key_bindings();
        commands::bind_configured_keys(cx, &self.config.keys);
    }

    pub(super) fn set_zoom(&mut self, percent: u16, cx: &mut Context<Self>) {
        let percent = zoom::snap(percent);
        if percent == zoom::snap(self.config.appearance.zoom) {
            return;
        }
        if self.config_path.is_none() {
            self.config.appearance.zoom = percent;
            cx.notify();
            return;
        }
        self.change(Setting::Zoom, zoom_step(percent), cx);
    }

    fn edit_field(&mut self, field: Field, cx: &mut Context<Self>) {
        if self.overlays.setting_writes.field.is_some() {
            return;
        }
        let text = field.text(&self.config);
        let Some(state) = &mut self.overlays.settings else {
            return;
        };
        if let Some(at) = rows(state, &self.keymap)
            .iter()
            .position(|row| *row == Row::Field(field))
        {
            state.row = at;
        }
        state.editor = Some(Editor {
            field,
            text,
            error: None,
        });
        cx.notify();
    }

    pub(super) fn saving_protected_paths(&self) -> bool {
        self.overlays.setting_writes.field == Some(Field::Protected)
    }

    fn field_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) {
        if self.overlays.setting_writes.field.is_some() {
            return;
        }
        let Some(state) = &mut self.overlays.settings else {
            return;
        };
        match self
            .keymap
            .lookup(Scope::PathEditor, commands::stroke(keystroke))
        {
            Some(Key::Back) => state.editor = None,
            Some(Key::SaveProtectedPath) => {
                self.save_field(cx);
                return;
            }
            _ => {
                let Some(editor) = &mut state.editor else {
                    return;
                };
                let paste = || cx.read_from_clipboard().and_then(|item| item.text());
                let separators = editor.field.separators();
                if !field::edit(&self.keymap, &mut editor.text, keystroke, separators, paste) {
                    return;
                }
                editor.error = None;
            }
        }
        cx.notify();
    }

    fn save_field(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self
            .overlays
            .settings
            .as_mut()
            .and_then(|state| state.editor.as_mut())
        else {
            return;
        };
        let field = editor.field;
        let Some(file) = self.config_path.clone() else {
            editor.error = Some("This system has no config folder".to_owned());
            cx.notify();
            return;
        };
        let write = match field.request(&self.config, editor.text.trim()) {
            Ok(Some(write)) => write,
            Ok(None) => {
                if let Some(state) = &mut self.overlays.settings {
                    state.editor = None;
                }
                cx.notify();
                return;
            }
            Err(error) => {
                editor.error = Some(error);
                cx.notify();
                return;
            }
        };
        editor.error = None;
        self.overlays.setting_writes.field = Some(field);
        let written = cx.background_spawn(async move {
            let _one_writer = CONFIG_WRITE
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            write.run(&file)
        });
        cx.spawn(async move |this, cx| {
            let result = written.await;
            let _ = this.update(cx, |app, cx| app.field_written(field, result, cx));
        })
        .detach();
        cx.notify();
    }

    fn field_written(
        &mut self,
        field: Field,
        result: Result<Box<Config>, String>,
        cx: &mut Context<Self>,
    ) {
        self.overlays.setting_writes.field = None;
        match result {
            Ok(parsed) => {
                field.apply(&mut self.config, *parsed);
                if field == Field::MinSize {
                    self.session.options.min_size = self.config.scan.min_size.0;
                }
                if let Some(state) = &mut self.overlays.settings
                    && state
                        .editor
                        .as_ref()
                        .is_some_and(|editor| editor.field == field)
                {
                    state.editor = None;
                }
            }
            Err(error) => match self
                .overlays
                .settings
                .as_mut()
                .and_then(|state| state.editor.as_mut())
                .filter(|editor| editor.field == field)
            {
                Some(editor) => editor.error = Some(error),
                None => self.tell(
                    Notice::Failed(format!("Could not save {}: {error}", field.label())),
                    cx,
                ),
            },
        }
        cx.notify();
    }

    fn change(&mut self, setting: Setting, value: usize, cx: &mut Context<Self>) {
        let Some(file) = self.config_path.clone() else {
            self.tell(
                Notice::Failed(
                    "Could not save the setting: this system has no config folder".into(),
                ),
                cx,
            );
            return;
        };
        let was = setting.value(&self.config);
        self.apply_setting(setting, value);
        if setting == Setting::Zoom
            && let Some(state) = &self.overlays.settings
        {
            state.reveal.set(true);
        }
        self.overlays.setting_writes.queue(setting, was, value);
        self.write_next_setting(file, cx);
        cx.notify();
    }

    fn apply_setting(&mut self, setting: Setting, value: usize) {
        setting.set(&mut self.config, value);
        if setting == Setting::DuplicatesAuto {
            self.session.options.duplicates = self.config.duplicates.enabled;
        }
    }

    fn write_next_setting(&mut self, file: PathBuf, cx: &mut Context<Self>) {
        let Some((setting, value)) = self.overlays.setting_writes.next() else {
            return;
        };
        let (table, key, entry) = setting.entry(value);
        let target = file.clone();
        let written = cx.background_spawn(async move {
            let _one_writer = CONFIG_WRITE
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            kuizer::config::set_setting(&target, table, key, entry)
                .map_err(|error| error.to_string())
        });
        cx.spawn(async move |this, cx| {
            let written = written.await;
            let _ = this.update(cx, |app, cx| {
                app.setting_written(setting, value, written, cx);
                app.write_next_setting(file, cx);
            });
        })
        .detach();
    }

    pub(super) fn setting_written(
        &mut self,
        setting: Setting,
        value: usize,
        written: Result<(), String>,
        cx: &mut Context<Self>,
    ) {
        let rollback = self
            .overlays
            .setting_writes
            .finished(setting, value, written.is_ok());
        if let Err(error) = written {
            if let Some(saved) = rollback {
                self.apply_setting(setting, saved);
            }
            self.tell(
                Notice::Failed(format!("Could not save {}: {error}", setting.label())),
                cx,
            );
            cx.notify();
        }
    }

    pub(super) fn render_settings(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let state = self.overlays.settings.as_ref()?;
        let nav = PAGES.iter().enumerate().map(|(index, page)| {
            let active = index == state.page;
            div()
                .id(("settings-page", index))
                .debug_selector(move || format!("settings-page-{}", page.title))
                .flex()
                .items_center()
                .h(unit(size::row()))
                .px(unit(space::M))
                .rounded(theme::radius_control())
                .cursor_pointer()
                .text_color(if active {
                    theme::text()
                } else {
                    theme::muted()
                })
                .when(active, |this| {
                    this.bg(theme::hover()).font_weight(FontWeight::MEDIUM)
                })
                .when(!active, |this| {
                    this.hover(|style| style.bg(theme::hover_row()).text_color(theme::text()))
                })
                .child(page.title)
                .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                    if let Some(state) = &mut app.overlays.settings {
                        state.page = index;
                        state.row = 0;
                        state.editor = None;
                        state.keys.recording = None;
                        state.keys.filtering = false;
                        state.keys.error = None;
                        cx.notify();
                    }
                }))
        });
        let nav: Vec<_> = nav.collect();
        let page = &PAGES[state.page];
        let mut position = 0;
        let groups: Vec<_> = if state.page == KEYS_PAGE {
            self.key_groups(state, cx)
        } else {
            page.groups
                .iter()
                .map(|group| {
                    let last_row = group.rows.len() - 1;
                    let rows: Vec<_> = group
                        .rows
                        .iter()
                        .enumerate()
                        .map(|(at, &row)| {
                            let index = position;
                            position += 1;
                            let rule = at < last_row;
                            let line = match row {
                                Row::Setting(setting) => {
                                    self.setting_row(setting, index, state.row, rule, cx)
                                }
                                Row::Field(field) => self.field_row(field, index, state, rule, cx),
                                Row::Key(key) => self.key_row(key, index, state, rule, cx),
                            };
                            line.relative().when(index == state.row, |line| {
                                line.child(reveal(state.scroll.clone(), Rc::clone(&state.reveal)))
                            })
                        })
                        .collect();
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(unit(space::XXS))
                                .pb(unit(space::M))
                                .child(ui::heading(group.title))
                                .children(group.hint.map(|hint| {
                                    div()
                                        .text_size(unit(font::SMALL))
                                        .text_color(theme::faint())
                                        .child(hint)
                                })),
                        )
                        .children(rows)
                        .into_any_element()
                })
                .collect()
        };
        let hint = |keys: &[Key], what: &'static str| {
            div()
                .flex()
                .items_center()
                .gap(unit(space::XS))
                .children(
                    keys.iter()
                        .map(|key| ui::kbd(self.key_hint(Scope::Settings, *key))),
                )
                .child(div().pl(unit(space::XXS)).child(what))
        };
        let footer = div()
            .flex()
            .flex_col()
            .flex_none()
            .gap(unit(space::S))
            .px(unit(space::M))
            .pt(unit(space::L))
            .text_size(unit(font::CAPTION))
            .text_color(theme::faint())
            .child(hint(&[Key::SettingsPrevious, Key::SettingsNext], "move"))
            .child(hint(&[Key::SettingsNextTab], "section"))
            .child(hint(
                &[Key::SettingsChange],
                if state.page == KEYS_PAGE {
                    "record"
                } else {
                    "change"
                },
            ))
            .child(hint(&[Key::Back], "close"));
        let panel = div()
            .id("settings")
            .debug_selector(|| "settings".to_owned())
            .flex()
            .w(unit(size::SETTINGS))
            .h(unit(size::SETTINGS_HEIGHT))
            .max_w(gpui::relative(0.92))
            .max_h(gpui::relative(0.9))
            .rounded(theme::radius_dialog())
            .bg(theme::elevated())
            .border_1()
            .border_color(theme::border_strong())
            .font_family(self.chrome.font.clone())
            .text_size(unit(font::BODY))
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_none()
                    .gap(unit(space::XXS))
                    .w(unit(size::SETTINGS_NAV))
                    .p(unit(space::L))
                    .bg(theme::panel())
                    .border_r_1()
                    .border_color(theme::border())
                    .child(
                        div()
                            .px(unit(space::M))
                            .pb(unit(space::L))
                            .text_size(unit(font::TITLE))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Settings"),
                    )
                    .children(nav)
                    .child(div().flex_1())
                    .child(footer),
            )
            .child(
                div()
                    .id("settings-content")
                    .flex_1()
                    .min_w_0()
                    .overflow_y_scroll()
                    .track_scroll(&state.scroll)
                    .px(unit(space::XXL))
                    .pt(unit(space::XXL))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(unit(space::XXL))
                            .pb(unit(space::XXL))
                            .w_full()
                            .max_w(unit(size::SETTINGS_CONTENT))
                            .mx_auto()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(unit(space::XS))
                                    .child(
                                        div()
                                            .text_size(unit(font::FIGURE))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(page.title),
                                    )
                                    .child(div().text_color(theme::muted()).child(page.detail)),
                            )
                            .children(groups),
                    ),
            );
        Some(
            div()
                .id("settings-backdrop")
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(theme::scrim())
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|app, _: &MouseDownEvent, _, cx| {
                        app.overlays.settings = None;
                        cx.notify();
                    }),
                )
                .child(panel)
                .into_any_element(),
        )
    }

    fn field_row(
        &self,
        field: Field,
        index: usize,
        state: &Settings,
        rule: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let focused = index == state.row;
        let editor = state.editor.as_ref().filter(|editor| editor.field == field);
        let list = field.kind() == Kind::List;
        let summary = field.summary(&self.config);
        let label = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .gap(unit(space::XXS))
            .child(div().font_weight(FontWeight::MEDIUM).child(field.label()))
            .child(
                div()
                    .text_size(unit(font::SMALL))
                    .text_color(theme::muted())
                    .child(field.hint()),
            )
            .when(list, |this| {
                this.child(div().text_color(theme::text()).child(summary.clone()))
            });
        div()
            .id(("setting", index))
            .debug_selector(move || format!("setting-{}", field.label()))
            .flex()
            .flex_col()
            .gap(unit(space::XS))
            .px(unit(space::M))
            .py(unit(space::row_y()))
            .rounded(theme::radius_control())
            .when(rule, |this| this.border_b_1().border_color(theme::border()))
            .when(focused, |this| this.bg(theme::selected()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(unit(space::L))
                    .child(label)
                    .when(!list && editor.is_none(), |this| {
                        this.child(div().flex_none().text_color(theme::muted()).child(summary))
                    }),
            )
            .when(editor.is_none(), |row| {
                row.cursor_pointer()
                    .when(!focused, |this| {
                        this.hover(|style| style.bg(theme::hover()))
                    })
                    .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                        app.edit_field(field, cx);
                    }))
            })
            .when_some(editor, |row, editor| {
                let detail = if self.overlays.setting_writes.field == Some(field) {
                    "Saving".to_owned()
                } else {
                    let keys = format!(
                        "{} saves. {} cancels.",
                        self.key_hint(Scope::PathEditor, Key::SaveProtectedPath),
                        self.key_hint(Scope::PathEditor, Key::Back),
                    );
                    if list {
                        format!("{keys} Type an entry to add it, or - and an entry to remove it.")
                    } else {
                        keys
                    }
                };
                row.child(field::view(
                    ("setting-input", index),
                    &editor.text,
                    field.placeholder(),
                    true,
                ))
                .child(
                    div()
                        .text_size(unit(font::SMALL))
                        .text_color(theme::muted())
                        .child(detail),
                )
                .children(
                    editor
                        .error
                        .clone()
                        .map(|error| div().text_color(theme::review()).child(error)),
                )
            })
    }

    fn key_groups(&self, state: &Settings, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let actions = key_actions(&state.keys.filter, &self.keymap);
        let hint = |key: Key, what: &'static str| {
            div()
                .flex()
                .items_center()
                .gap(unit(space::XS))
                .child(ui::kbd(self.key_hint(Scope::Settings, key)))
                .child(what)
        };
        let placeholder = format!(
            "{} filters by action or key",
            self.key_hint(Scope::Settings, Key::SettingsFilter)
        );
        let tools = div()
            .flex()
            .flex_col()
            .gap(unit(space::M))
            .child(
                field::view(
                    "keys-filter",
                    &state.keys.filter,
                    placeholder,
                    state.keys.filtering,
                )
                .debug_selector(|| "keys-filter".to_owned())
                .on_click(cx.listener(|app, _: &ClickEvent, _, cx| {
                    if let Some(state) = &mut app.overlays.settings {
                        state.keys.filtering = true;
                        cx.notify();
                    }
                })),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(unit(space::L))
                    .text_size(unit(font::CAPTION))
                    .text_color(theme::faint())
                    .child(hint(Key::SettingsChange, "record a new key"))
                    .child(hint(Key::SettingsAddKey, "add one"))
                    .child(hint(Key::SettingsRemoveKey, "remove the picked key"))
                    .child(hint(Key::SettingsResetKey, "back to the default")),
            );
        let mut groups = vec![tools.into_any_element()];
        if actions.is_empty() {
            groups.push(ui::empty("No action or key matches the filter").into_any_element());
            return groups;
        }
        let mut start = 0;
        while start < actions.len() {
            let title = area(home_scope(actions[start]));
            let end = actions[start..]
                .iter()
                .position(|&action| area(home_scope(action)) != title)
                .map_or(actions.len(), |length| start + length);
            let rows: Vec<_> = (start..end)
                .map(|index| {
                    self.key_row(actions[index], index, state, index + 1 < end, cx)
                        .relative()
                        .when(index == state.row, |line| {
                            line.child(reveal(state.scroll.clone(), Rc::clone(&state.reveal)))
                        })
                })
                .collect();
            groups.push(
                div()
                    .flex()
                    .flex_col()
                    .child(div().pb(unit(space::M)).child(ui::heading(title)))
                    .children(rows)
                    .into_any_element(),
            );
            start = end;
        }
        groups
    }

    fn key_row(
        &self,
        key: Key,
        index: usize,
        state: &Settings,
        rule: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let focused = index == state.row;
        let home = home_scope(key);
        let current: Vec<&str> = self.keymap.keys(home, key).collect();
        let recording = state
            .keys
            .recording
            .filter(|(recorded, _)| *recorded == key)
            .map(|(_, record)| record);
        let picked = state.keys.chip.min(current.len().saturating_sub(1));
        let mut chips: Vec<AnyElement> = Vec::new();
        if recording != Some(Record::Replace) {
            chips.extend(current.iter().enumerate().map(|(at, binding)| {
                let label = commands::label(binding);
                if focused && recording.is_none() && at == picked {
                    ui::kbd_on(label, theme::bar(), theme::on_accent()).into_any_element()
                } else {
                    ui::kbd(label).into_any_element()
                }
            }));
        }
        if recording.is_some() {
            chips.push(ui::appear(
                ("key-recording", index),
                ui::kbd_on("Press a key", theme::review_soft(), theme::review()),
            ));
        }
        let mut places: Vec<&str> = Vec::new();
        let mut elsewhere: Vec<String> = Vec::new();
        for scope in key.scopes() {
            let place = area(scope);
            if !places.contains(&place) {
                places.push(place);
            }
            let keys: Vec<&str> = self.keymap.keys(scope, key).collect();
            let line = format!("{place}: {}", labels(keys.iter().copied()));
            if keys != current && !elsewhere.contains(&line) {
                elsewhere.push(line);
            }
        }
        let changed = self.config.keys.contains_key(key.name());
        let default = format!(
            "{}Default: {}",
            if changed { "Changed. " } else { "" },
            labels(DEFAULT_KEYS.keys(home, key))
        );
        let status = if self.overlays.setting_writes.keys == Some(key) {
            Some(("Saving".to_owned(), theme::muted()))
        } else if recording.is_some() {
            let cancel = self.key_hint(Scope::Settings, Key::Back);
            Some((
                format!("Press the key combination to use. {cancel} cancels."),
                theme::text(),
            ))
        } else {
            state
                .keys
                .error
                .as_ref()
                .filter(|(failed, _)| *failed == key)
                .map(|(_, error)| (error.clone(), theme::review()))
        };
        let label = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .gap(unit(space::XXS))
            .child(
                theme::mono(div())
                    .font_weight(FontWeight::MEDIUM)
                    .child(key.name()),
            )
            .child(
                div()
                    .text_size(unit(font::SMALL))
                    .text_color(theme::muted())
                    .child(places.join(", ")),
            )
            .child(
                div()
                    .text_size(unit(font::SMALL))
                    .text_color(if changed {
                        theme::muted()
                    } else {
                        theme::faint()
                    })
                    .child(default),
            )
            .children(elsewhere.into_iter().map(|line| {
                div()
                    .text_size(unit(font::SMALL))
                    .text_color(theme::faint())
                    .child(line)
            }));
        div()
            .id(("key", index))
            .debug_selector(move || format!("key-{}", key.name()))
            .flex()
            .flex_col()
            .gap(unit(space::XS))
            .px(unit(space::M))
            .py(unit(space::row_y()))
            .rounded(theme::radius_control())
            .when(rule, |this| this.border_b_1().border_color(theme::border()))
            .when(focused, |this| this.bg(theme::selected()))
            .when(!focused, |this| {
                this.hover(|style| style.bg(theme::hover()))
            })
            .cursor_pointer()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(unit(space::L))
                    .child(label)
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .flex_wrap()
                            .justify_end()
                            .max_w(unit(size::LABEL))
                            .gap(unit(space::XS))
                            .children(chips),
                    ),
            )
            .children(status.map(|(text, color)| {
                ui::appear(
                    ("key-status", index),
                    div()
                        .text_size(unit(font::SMALL))
                        .text_color(color)
                        .child(text),
                )
            }))
            .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                if let Some(state) = &mut app.overlays.settings {
                    move_to(state, index);
                }
                app.start_recording(key, Record::Replace, cx);
            }))
    }

    fn setting_row(
        &self,
        setting: Setting,
        index: usize,
        cursor: usize,
        rule: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let value = setting.value(&self.config);
        let focused = index == cursor;
        let label = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .gap(unit(space::XXS))
            .child(div().font_weight(FontWeight::MEDIUM).child(setting.label()))
            .child(
                div()
                    .text_size(unit(font::SMALL))
                    .text_color(theme::muted())
                    .child(setting.hint()),
            );
        let row = div()
            .id(("setting", index))
            .debug_selector(move || format!("setting-{}", setting.label()))
            .flex()
            .gap(unit(space::L))
            .px(unit(space::M))
            .py(unit(space::row_y()))
            .rounded(theme::radius_control())
            .when(rule, |this| this.border_b_1().border_color(theme::border()))
            .when(focused, |this| this.bg(theme::selected()));
        if setting == Setting::Zoom {
            return row
                .items_center()
                .child(label)
                .child(zoom_stepper(value, cx));
        }
        match setting.choices() {
            Some(choices) => {
                row.flex_col()
                    .child(label)
                    .child(div().flex().gap(unit(space::L)).children(
                        choices.iter().enumerate().map(|(choice, name)| {
                            if setting == Setting::Theme {
                                theme_card(setting, choice, name, choice == value, cx)
                            } else {
                                choice_chip(setting, choice, name, choice == value, cx)
                            }
                        }),
                    ))
            }
            None => row
                .items_center()
                .cursor_pointer()
                .when(!focused, |this| {
                    this.hover(|style| style.bg(theme::hover()))
                })
                .child(label)
                .child(ui::switch(value == 1))
                .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                    if let Some(state) = &mut app.overlays.settings {
                        state.row = index;
                    }
                    app.change(setting, setting.next(&app.config), cx);
                })),
        }
    }
}

fn reveal(scroll: ScrollHandle, reveal: Rc<Cell<bool>>) -> Div {
    div().absolute().inset_0().child(
        canvas(
            move |area, window, _| {
                if reveal.take() {
                    keep_visible(&scroll, area, window);
                }
            },
            |_, (), _, _| {},
        )
        .size_full(),
    )
}

fn zoom_stepper(value: usize, cx: &mut Context<Kuizer>) -> Div {
    let button = |name: &'static str, glyph: &'static str, step: usize| {
        div()
            .id(name)
            .debug_selector(move || name.to_owned())
            .flex()
            .items_center()
            .justify_center()
            .size(unit(size::CONTROL))
            .rounded(theme::radius_control())
            .border_1()
            .border_color(theme::border())
            .cursor_pointer()
            .hover(|style| style.border_color(theme::border_strong()))
            .child(glyph)
            .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                app.change(Setting::Zoom, step, cx);
            }))
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(unit(space::S))
        .child(button("zoom-out", "-", value.saturating_sub(1)))
        .child(
            div()
                .min_w(unit(size::GAUGE))
                .text_center()
                .child(format!("{}%", zoom_percent(value))),
        )
        .child(button("zoom-in", "+", (value + 1).min(ZOOM_STEPS - 1)))
}

fn theme_card(
    setting: Setting,
    choice: usize,
    name: &'static str,
    active: bool,
    cx: &mut Context<Kuizer>,
) -> Stateful<Div> {
    let swatch = theme::swatch(if choice == 1 {
        Theme::Light
    } else {
        Theme::Dark
    });
    let line = |width: f32, color: gpui::Rgba| {
        div()
            .h(unit(size::TRACK))
            .w(gpui::relative(width))
            .bg(color)
    };
    let preview = div()
        .flex()
        .h(unit(size::PREVIEW))
        .bg(swatch.background)
        .border_1()
        .border_color(theme::border_strong())
        .child(div().w(gpui::relative(0.28)).h_full().bg(swatch.panel))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .gap(unit(space::S))
                .p(unit(space::M))
                .child(line(0.5, swatch.text))
                .child(line(0.8, swatch.muted))
                .child(line(0.35, swatch.accent)),
        );
    div()
        .id(("theme-card", choice))
        .debug_selector(move || format!("theme-card-{name}"))
        .flex()
        .flex_col()
        .flex_1()
        .max_w(unit(size::CARD))
        .gap(unit(space::S))
        .p(unit(space::S))
        .border_1()
        .border_color(if active {
            theme::focus()
        } else {
            theme::border()
        })
        .cursor_pointer()
        .hover(|style| style.border_color(theme::border_strong()))
        .child(preview)
        .child(
            div()
                .px(unit(space::XS))
                .font_weight(if active {
                    FontWeight::MEDIUM
                } else {
                    FontWeight::NORMAL
                })
                .text_color(if active {
                    theme::text()
                } else {
                    theme::muted()
                })
                .child(name),
        )
        .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
            app.change(setting, choice, cx);
        }))
}

fn choice_chip(
    setting: Setting,
    choice: usize,
    name: &'static str,
    active: bool,
    cx: &mut Context<Kuizer>,
) -> Stateful<Div> {
    let accent = setting == Setting::Accent;
    let prefix = if accent { "accent-chip" } else { "choice-chip" };
    div()
        .id((prefix, choice))
        .debug_selector(move || format!("{prefix}-{name}"))
        .flex()
        .items_center()
        .gap(unit(space::S))
        .h(unit(size::CONTROL))
        .px(unit(space::M))
        .rounded(theme::radius_control())
        .border_1()
        .border_color(if active {
            theme::focus()
        } else {
            theme::border()
        })
        .cursor_pointer()
        .hover(|style| style.border_color(theme::border_strong()))
        .when(accent, |this| {
            this.child(
                div()
                    .size(unit(size::LEGEND))
                    .rounded(theme::radius_pill())
                    .bg(theme::accent_of(Accent::ALL[choice])),
            )
        })
        .child(
            div()
                .text_color(if active {
                    theme::text()
                } else {
                    theme::muted()
                })
                .child(name),
        )
        .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
            app.change(setting, choice, cx);
        }))
}

fn move_to(state: &mut Settings, row: usize) {
    if state.row != row {
        state.keys.chip = 0;
        state.keys.error = None;
    }
    state.row = row;
    state.reveal.set(true);
}

fn switch_page(state: &mut Settings, step: usize) {
    state.page = (state.page + step) % PAGES.len();
    state.row = 0;
    state.keys.chip = 0;
    state.keys.error = None;
    state.scroll.set_offset(gpui::point(px(0.), px(0.)));
}

#[cfg(test)]
mod write_tests {
    use super::*;

    #[test]
    fn a_failed_write_never_rolls_back_a_newer_value() {
        let mut writes = SettingWrites::default();
        writes.queue(Setting::Motion, 1, 0);
        assert_eq!(writes.next(), Some((Setting::Motion, 0)));
        writes.queue(Setting::Motion, 0, 1);
        assert_eq!(writes.next(), None);
        assert_eq!(writes.finished(Setting::Motion, 0, false), None);
        assert_eq!(writes.next(), Some((Setting::Motion, 1)));
        assert_eq!(writes.finished(Setting::Motion, 1, true), None);
        writes.queue(Setting::Motion, 1, 0);
        assert_eq!(writes.next(), Some((Setting::Motion, 0)));
        assert_eq!(writes.finished(Setting::Motion, 0, false), Some(1));
    }

    #[test]
    fn list_fields_add_and_remove_one_entry_and_refuse_bad_input() {
        let config = Config::default();
        let added = |field: Field, input: &str| match field.request(&config, input) {
            Ok(Some(Write::Setting { value, .. })) => Ok(value),
            Ok(_) => Err("nothing to write".to_owned()),
            Err(error) => Err(error),
        };
        assert_eq!(
            added(Field::Engines, "-docker"),
            Ok(SettingValue::List(vec!["podman".to_owned()]))
        );
        assert!(added(Field::Engines, "podman").is_err());
        assert!(added(Field::Engines, "-nerdctl").is_err());
        assert!(added(Field::Tools, "asdf").is_err());
        assert!(added(Field::Exclude, "[").is_err());
        assert!(added(Field::MinSize, "big").is_err());
        assert!(added(Field::SizeThreads, "-1").is_err());
        assert!(added(Field::SizeThreads, "65").is_err());
        assert_eq!(
            added(Field::DuplicateThreads, "4"),
            Ok(SettingValue::Integer(4))
        );
        assert!(matches!(Field::Roots.request(&config, ""), Ok(None)));
    }
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;
    use kuizer::units::ByteSize;

    use super::*;
    use crate::app::tests::{open, type_text};

    #[gpui::test]
    fn a_typed_size_is_checked_inline_and_written_to_the_config(cx: &mut TestAppContext) {
        cx.update(bind_keys);
        let dir = tempfile::tempdir().unwrap();
        let (view, cx) = open(cx, dir.path().join("recent"));
        cx.simulate_keystrokes("ctrl-, tab tab tab j j j enter ctrl-u");
        type_text(cx, "big");
        cx.simulate_keystrokes("enter");
        view.read_with(cx, |app, _| {
            let editor = app
                .overlays
                .settings
                .as_ref()
                .and_then(|state| state.editor.as_ref());
            assert!(editor.is_some_and(|editor| editor.error.is_some()));
            assert_eq!(app.config.duplicates.min_size, ByteSize(4096));
        });
        cx.simulate_keystrokes("ctrl-u");
        type_text(cx, "8KiB");
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        view.read_with(cx, |app, _| {
            assert_eq!(app.config.duplicates.min_size, ByteSize(8192));
            assert!(
                app.overlays
                    .settings
                    .as_ref()
                    .is_some_and(|state| state.editor.is_none())
            );
        });
        let written = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(written.contains("min_size = \"8KiB\""), "{written}");
        cx.simulate_keystrokes("enter escape");
        view.read_with(cx, |app, _| {
            assert!(
                app.overlays
                    .settings
                    .as_ref()
                    .is_some_and(|state| state.editor.is_none())
            );
        });
        cx.simulate_keystrokes("escape");
        view.read_with(cx, |app, _| assert!(app.overlays.settings.is_none()));
    }

    #[gpui::test]
    fn a_recorded_key_is_written_and_works_at_once_while_escape_changes_nothing(
        cx: &mut TestAppContext,
    ) {
        cx.update(bind_keys);
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config.toml");
        let (view, cx) = open(cx, dir.path().join("recent"));
        cx.simulate_keystrokes("ctrl-, shift-tab /");
        type_text(cx, "help.open");
        cx.simulate_keystrokes("enter enter");
        view.read_with(cx, |app, _| assert_eq!(app.mode(), keys::Mode::Record));
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        view.read_with(cx, |app, _| {
            assert_eq!(app.mode(), keys::Mode::Settings);
            assert!(app.config.keys.is_empty());
        });
        assert!(!file.exists());

        cx.simulate_keystrokes("enter f1");
        cx.run_until_parked();
        let written = std::fs::read_to_string(&file).unwrap();
        assert!(written.contains("\"help.open\" = \"f1\""), "{written}");
        cx.simulate_keystrokes("escape f1");
        view.read_with(cx, |app, _| {
            assert!(app.overlays.settings.is_none());
            assert!(
                app.overlays.shortcuts,
                "the new key opens the shortcuts at once"
            );
        });
    }
}
