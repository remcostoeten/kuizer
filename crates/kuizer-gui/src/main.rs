mod app;
mod chart;
mod frame;
mod glossary;
mod model;
mod motion;
mod snapshot;
mod start;
mod theme;
mod tooltip;
mod ui;
mod zoom;

use std::path::PathBuf;
use std::process::ExitCode;

use gpui::{
    App, Application, Bounds, SharedString, WindowBounds, WindowOptions, prelude::*, px, size,
};
use kuizer::config::Config;

use crate::app::Kuizer;
use crate::zoom::Zoom;

fn font(cx: &App) -> SharedString {
    let installed = cx.text_system().all_font_names();
    theme::FONT_CANDIDATES
        .iter()
        .find(|candidate| installed.iter().any(|name| name == *candidate))
        .map_or_else(
            || SharedString::from(".SystemUIFont"),
            |name| (*name).into(),
        )
}

fn main() -> ExitCode {
    let loaded = match Config::load(None) {
        Ok(loaded) => loaded,
        Err(error) => {
            eprintln!("kuizer-gui: {error}");
            return ExitCode::FAILURE;
        }
    };
    let paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let roots = kuizer::scan::normalize_roots(&paths);
    let config = loaded.config;

    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1280.), px(820.)), cx);
        let font = font(cx);
        app::bind_keys(cx);
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(frame::titlebar()),
                window_decorations: frame::decorations(),
                app_id: Some("kuizer-gui".to_owned()),
                window_min_size: Some(size(px(860.), px(520.))),
                ..WindowOptions::default()
            },
            |window, cx| {
                cx.new(|cx| {
                    Kuizer::new(
                        config,
                        roots,
                        start::Recent::for_user(),
                        Zoom::for_user(),
                        font,
                        window,
                        cx,
                    )
                })
            },
        );
        if let Err(error) = opened {
            eprintln!("kuizer-gui: cannot open a window: {error}");
            cx.quit();
            return;
        }
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
    ExitCode::SUCCESS
}
