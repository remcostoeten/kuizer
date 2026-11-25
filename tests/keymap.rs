use std::path::Path;

use kuizer::config::Config;
use kuizer::keymap::{Action, Keymap, Modifiers, Scope, Stroke};

fn stroke(key: &str) -> Stroke<'_> {
    Stroke {
        key,
        typed: None,
        modifiers: Modifiers::default(),
    }
}

#[test]
fn a_shared_override_replaces_the_defaults_in_both_frontends() {
    let (config, warnings) = Config::parse_lenient("[keys]\n\"nav.down\" = [\"n\", \"down\"]");
    assert!(warnings.is_empty());
    let (keymap, warnings) = Keymap::resolve(&config.keys);
    assert!(warnings.is_empty());
    for scope in [Scope::Gui, Scope::Tui] {
        assert_eq!(keymap.lookup(scope, stroke("n")), Some(Action::Down));
        assert_eq!(keymap.lookup(scope, stroke("down")), Some(Action::Down));
        assert_eq!(keymap.lookup(scope, stroke("j")), None);
    }
}

#[test]
fn malformed_and_conflicting_overrides_keep_defaults_and_report_the_action() {
    let text = r#"
[keys]
"nav.down" = "k"
"nav.up" = ["n", 2]
"nav.bottom" = []
"nav.back" = "r"
"palette.open" = "ctrl-v"
"control.next" = "j"
"select.toggle" = "t"
"#;
    let (config, warnings) = Config::parse_lenient(text);
    for action in [
        "nav.down",
        "nav.up",
        "nav.bottom",
        "nav.back",
        "palette.open",
        "control.next",
    ] {
        assert!(
            warnings
                .iter()
                .any(|warning| warning.key == format!("keys.{action}"))
        );
    }
    assert!(warnings.iter().all(|warning| !warning.widens_scope));
    assert!(Config::parse(text, Path::new("config.toml")).is_err());
    let (keymap, errors) = Keymap::resolve(&config.keys);
    assert!(errors.is_empty());
    for scope in [Scope::Gui, Scope::Tui] {
        assert_eq!(keymap.lookup(scope, stroke("j")), Some(Action::Down));
        assert_eq!(keymap.lookup(scope, stroke("k")), Some(Action::Up));
        assert_eq!(keymap.lookup(scope, stroke("t")), Some(Action::Toggle));
    }
}

#[test]
fn shifted_symbols_match_the_platform_event_in_their_own_context() {
    let keymap = Keymap::default();
    assert_eq!(
        keymap.lookup(Scope::Gui, stroke("@")),
        Some(Action::Artifacts)
    );
    assert_eq!(
        keymap.lookup(
            Scope::Gui,
            Stroke {
                key: "/",
                typed: Some("?"),
                modifiers: Modifiers {
                    shift: true,
                    ..Modifiers::default()
                },
            }
        ),
        Some(Action::Help)
    );
    assert_eq!(
        keymap.lookup(Scope::Map, stroke("enter")),
        Some(Action::MapIn)
    );
    assert_eq!(
        keymap.lookup(Scope::Gui, stroke("enter")),
        Some(Action::Delete)
    );
}
