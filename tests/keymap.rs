use std::path::Path;

use kuizer::config::Config;
use kuizer::keymap::{
    Action, Keymap, Modifiers, Override, Overrides, Scope, Stroke, binding_text, rebind,
};

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

#[test]
fn a_rebind_is_refused_when_it_clashes_and_kept_when_only_old_problems_remain() {
    let one = |key: &str| Some(Override::One(key.to_owned()));
    let clash = rebind(&Overrides::new(), "nav.up", one("j")).expect_err("j moves down");
    assert_eq!(clash.name, "nav.up");
    assert!(clash.message.contains("nav.down"), "{}", clash.message);
    assert!(rebind(&Overrides::new(), "palette.open", one("p")).is_err());
    assert!(rebind(&Overrides::new(), "nav.sideways", one("n")).is_err());

    let mut broken = Overrides::new();
    broken.insert("nav.top".to_owned(), Override::One("ctrl-".to_owned()));
    let moved = rebind(&broken, "nav.down", one("n")).expect("n is free");
    let swapped = rebind(&moved, "nav.up", one("j")).expect("j is free once down moved");
    let back = rebind(&swapped, "nav.down", None).expect_err("down's j is taken by up");
    assert_eq!(back.name, "nav.up");
    assert_eq!(rebind(&swapped, "nav.up", None).ok(), Some(moved));
}

#[test]
fn a_recorded_key_answers_the_stroke_that_was_pressed() {
    let held = |ctrl: bool, shift: bool| Modifiers {
        ctrl,
        shift,
        ..Modifiers::default()
    };
    let strokes = [
        ("/", Some("?"), held(false, true), "?"),
        ("o", Some("O"), held(false, true), "O"),
        ("h", None, held(true, true), "ctrl-shift-h"),
        ("f5", None, held(false, false), "f5"),
    ];
    for (key, typed, modifiers, text) in strokes {
        let stroke = Stroke {
            key,
            typed,
            modifiers,
        };
        assert_eq!(binding_text(stroke).as_deref(), Some(text));
        let overrides = Overrides::from([("help.open".to_owned(), Override::One(text.to_owned()))]);
        let (keymap, problems) = Keymap::resolve(&overrides);
        assert!(problems.is_empty(), "{text}");
        assert!(keymap.matches(Scope::Gui, Action::Help, stroke), "{text}");
    }
    let unknown = Stroke {
        key: "printscreen",
        typed: None,
        modifiers: Modifiers::default(),
    };
    assert_eq!(binding_text(unknown), None);
}
