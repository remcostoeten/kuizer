---
name: gpui
description: Use when writing, changing or testing kuizer-gui code with GPUI 0.2.2: views, entities, elements, layout and styling, events, focus and keys, async tasks, tooltips, and #[gpui::test] UI tests.
---

# GPUI 0.2.2 in kuizer-gui

kuizer-gui pins `gpui = "=0.2.2"` from crates.io. The references in this skill
are adapted from longbridge/gpui-kit, which tracks newer GPUI snapshots, so they
explain the mechanics well but can name APIs that 0.2.2 does not have.

## Rules

- **Never invent an API.** Before using a method, find it in
  `~/.cargo/registry/src/*/gpui-0.2.2/src` (in a cloud container, run
  `cargo fetch` first). If it is not there, it does not exist for us.
- `use gpui::*` / `gpui::prelude::*`. Never `gpui_kit` or `gpui-component`;
  kuizer does not depend on them.
- Colors, spacing, radii and sizes come from `crates/kuizer-gui/src/theme.rs`.
  No hardcoded values in views.
- Logic lives in the `kuizer` core crate. The GUI renders state and dispatches
  actions; it never walks the filesystem or deletes files itself.
- Repeated elements need stable ids derived from the data
  (`(ElementId::from("recent"), path.to_string_lossy().to_string())`), not list
  positions, whenever the list can reorder. 0.2.2 has no `From<String>` or
  `From<(&str, PathBuf)>` for `ElementId`.
- Call `cx.notify()` after any state change that affects `render`.
- Background work: `cx.spawn(async move |this, cx| ...)` and keep the `Task`
  in a field, or `.detach()` it deliberately. Use
  `cx.background_executor().timer(..)` for delays so tests can drive them.
- Build with `--release` on Remco's machine (debug GPUI builds are huge and his
  disk is nearly full). Never drive his desktop with xdotool; test interaction
  headless with `#[gpui::test]` instead.

## Tooltips

Plain GPUI has tooltips on stateful elements: give the element an `.id(..)`,
then `.tooltip(|window, cx| cx.new(|_| MyTooltip { .. }).into())` where the
tooltip is a small view implementing `Render`. `hoverable_tooltip` keeps it open
while hovered. Check both signatures in `gpui-0.2.2/src/elements/div.rs`. One
reusable tooltip view, styled from `theme.rs`, beats per-call styling.

## Testing

- `#[gpui::test] fn name(cx: &mut TestAppContext)` and
  `cx.add_window_view(..)` for a `VisualTestContext`. kuizer-gui's tests in
  `src/app.rs` show the pattern: open, `simulate_keystrokes("ctrl-backspace")`,
  `run_until_parked()`, assert on the entity's state.
- Test our behavior, not GPUI's. Assert outcomes through the view's state.
- For flaky or seed-dependent failures, read
  [test-debugging.md](references/test-debugging.md) (from Zed).

## References

Load the file for the mechanism the task touches. Code samples use `gpui::`;
verify each API against the 0.2.2 source before relying on it.

| Topic | File | Load when |
| --- | --- | --- |
| Actions and keybindings | [action.md](references/action.md) | `actions!`, `bind_keys`, `on_action`, `key_context` |
| Async and background tasks | [async.md](references/async.md) | `cx.spawn`, `background_spawn`, `Task` |
| Contexts | [context.md](references/context.md) | `App`, `Window`, `Context<T>`, `AsyncApp` |
| Custom elements | [element.md](references/element.md) | `Element` trait, layout, prepaint, paint |
| Element ids | [element-id.md](references/element-id.md) | `.id()`, stateful elements, uniqueness |
| Entities | [entity.md](references/entity.md) | `Entity<T>`, `WeakEntity`, updates |
| Events and subscriptions | [event.md](references/event.md) | `cx.emit`, `cx.subscribe`, `cx.observe` |
| Focus and keyboard | [focus-handle.md](references/focus-handle.md) | `FocusHandle`, `track_focus`, tab order |
| Global state | [global.md](references/global.md) | `Global`, `cx.set_global` |
| Layout and styling | [layout-style.md](references/layout-style.md) | `div()`, flex, overflow, positioning |
| Tests | [test.md](references/test.md) | `TestAppContext`, `VisualTestContext` |
| Test debugging | [test-debugging.md](references/test-debugging.md) | seeds, `ITERATIONS`, "Parking forbidden" |

Sources: references adapted from [longbridge/gpui-kit](https://github.com/longbridge/gpui-kit)
`skills/gpui-kit/references/gpui/` (Apache-2.0, see `NOTICE.md`), and
`test-debugging.md` from [zed-industries/zed](https://github.com/zed-industries/zed)
`.agents/skills/gpui-test` (Apache-2.0).
