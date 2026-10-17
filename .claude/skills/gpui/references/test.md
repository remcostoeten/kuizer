# Testing

**Contents:** [Choose the test level](#choose-the-test-level) · [UI integration workflow](#ui-integration-workflow) · [Complete example](#complete-example) · [Queries and interactions](#queries-and-interactions) · [Frames and async work](#frames-and-async-work) · [Resize and clipboard](#resize-and-clipboard) · [Assertion boundaries](#assertion-boundaries) · [Additional resources](#additional-resources)

> Adapted for GPUI 0.2.2. The upstream gpui-kit version of this page documents
> a `gpui_kit::test` query layer (`TestWindowExt`, `window.find`, `click`,
> `press`, `TestInput`, `wait_for`) that does not exist in GPUI 0.2.2. This page
> uses only the plain GPUI test API from `gpui-0.2.2/src/app/test_context.rs`.

## UI integration testing

A UI integration test renders real views in a headless window, dispatches
keystrokes, text input and mouse events, then checks the view's state, focus
and layout. `#[gpui::test]` runs the test; `TestAppContext` and
`VisualTestContext` drive it.

## Choose the test level

Use ordinary Rust `#[test]` for pure logic. Use `#[gpui::test]` and
`TestAppContext` for entities, subscriptions, actions and async tasks. For a UI
flow, open a window with `cx.add_window_view(..)`, which returns the root view
and a `VisualTestContext` bound to that window, and drive it through the real
key and mouse paths.

Enable the `test-support` feature in the dev-dependency, with the same version
as the normal dependency. kuizer-gui already does this:

```toml
[dependencies]
gpui = "=0.2.2"

[dev-dependencies]
gpui = { version = "=0.2.2", features = ["test-support"] }
```

Avoid `use gpui::*;` in test modules: it imports GPUI's `test` macro and can
shadow Rust's built-in `#[test]`. Import the types you use explicitly.

## UI integration workflow

1. Build the production view in the test. Keep state entities and
   subscriptions owned by that view. Define a view inside a test only for a
   self-contained example or a deliberate fixture.
2. Open it with `let (view, cx) = cx.add_window_view(|window, cx| ..)`. The
   returned `cx` shadows the original and is a `&mut VisualTestContext`. The
   window is maximized on the test platform's display, and the context has
   already run until parked.
3. Dispatch input through the window: `cx.simulate_keystrokes("ctrl-backspace")`
   for bound keys, `cx.simulate_input("text")` for typed text,
   `cx.simulate_click(point, Modifiers::none())` and the
   `simulate_mouse_*` helpers for pointer events, or
   `cx.dispatch_action(MyAction)` to fire an action without its binding.
   Exercise the actual handler path; calling handlers directly or setting
   state by hand bypasses the behavior being tested.
4. Assert the initial state, the outcome after each meaningful action, and the
   application result, through `view.read_with(cx, |view, cx| ..)`. Include a
   relevant negative case, such as invalid input leaving state unchanged.
5. Run the affected test target and report its result. An uncompiled example or
   a test which never checks the action's outcome is not completion evidence.

## Complete example

```rust
use gpui::{
    Context, FocusHandle, InteractiveElement, IntoElement, KeyBinding, ParentElement, Render,
    TestAppContext, Window, actions, div,
};

actions!(counter, [Increment, Save]);

struct Counter {
    count: usize,
    saved: Option<usize>,
    focus: FocusHandle,
}

impl Render for Counter {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("Counter")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &Increment, _, cx| {
                this.count += 1;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Save, _, cx| {
                this.saved = Some(this.count);
                cx.notify();
            }))
            .child(self.count.to_string())
    }
}

#[gpui::test]
fn saves_the_count_through_the_keyboard(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.bind_keys([
            KeyBinding::new("up", Increment, Some("Counter")),
            KeyBinding::new("enter", Save, Some("Counter")),
        ])
    });
    let (view, cx) = cx.add_window_view(|window, cx| {
        let focus = cx.focus_handle();
        window.focus(&focus);
        Counter { count: 0, saved: None, focus }
    });

    view.read_with(cx, |counter, _| assert!(counter.saved.is_none()));

    cx.simulate_keystrokes("up up enter");

    view.read_with(cx, |counter, _| assert_eq!(counter.saved, Some(2)));
}
```

kuizer-gui's own tests in `crates/kuizer-gui/src/app.rs` follow this pattern
with the real app view: open, type, `simulate_keystrokes`, then assert on the
view's phase and folder fields.

## Queries and interactions

GPUI 0.2.2 has no element query layer: there is no `find(id)`, label lookup or
accessibility snapshot. Assert through the view's state instead, and use
`debug_selector` when geometry matters.

| API | Behavior |
| --- | --- |
| `cx.add_window_view(build)` | Opens a window with `build` as root view; returns `(Entity<V>, &mut VisualTestContext)`. |
| `cx.add_empty_window()` | Opens a window with an empty root; returns `&mut VisualTestContext`. |
| `cx.simulate_keystrokes("cmd-shift-p escape")` | Space-separated keystrokes through the keymap and focus path; runs until parked. |
| `cx.simulate_input("text")` | Types text into the focused input handler; runs until parked. |
| `cx.dispatch_action(action)` | Dispatches an action on the focused element path. |
| `cx.simulate_click(position, modifiers)` | Left mouse down and up at a window-local point. |
| `cx.simulate_mouse_move(position, button, modifiers)` | Moves the pointer; pass the held button or `None`. Use it for hover. |
| `cx.simulate_mouse_down` / `simulate_mouse_up` | Single pointer transitions, for drags and pressed states. |
| `cx.simulate_modifiers_change(modifiers)` | Changes held modifiers. |
| `cx.simulate_event(event)` | Any `InputEvent`, such as a `ScrollWheelEvent`. |
| `cx.debug_bounds("selector")` | Bounds from the last frame of the element marked `.debug_selector(\|\| "selector".into())`. |
| `cx.update(\|window, cx\| ..)` | Direct access to `Window` and `App`, for example `focus_handle.is_focused(window)`. |
| `view.read_with(cx, \|view, cx\| ..)` / `view.update(cx, ..)` | Read or change entity state. |

`debug_selector` is compiled only with `test` or `test-support`; it is a no-op
in other builds, so it is safe to leave on production elements.

## Frames and async work

`simulate_keystrokes` and `simulate_input` run until parked afterward. After
an `update`, a spawned task or a timer, call `cx.run_until_parked()` before
asserting so queued effects, notifications and tasks complete.

Timers created with `cx.background_executor().timer(..)` run on the test
clock. Move it forward with `cx.executor().advance_clock(duration)`, then
`run_until_parked()`. Check the UI before the deadline as well as after it;
kuizer-gui's tooltip test in `src/tooltip.rs` does exactly this.

To wait on a condition, `cx.condition(&entity, |view, cx| ..).await` (on
`TestAppContext`) runs until the predicate holds or times out. Prefer
`run_until_parked` when no specific moment matters.

`std::thread::sleep` and real file I/O run on wall-clock time, outside the test
scheduler. If a test waits for real background work, loop on
`run_until_parked` with a short real sleep and a bounded number of attempts.

Process a queued action before changing any input that the action is meant to
validate. Otherwise it can read the later input instead of the value at the
intended test step.

GPUI's non-synced `Animation` uses wall-clock time; advancing the test clock
does not complete it. Use reduced motion when supported, or a bounded wait
appropriate to that production animation before asserting final geometry.

## Resize and clipboard

Use `cx.simulate_resize(size)` on a `VisualTestContext`, or
`TestAppContext::simulate_window_resize(handle, size)`, then
`run_until_parked()` before checking layout. GPUI 0.2.2 has no scale-factor
simulation.

Exercise clipboard behavior through production shortcuts and inspect
`cx.read_from_clipboard()`; seed it with `cx.write_to_clipboard(item)`. The
test platform shares its clipboard across windows.

After activating an external link, `cx.opened_url()` returns the URL the test
platform received. Prompts are driven with `simulate_prompt_answer(button)` and
inspected with `has_pending_prompt()` and `pending_prompt()`.

## Assertion boundaries

- Assert outcomes through the view's state or the core's results, not GPUI
  internals. Test our behavior, not GPUI's.
- `debug_bounds` supports containment, relative position and overlap checks.
  It does not prove a target is unobstructed. A real `simulate_click` at the
  bounds' center decides whether a click reaches the control.
- These tests run in-process on the test platform, with no real renderer.
  Pixel comparisons, packaged-app behavior and OS IME composition need other
  coverage.

## Additional resources

- [test-debugging.md](test-debugging.md): seeds, `ITERATIONS`, `SEED` and
  "Parking forbidden" failures.
- `gpui-0.2.2/src/app/test_context.rs`: the full `TestAppContext` and
  `VisualTestContext` API.
