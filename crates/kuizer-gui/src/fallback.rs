use std::sync::OnceLock;

use gpui::{AnyElement, Div, FontWeight, Pixels, SharedString, div, prelude::*};
use kuizer::config::ConfigWarning;

use crate::theme::{self, font, space, unit};

pub struct Problem {
    reason: String,
    font: SharedString,
}

static PROBLEM: OnceLock<Problem> = OnceLock::new();

/// @name summary
/// @description Shortens a config error to the lines that matter in a banner: the line naming the
/// file and the position, and the line saying what is wrong. The quoted source lines in between
/// are dropped.
///
/// @example
/// let line = summary("invalid config /c.toml: TOML parse error at line 2\n  |\n2 | mde = 1\n  | ^^^\nunknown field `mde`\n");
/// assert!(line.ends_with("unknown field `mde`"));
pub fn summary(error: &str) -> String {
    let mut lines = error.lines().map(str::trim).filter(|line| !line.is_empty());
    let Some(first) = lines.next() else {
        return String::new();
    };
    match lines.next_back() {
        Some(last) => format!("{first}: {last}"),
        None => first.to_owned(),
    }
}

/// @name notice
/// @description The banner line for the settings the config loader dropped: the first one and
/// how many more there are, or `None` when every setting loaded.
///
/// @example
/// let line = notice(&loaded.warnings);
pub fn notice(warnings: &[ConfigWarning]) -> Option<String> {
    let first = warnings.first()?;
    Some(match warnings.len() - 1 {
        0 => first.to_string(),
        more => format!("{first} (and {more} more)"),
    })
}

/// @name remember
/// @description Keeps the config problem found at start, so the window shows it in a banner for
/// as long as kuizer runs with defaults in place of the bad settings.
///
/// @example
/// remember(summary(&error.to_string()), font.clone());
pub fn remember(reason: String, font: SharedString) {
    PROBLEM.set(Problem { reason, font }).ok();
}

/// @name current
/// @description The remembered config problem, when kuizer started with defaults in place of
/// some or all settings.
///
/// @example
/// let banner = wrap(content, current(), None);
pub fn current() -> Option<&'static Problem> {
    PROBLEM.get()
}

/// @name wrap
/// @description Puts the config banner above `content` when there is a problem to show, and
/// returns `content` untouched otherwise. `inset` keeps the banner text clear of the macOS
/// traffic lights.
///
/// @example
/// let root = wrap(content, current(), inset(window));
pub fn wrap(
    content: impl IntoElement,
    problem: Option<&Problem>,
    inset: Option<Pixels>,
) -> AnyElement {
    let Some(problem) = problem else {
        return content.into_any_element();
    };
    div()
        .flex()
        .flex_col()
        .size_full()
        .child(banner(problem, inset))
        .child(div().flex().flex_col().flex_1().min_h_0().child(content))
        .into_any_element()
}

fn banner(problem: &Problem, inset: Option<Pixels>) -> Div {
    div()
        .debug_selector(|| "config-problem".to_owned())
        .flex()
        .flex_none()
        .items_start()
        .gap(unit(space::L))
        .px(unit(space::GUTTER))
        .py(unit(space::S))
        .when_some(inset, |this, inset| this.pl(inset))
        .border_b_1()
        .border_color(theme::review())
        .bg(theme::elevated())
        .font_family(problem.font.clone())
        .text_size(unit(font::SMALL))
        .text_color(theme::text())
        .child(
            div()
                .flex_none()
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme::review())
                .child("Config problem, defaults in use"),
        )
        .child(div().flex_1().min_w_0().child(problem.reason.clone()))
}

#[cfg(test)]
mod tests {
    use gpui::{Context, IntoElement, Render, TestAppContext, Window};

    use super::*;

    struct Probe(Problem);

    impl Render for Probe {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            wrap(
                div().size_full().debug_selector(|| "content".to_owned()),
                Some(&self.0),
                None,
            )
        }
    }

    #[test]
    fn a_toml_error_keeps_its_first_and_last_line() {
        let error = "invalid config /c.toml: TOML parse error at line 2, column 1\n  |\n2 | mde = 1\n  | ^^^\nunknown field `mde`\n";
        assert_eq!(
            summary(error),
            "invalid config /c.toml: TOML parse error at line 2, column 1: unknown field `mde`"
        );
        assert_eq!(summary("unknown rule `x`"), "unknown rule `x`");
    }

    #[gpui::test]
    fn the_banner_shows_above_the_content(cx: &mut TestAppContext) {
        let (_view, cx) = cx.add_window_view(|_, _| {
            Probe(Problem {
                reason: "bad key".to_owned(),
                font: ".SystemUIFont".into(),
            })
        });
        cx.run_until_parked();
        let banner = cx
            .debug_bounds("config-problem")
            .expect("banner is painted");
        let content = cx.debug_bounds("content").expect("content is painted");
        assert!(content.origin.y >= banner.bottom());
    }
}
