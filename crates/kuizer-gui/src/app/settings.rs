//! The settings popover: pages of switches that read and write the config file.

use gpui::Keystroke;
use kuizer::config::{Config, SettingValue, Sidebar, Theme};

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Setting {
    Theme,
    Motion,
    SidebarCollapsed,
    RoutesEnabled,
    RouteHints,
}

const THEMES: [&str; 2] = ["Dark", "Light"];

impl Setting {
    fn label(self) -> &'static str {
        match self {
            Self::Theme => "Theme",
            Self::Motion => "Motion",
            Self::SidebarCollapsed => "Open the sidebar collapsed",
            Self::RoutesEnabled => "Section shortcuts",
            Self::RouteHints => "Show shortcut hints",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::Theme => "Dark, or light on an off-white background.",
            Self::Motion => "Animate selection, expanding rows and items leaving after a cleanup.",
            Self::SidebarCollapsed => {
                "Start as a strip of icons. The sidebar shortcut switches it while the app runs."
            }
            Self::RoutesEnabled => {
                "Shift+1 to Shift+9 open the sidebar sections. The command palette still opens every section."
            }
            Self::RouteHints => "Key badges in the sidebar and in the section tooltips.",
        }
    }

    fn choices(self) -> Option<&'static [&'static str]> {
        match self {
            Self::Theme => Some(&THEMES),
            _ => None,
        }
    }

    fn value(self, config: &Config) -> usize {
        match self {
            Self::Theme => usize::from(config.appearance.theme == Theme::Light),
            Self::Motion => usize::from(config.appearance.motion),
            Self::SidebarCollapsed => usize::from(config.appearance.sidebar == Sidebar::Collapsed),
            Self::RoutesEnabled => usize::from(config.route_shortcuts.enabled),
            Self::RouteHints => usize::from(config.route_shortcuts.hints),
        }
    }

    fn count(self) -> usize {
        self.choices().map_or(2, <[_]>::len)
    }

    fn next(self, config: &Config) -> usize {
        (self.value(config) + 1) % self.count()
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
                config.appearance.theme = if on { Theme::Light } else { Theme::Dark };
                theme::configure(config.appearance.theme);
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
            Self::RoutesEnabled => config.route_shortcuts.enabled = on,
            Self::RouteHints => config.route_shortcuts.hints = on,
        }
    }

    fn entry(self, value: usize) -> (&'static str, &'static str, SettingValue) {
        let on = value == 1;
        match self {
            Self::Theme => (
                "appearance",
                "theme",
                SettingValue::Text(if on { "light" } else { "dark" }),
            ),
            Self::Motion => ("appearance", "motion", SettingValue::Bool(on)),
            Self::SidebarCollapsed => (
                "appearance",
                "sidebar",
                SettingValue::Text(if on { "collapsed" } else { "expanded" }),
            ),
            Self::RoutesEnabled => ("route_shortcuts", "enabled", SettingValue::Bool(on)),
            Self::RouteHints => ("route_shortcuts", "hints", SettingValue::Bool(on)),
        }
    }
}

struct Group {
    title: &'static str,
    hint: Option<&'static str>,
    settings: &'static [Setting],
}

struct Page {
    title: &'static str,
    detail: &'static str,
    groups: &'static [Group],
}

const PAGES: [Page; 2] = [
    Page {
        title: "General",
        detail: "Choose how kuizer looks and opens.",
        groups: &[
            Group {
                title: "Theme",
                hint: Some("Applied to the whole window as soon as you pick it."),
                settings: &[Setting::Theme],
            },
            Group {
                title: "Interface",
                hint: None,
                settings: &[Setting::Motion, Setting::SidebarCollapsed],
            },
        ],
    },
    Page {
        title: "Shortcuts",
        detail: "Keys that jump between sections.",
        groups: &[Group {
            title: "Sidebar sections",
            hint: None,
            settings: &[Setting::RoutesEnabled, Setting::RouteHints],
        }],
    },
];

fn rows(page: usize) -> Vec<Setting> {
    PAGES[page]
        .groups
        .iter()
        .flat_map(|group| group.settings.iter().copied())
        .collect()
}

/// Which page of the settings popover is open and which row has the cursor.
#[derive(Debug, Default)]
pub(super) struct Settings {
    page: usize,
    row: usize,
}

impl Kuizer {
    pub(super) fn open_settings(&mut self, cx: &mut Context<Self>) {
        self.palette = None;
        self.menu = None;
        self.shortcuts = false;
        self.filtering = false;
        self.settings = Some(Settings::default());
        cx.notify();
    }

