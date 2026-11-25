mod activity;
mod app;
mod chart;
mod commands;
mod fallback;
mod field;
mod frame;
mod glossary;
mod icon;
mod model;
mod motion;
mod palette;
mod persist;
mod reveal;
mod snapshot;
mod squarify;
mod start;
mod stats;
mod tab;
mod theme;
mod tooltip;
mod treemap;
mod ui;
mod zoom;

use std::borrow::Cow;
use std::path::PathBuf;
use std::process::ExitCode;

use gpui::{
    App, Application, Bounds, SharedString, WindowBounds, WindowOptions, prelude::*, px, size,
};
use kuizer::config::Config;

use crate::app::Kuizer;
use crate::zoom::Zoom;

const BUNDLED_FONTS: [&[u8]; 3] = [
    include_bytes!("../assets/fonts/SchibstedGrotesk-Regular.ttf"),
    include_bytes!("../assets/fonts/SchibstedGrotesk-Medium.ttf"),
    include_bytes!("../assets/fonts/SchibstedGrotesk-SemiBold.ttf"),
];

fn register_fonts(cx: &App) {
    let fonts = BUNDLED_FONTS.into_iter().map(Cow::Borrowed).collect();
    if let Err(error) = cx.text_system().add_fonts(fonts) {
        eprintln!("kuizer-gui: cannot load the bundled font: {error}");
    }
}

fn font(cx: &App) -> SharedString {
    let installed = cx.text_system().all_font_names();
    let first = |candidates: &[&'static str]| {
        candidates
            .iter()
            .find(|candidate| installed.iter().any(|name| name == *candidate))
            .map(|name| SharedString::from(*name))
    };
    if let Some(mono) = first(&theme::MONO_CANDIDATES) {
        theme::set_mono(mono);
    }
    first(&theme::FONT_CANDIDATES).unwrap_or_else(|| SharedString::from(".SystemUIFont"))
}

const NO_WINDOW_HINT: &str = "kuizer-gui needs a graphical session with a working GPU driver (Vulkan on Linux, Metal on macOS). Install the Vulkan driver for your GPU, or run `kuizer` in a terminal for the same scan and cleanup in the terminal UI.";

// GPUI panics instead of returning an error when it finds no display server or usable GPU.
fn start_platform() -> Application {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|info| {
        let payload = info.payload();
        let reason = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&str>().copied())
            .unwrap_or("unknown error");
        let reason = reason.lines().next().unwrap_or(reason);
        let reason = reason
            .strip_prefix("called `Result::unwrap()` on an `Err` value: ")
            .unwrap_or(reason);
        eprintln!("kuizer-gui: cannot start the graphics backend: {reason}");
        eprintln!("{NO_WINDOW_HINT}");
        std::process::exit(1);
    }));
    let application = Application::new();
    std::panic::set_hook(previous);
    application
}

fn main() -> ExitCode {
    let (config, problem) = match Config::load(None) {
        Ok(loaded) => {
            for warning in &loaded.warnings {
                eprintln!("kuizer-gui: config: {warning}");
            }
            (loaded.config, fallback::notice(&loaded.warnings))
        }
        Err(error) => {
            eprintln!("kuizer-gui: {error}");
            (
                Config::default(),
                Some(fallback::summary(&error.to_string())),
            )
        }
    };
    let paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let roots = kuizer::scan::normalize_roots(&paths);

    start_platform()
        .with_assets(icon::Assets)
        .run(move |cx: &mut App| {
            let bounds = Bounds::centered(None, size(px(1280.), px(820.)), cx);
            register_fonts(cx);
            let font = font(cx);
            if let Some(reason) = problem {
                fallback::remember(reason, font.clone());
            }
            commands::bind_configured_keys(cx, &config.keys);
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
                            snapshot::Snapshots::for_user(),
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
                eprintln!("{NO_WINDOW_HINT}");
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
