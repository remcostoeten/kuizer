use gpui::{CursorStyle, Div, ElementId, Keystroke, SharedString, Stateful, div, prelude::*};

use crate::theme::{self, size, space, unit};

/// Separators Ctrl+Backspace stops at in a path field.
pub const PATH: &[char] = &['/'];
/// Separators Ctrl+Backspace stops at in a search field.
pub const WORDS: &[char] = &['/', ' ', '-', '_', '.'];

/// @name edit
/// @description Applies one key to a single-line text field kept as a `String`: typed
/// characters, Backspace, Ctrl+Backspace (back to the previous separator), Ctrl+U (clear) and
/// Ctrl+V (paste the first line). Returns false when the key is not a text edit, so the caller
/// can use it for something else. GPUI 0.2.2 has no text input element, so every field in the
/// app goes through this.
///
/// @example
/// if field::edit(&mut query, keystroke, field::WORDS, || clipboard()) { refilter(); }
pub fn edit(
    text: &mut String,
    keystroke: &Keystroke,
    separators: &[char],
    paste: impl FnOnce() -> Option<String>,
) -> bool {
    let command = keystroke.modifiers.control || keystroke.modifiers.platform;
    match keystroke.key.as_str() {
        "backspace" if command || keystroke.modifiers.alt => delete_word(text, separators),
        "backspace" => {
            text.pop();
        }
        "v" if command => {
            if let Some(pasted) = paste() {
                text.push_str(pasted.lines().next().unwrap_or_default().trim());
            }
        }
        "u" if command => text.clear(),
        _ => {
            let Some(typed) = keystroke.key_char.as_deref() else {
                return false;
            };
            if command || typed.chars().any(char::is_control) {
                return false;
            }
            text.push_str(typed);
        }
    }
    true
}

fn delete_word(text: &mut String, separators: &[char]) {
    let trimmed = text.trim_end_matches(separators);
    let keep = trimmed
        .rfind(|ch: char| separators.contains(&ch))
        .map_or(0, |index| index + 1);
    text.truncate(keep);
}

fn caret() -> Div {
    div()
        .flex_none()
        .w(unit(size::HAIR))
        .h(unit(size::CARET))
        .bg(theme::text())
}

/// @name view
/// @description Draws a one-line field: the text with a caret after it while it has the
/// keyboard, or the placeholder when it is empty.
///
/// @example
/// field::view("filter-field", &filter, "Filter by path", active)
pub fn view(
    id: impl Into<ElementId>,
    text: &str,
    placeholder: impl Into<SharedString>,
    active: bool,
) -> Stateful<Div> {
    let empty = text.is_empty();
    div()
        .id(id)
        .flex()
        .items_center()
        .min_w_0()
        .overflow_hidden()
        .h(unit(size::CONTROL))
        .px(unit(space::M))
        .rounded(theme::radius_control())
        .bg(theme::background())
        .border_1()
        .border_color(if active {
            theme::focus()
        } else {
            theme::border_strong()
        })
        .cursor(CursorStyle::IBeam)
        .when(empty && active, |this| this.child(caret()))
        .when(empty, |this| {
            this.child(
                div()
                    .pl(unit(space::XXS))
                    .truncate()
                    .text_color(theme::faint())
                    .child(placeholder.into()),
            )
        })
        .when(!empty, |this| {
            this.child(div().flex_none().child(text.to_owned()))
                .when(active, |this| this.child(caret()))
        })
}

#[cfg(test)]
mod tests {
    use gpui::Modifiers;

    use super::*;

    fn key(source: &str) -> Keystroke {
        Keystroke::parse(source).unwrap().with_simulated_ime()
    }

    #[test]
    fn typing_backspace_and_clear() {
        let mut text = String::new();
        for typed in ["a", "shift-b", "space", "c"] {
            assert!(edit(&mut text, &key(typed), WORDS, || None));
        }
        assert_eq!(text, "aB c");
        assert!(edit(&mut text, &key("backspace"), WORDS, || None));
        assert_eq!(text, "aB ");
        assert!(edit(&mut text, &key("ctrl-u"), WORDS, || None));
        assert!(text.is_empty());
    }

    #[test]
    fn keys_that_are_not_text_are_left_alone() {
        let mut text = "x".to_owned();
        for other in ["down", "escape", "ctrl-k", "ctrl-a", "f10"] {
            assert!(!edit(&mut text, &key(other), WORDS, || None), "{other}");
        }
        let shortcut = Keystroke {
            modifiers: Modifiers::control(),
            key: "j".to_owned(),
            key_char: Some("j".to_owned()),
        };
        assert!(!edit(&mut text, &shortcut, WORDS, || None));
        assert_eq!(text, "x");
    }

    #[test]
    fn ctrl_backspace_stops_at_a_separator() {
        let mut path = "/home/me/apps".to_owned();
        edit(&mut path, &key("ctrl-backspace"), PATH, || None);
        assert_eq!(path, "/home/me/");
        edit(&mut path, &key("ctrl-backspace"), PATH, || None);
        assert_eq!(path, "/home/");
        let mut words = "node modules".to_owned();
        edit(&mut words, &key("ctrl-backspace"), WORDS, || None);
        assert_eq!(words, "node ");
    }

    #[test]
    fn paste_takes_the_first_line() {
        let mut text = "~/".to_owned();
        edit(&mut text, &key("ctrl-v"), PATH, || {
            Some(" apps/site \nmore".to_owned())
        });
        assert_eq!(text, "~/apps/site");
    }
}
