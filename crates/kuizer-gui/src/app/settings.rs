//! The settings popover: pages of switches that read and write the config file.

use gpui::Keystroke;
use kuizer::config::{Config, SettingValue, Sidebar};

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Setting {
    Motion,
    SidebarCollapsed,
    RoutesEnabled,
    RouteHints,
}

impl Setting {
    fn label(self) -> &'static str {
        match self {
            Self::Motion => "Motion",
            Self::SidebarCollapsed => "Open the sidebar collapsed",
            Self::RoutesEnabled => "Section shortcuts",
            Self::RouteHints => "Show shortcut hints",
        }
    }

    fn hint(self) -> &'static str {
        match self {
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

    fn on(self, config: &Config) -> bool {
        match self {
            Self::Motion => config.appearance.motion,
            Self::SidebarCollapsed => config.appearance.sidebar == Sidebar::Collapsed,
            Self::RoutesEnabled => config.route_shortcuts.enabled,
            Self::RouteHints => config.route_shortcuts.hints,
        }
    }

    fn set(self, config: &mut Config, on: bool) {
        match self {
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

    fn entry(self, on: bool) -> (&'static str, &'static str, SettingValue) {
        match self {
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
    settings: &'static [Setting],
}

struct Page {
    title: &'static str,
    groups: &'static [Group],
}

const PAGES: [Page; 2] = [
    Page {
        title: "General",
        groups: &[Group {
            title: "Appearance",
            settings: &[Setting::Motion, Setting::SidebarCollapsed],
        }],
    },
    Page {
        title: "Route shortcuts",
        groups: &[Group {
            title: "Sidebar sections",
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
            "right" | "l" => switch_page(state, 1),
            "left" | "h" => switch_page(state, PAGES.len() - 1),
            "tab" if shift => switch_page(state, PAGES.len() - 1),
            "tab" => switch_page(state, 1),
            "space" | "enter" => {
                let setting = rows(state.page)[state.row];
                self.flip(setting, cx);
                return;
            }
            _ => return,
        }
        cx.notify();
    }

    fn flip(&mut self, setting: Setting, cx: &mut Context<Self>) {
        let Some(file) = self.config_path.clone() else {
            self.tell(
                Notice::Failed(
                    "Could not save the setting: this system has no config folder".into(),
                ),
                cx,
            );
            return;
        };
        let was = setting.on(&self.config);
        setting.set(&mut self.config, !was);
        let (table, key, value) = setting.entry(!was);
        let written = cx.background_spawn(async move {
            let _one_writer = CONFIG_WRITE
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            kuizer::config::set_setting(&file, table, key, value).map_err(|error| error.to_string())
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
        was: bool,
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
        let mut position = 0;
        let groups = PAGES[state.page].groups.iter().map(|group| {
            let settings = group.settings.iter().map(|&setting| {
                let index = position;
                position += 1;
                let on = setting.on(&self.config);
                let focused = index == state.row;
                div()
                    .id(("setting", index))
                    .debug_selector(move || format!("setting-{}", setting.label()))
                    .flex()
                    .items_center()
                    .gap(unit(space::XL))
                    .px(unit(space::M))
                    .py(unit(space::M))
                    .rounded(theme::radius_control())
                    .cursor_pointer()
                    .when(focused, |this| this.bg(theme::selected()))
                    .when(!focused, |this| {
                        this.hover(|style| style.bg(theme::hover()))
                    })
                    .child(
                        div()
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
                            ),
                    )
                    .child(ui::switch(on))
                    .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                        if let Some(state) = &mut app.settings {
                            state.row = index;
                        }
                        app.flip(setting, cx);
                    }))
            });
            div()
                .flex()
                .flex_col()
                .gap(unit(space::XS))
                .child(div().px(unit(space::M)).child(ui::heading(group.title)))
                .children(settings)
        });
        let footer = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(unit(space::S))
            .text_size(unit(font::CAPTION))
            .text_color(theme::faint())
            .child(ui::kbd(commands::label("up")))
            .child(ui::kbd(commands::label("down")))
            .child(div().pr(unit(space::M)).child("move"))
            .child(ui::kbd(commands::label("tab")))
            .child(div().pr(unit(space::M)).child("page"))
            .child(ui::kbd(commands::label("space")))
            .child(div().pr(unit(space::M)).child("switch"))
            .child(ui::kbd(commands::label("escape")))
            .child("close");
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
                    .p(unit(space::M))
                    .bg(theme::panel())
                    .border_r_1()
                    .border_color(theme::border())
                    .child(
                        div()
                            .px(unit(space::M))
                            .pb(unit(space::S))
                            .child(ui::heading("Settings")),
                    )
                    .children(nav),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap(unit(space::XL))
                    .p(unit(space::XL))
                    .child(
                        div()
                            .text_size(unit(font::HEADING))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(PAGES[state.page].title),
                    )
                    .child(
                        div()
                            .id("settings-rows")
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h_0()
                            .gap(unit(space::XL))
                            .overflow_y_scroll()
                            .children(groups),
                    )
                    .child(footer),
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
}

fn switch_page(state: &mut Settings, step: usize) {
    state.page = (state.page + step) % PAGES.len();
    state.row = 0;
}
