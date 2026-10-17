# Focus & Keyboard Navigation

**Contents:** [Overview](#overview) · [Quick Start](#quick-start) · [Focus Events](#focus-events) · [Keyboard Navigation](#keyboard-navigation) · [Common Patterns](#common-patterns) · [Best Practices](#best-practices)

## Overview

GPUI's focus system enables keyboard navigation and focus management.

**Key Concepts:**
- **FocusHandle**: Reference to focusable element
- **Focus tracking**: Current focused element
- **Keyboard navigation**: Tab/Shift-Tab between elements
- **Focus events**: on_focus, on_blur

## Quick Start

### Creating Focus Handles

```rust
struct FocusableComponent {
    focus_handle: FocusHandle,
}

impl FocusableComponent {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
        }
    }
}
```

### Making Elements Focusable

```rust
impl Render for FocusableComponent {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_enter))
            .child("Focusable content")
    }

    fn on_enter(&mut self, _: &Enter, _: &mut Window, cx: &mut Context<Self>) {
        // Handle Enter key when focused
        cx.notify();
    }
}
```

### Focus Management

```rust
impl MyComponent {
    fn focus(&mut self, window: &mut Window) {
        self.focus_handle.focus(window);
    }

    fn is_focused(&self, window: &Window) -> bool {
        self.focus_handle.is_focused(window)
    }

    fn blur(&mut self, window: &mut Window) {
        window.blur();
    }
}
```

## Focus Events

### Handling Focus Changes

In GPUI 0.2.2 focus and blur are subscriptions on `Context`, not element
methods: `cx.on_focus(&handle, window, ..)`, `cx.on_blur(&handle, window, ..)`,
plus `on_focus_in` / `on_focus_out` for descendants.

```rust
impl MyInput {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let subscriptions = vec![
            cx.on_focus(&focus_handle, window, |this, _window, cx| this.on_focus(cx)),
            cx.on_blur(&focus_handle, window, |this, _window, cx| this.on_blur(cx)),
        ];
        Self { focus_handle, _subscriptions: subscriptions }
    }
}

impl Render for MyInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let is_focused = self.focus_handle.is_focused(window);

        div()
            .track_focus(&self.focus_handle)
            .when(is_focused, |el| {
                el.bg(focused_background) // in kuizer-gui, a color from theme.rs
            })
            .child(self.render_content())
    }
}

impl MyInput {
    fn on_focus(&mut self, cx: &mut Context<Self>) {
        // Handle focus gained
        cx.notify();
    }

    fn on_blur(&mut self, cx: &mut Context<Self>) {
        // Handle focus lost
        cx.notify();
    }
}
```

## Keyboard Navigation

### Tab Order

A tracked handle is a Tab stop only when the handle says so. `cx.focus_handle()`
starts with `tab_stop = false`, and an element's own `.tab_index()` /
`.tab_stop()` do not apply to a handle passed to `track_focus`: build the handle
with the flag.

```rust
// In the owning entity's constructor:
let focus1 = cx.focus_handle().tab_stop(true);          // Tab order: 1
let focus2 = cx.focus_handle().tab_stop(true);          // Tab order: 2
let hidden = cx.focus_handle();                         // focusable, never a Tab stop

div()
    .child(div().track_focus(&focus1).child("first"))
    .child(div().track_focus(&focus2).child("second"))
    .child(div().track_focus(&hidden).child("container"))
```

The Tab table is rebuilt every frame from the elements that painted with a
tracked handle, in tree order (`tab_index` reorders within a `tab_group`). A
stateless component may create its handle in `render` through keyed state —
`window.use_keyed_state(id, cx, |_, cx| cx.focus_handle().tab_stop(true))` —
which survives re-renders, so the order stays stable; `Button` does exactly
this in gpui-kit. A focused element receives `ClickEvent::Keyboard` in `on_click` when
Enter or Space is released, so a `track_focus` + `on_click` row activates from
the keyboard without extra bindings.

### Focus Within Containers

```rust
impl Container {
    fn focus_first(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(first) = self.children.first() {
            first.read(cx).focus_handle.focus(window);
        }
    }

    fn focus_next(&mut self, window: &mut Window, _cx: &mut Context<Self>) {
        window.focus_next(); // next tab stop in the window's tab order
    }
}
```

## Common Patterns

### 1. Auto-focus on Mount

```rust
impl MyDialog {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();

        // Focus when created
        focus_handle.focus(window);

        Self { focus_handle }
    }
}
```

### 2. Focus Trap (Modal)

```rust
impl Modal {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "tab" {
                    // Keep focus within modal
                    this.focus_next_in_modal(window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(self.render_content())
    }
}
```

### 3. Conditional Focus

Not in GPUI 0.2.2: there is no `on_mount`. Focus in the method that changes
the state instead:

```rust
impl Searchable {
    fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_active = true;
        self.focus_handle.focus(window);
        cx.notify();
    }
}
```

### 4. Restore Lost Focus

If the focused element is no longer rendered (a table swapped for an empty
state), key dispatch starts at the dispatch tree's root node, so the view's
`key_context` bindings and `on_action` handlers stop firing. Register one
`cx.on_focus_lost` listener per window, usually on the root view, and move
Focus back to the view's own handle. Track that handle on the element that
carries its `key_context`. (gpui-kit's `window.focus_lost_restore_target` is
not in GPUI 0.2.2.)

```rust
let focus_lost = cx.on_focus_lost(window, |this, window, _cx| {
    this.focus_handle.focus(window);
});
// `this.focus_handle` is the view's tracked handle. Keep `focus_lost:
// Subscription` on the view; dropping it unregisters.
```

## Best Practices

### ✅ Track Focus on Interactive Elements

```rust
// ✅ Good: Track focus for keyboard interaction
div()
    .track_focus(&self.focus_handle)
    .on_action(cx.listener(Self::on_enter))
```

### ✅ Provide Visual Focus Indicators

Not in GPUI 0.2.2: gpui-kit's `focus_ring_style`. Draw the ring yourself from
`theme.rs` values so every control matches:

```rust
let is_focused = self.focus_handle.is_focused(window);

div()
    .track_focus(&self.focus_handle)
    .when(is_focused, |el| el.border_2().border_color(focus_ring)) // focus_ring from theme.rs
```

A ring painted outside the element is clipped by an ancestor with
`overflow_hidden()`; leave it room or draw the ring inside the element.

### ❌ Don't: Forget to Track Focus

```rust
// ❌ Bad: No track_focus, keyboard navigation won't work
div()
    .on_action(cx.listener(Self::on_enter))
```