    pub(super) fn settings_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) {
        if commands::lookup(keystroke, false) == Some(Command::Settings) {
            self.settings = None;
            cx.notify();
            return;
        }
        let Some(state) = &mut self.settings else {
            return;
        };
        let shift = keystroke.modifiers.shift;
        let count = rows(state.page).len();
        match keystroke.key.as_str() {
            "escape" => self.settings = None,
            "down" | "j" if !shift => state.row = (state.row + 1).min(count - 1),
            "up" | "k" if !shift => state.row = state.row.saturating_sub(1),
            "g" | "home" if !shift => state.row = 0,
            "g" | "end" => state.row = count - 1,
            "right" | "l" | "left" | "h" => {
                let setting = rows(state.page)[state.row];
                if setting.choices().is_some() {
                    let delta = if matches!(keystroke.key.as_str(), "right" | "l") {
                        1
                    } else {
                        -1
                    };
                    self.change(setting, setting.step(&self.config, delta), cx);
                    return;
                }
                let step = if matches!(keystroke.key.as_str(), "right" | "l") {
                    1
                } else {
                    PAGES.len() - 1
                };
                switch_page(state, step);
            }
            "tab" if shift => switch_page(state, PAGES.len() - 1),
            "tab" => switch_page(state, 1),
            "space" | "enter" => {
                let setting = rows(state.page)[state.row];
                self.change(setting, setting.next(&self.config), cx);
                return;
            }
            _ => return,
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
        setting.set(&mut self.config, value);
        let (table, key, entry) = setting.entry(value);
        let written = cx.background_spawn(async move {
            let _one_writer = CONFIG_WRITE
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            kuizer::config::set_setting(&file, table, key, entry).map_err(|error| error.to_string())
        });
        cx.spawn(async move |this, cx| {
            let written = written.await;
            let _ = this.update(cx, |app, cx| app.setting_written(setting, was, written, cx));
        })
        .detach();
        cx.notify();
    }

    pub(super) fn setting_written(
        &mut self,
        setting: Setting,
        was: usize,
        written: Result<(), String>,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = written {
            setting.set(&mut self.config, was);
            self.tell(
                Notice::Failed(format!("Could not save {}: {error}", setting.label())),
                cx,
            );
            cx.notify();
        }
    }

    pub(super) fn render_settings(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let state = self.settings.as_ref()?;
        let nav = PAGES.iter().enumerate().map(|(index, page)| {
            let active = index == state.page;
            div()
                .id(("settings-page", index))
                .debug_selector(move || format!("settings-page-{}", page.title))
                .flex()
                .items_center()
                .h(unit(size::ROW))
                .px(unit(space::M))
                .rounded(theme::radius_control())
                .cursor_pointer()
                .text_color(if active {
                    theme::text()
                } else {
                    theme::muted()
                })
                .when(active, |this| {
                    this.bg(theme::selected()).font_weight(FontWeight::MEDIUM)
                })
                .when(!active, |this| {
                    this.hover(|style| style.bg(theme::hover()).text_color(theme::text()))
                })
                .child(page.title)
                .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                    if let Some(state) = &mut app.settings {
                        state.page = index;
                        state.row = 0;
                        cx.notify();
                    }
                }))
        });
        let nav: Vec<_> = nav.collect();
        let page = &PAGES[state.page];
        let mut position = 0;
        let groups: Vec<_> = page
            .groups
            .iter()
            .map(|group| {
                let last_setting = group.settings.len() - 1;
                let settings: Vec<_> = group
                    .settings
                    .iter()
                    .enumerate()
                    .map(|(at, &setting)| {
                        let index = position;
                        position += 1;
                        self.setting_row(setting, index, state.row, at < last_setting, cx)
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
                    .children(settings)
            })
            .collect();
        let hint = |keys: &[&'static str], what: &'static str| {
            div()
                .flex()
                .items_center()
                .gap(unit(space::XS))
                .children(keys.iter().map(|key| ui::kbd(commands::label(key))))
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
            .child(hint(&["up", "down"], "move"))
            .child(hint(&["tab"], "section"))
            .child(hint(&["space"], "change"))
            .child(hint(&["escape"], "close"));
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
            .font_family(self.font.clone())
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
                    .px(unit(space::XXL))
                    .py(unit(space::XXL))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(unit(space::XXL))
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
                        app.settings = None;
                        cx.notify();
                    }),
                )
                .child(panel)
                .into_any_element(),
        )
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
            .py(unit(space::L))
            .rounded(theme::radius_control())
            .when(rule, |this| this.border_b_1().border_color(theme::border()))
            .when(focused, |this| this.bg(theme::selected()));
        match setting.choices() {
            Some(choices) => {
                row.flex_col()
                    .child(label)
                    .child(div().flex().gap(unit(space::L)).children(
                        choices.iter().enumerate().map(|(choice, name)| {
                            theme_card(setting, choice, name, choice == value, cx)
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
                    if let Some(state) = &mut app.settings {
                        state.row = index;
                    }
                    app.change(setting, setting.next(&app.config), cx);
                })),
        }
    }
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

fn switch_page(state: &mut Settings, step: usize) {
    state.page = (state.page + step) % PAGES.len();
    state.row = 0;
}
