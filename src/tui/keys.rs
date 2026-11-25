use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::keymap::{Action, Keymap, Modifiers, Scope, Stroke};

pub(super) fn modifiers(event: KeyEvent) -> Modifiers {
    Modifiers {
        ctrl: event.modifiers.contains(KeyModifiers::CONTROL),
        alt: event.modifiers.contains(KeyModifiers::ALT),
        shift: event.modifiers.contains(KeyModifiers::SHIFT),
        cmd: event.modifiers.contains(KeyModifiers::SUPER),
        function: false,
    }
}

pub(super) fn action(keys: &Keymap, scope: Scope, event: KeyEvent) -> Option<Action> {
    let mut modifiers = modifiers(event);
    let mut buffer = [0; 4];
    let key = match event.code {
        KeyCode::Char(' ') => "space",
        KeyCode::Char(key) => {
            modifiers.shift |= key.is_ascii_uppercase();
            key.to_ascii_lowercase().encode_utf8(&mut buffer)
        }
        KeyCode::Esc => "escape",
        KeyCode::Enter => "enter",
        KeyCode::Backspace => "backspace",
        KeyCode::Delete => "delete",
        KeyCode::Insert => "insert",
        KeyCode::Tab => "tab",
        KeyCode::BackTab => {
            modifiers.shift = true;
            "tab"
        }
        KeyCode::Up => "up",
        KeyCode::Down => "down",
        KeyCode::Left => "left",
        KeyCode::Right => "right",
        KeyCode::Home => "home",
        KeyCode::End => "end",
        KeyCode::PageUp => "pageup",
        KeyCode::PageDown => "pagedown",
        KeyCode::F(n @ 1..=24) => {
            buffer[0] = b'f';
            let len = if n < 10 {
                buffer[1] = b'0' + n;
                2
            } else {
                buffer[1] = b'0' + n / 10;
                buffer[2] = b'0' + n % 10;
                3
            };
            std::str::from_utf8(&buffer[..len]).ok()?
        }
        _ => return None,
    };
    keys.lookup(
        scope,
        Stroke {
            key,
            typed: None,
            modifiers,
        },
    )
}
