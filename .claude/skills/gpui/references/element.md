# Custom Elements

**Contents:** [When to Use](#when-to-use) · [Quick Start](#quick-start) · [Core Concepts](#core-concepts) · [Reference Documentation](#reference-documentation)

## When to Use

Use the low-level `Element` trait when:
- Need fine-grained control over layout calculation
- Building complex, performance-critical components
- Implementing custom layout algorithms (masonry, circular, etc.)
- High-level `Render`/`RenderOnce` APIs are insufficient

**Prefer `Render`/`RenderOnce` for:** Simple components, standard layouts, declarative UI

## Quick Start

The `Element` trait provides direct control over three rendering phases:

```rust
impl Element for MyElement {
    type RequestLayoutState = MyLayoutState;  // Data passed to later phases
    type PrepaintState = MyPaintState;        // Data for painting

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    // Phase 1: Calculate sizes and positions
    fn request_layout(&mut self, .., window: &mut Window, cx: &mut App)
        -> (LayoutId, Self::RequestLayoutState)
    {
        let layout_id = window.request_layout(
            Style { size: size(px(200.).into(), px(100.).into()), ..Default::default() },
            vec![],
            cx
        );
        (layout_id, MyLayoutState { /* ... */ })
    }

    // Phase 2: Create hitboxes, prepare for painting
    fn prepaint(&mut self, .., bounds: Bounds<Pixels>, layout: &mut Self::RequestLayoutState,
                window: &mut Window, cx: &mut App) -> Self::PrepaintState
    {
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        MyPaintState { hitbox }
    }

    // Phase 3: Render and handle interactions
    fn paint(&mut self, .., bounds: Bounds<Pixels>, layout: &mut Self::RequestLayoutState,
             paint_state: &mut Self::PrepaintState, window: &mut Window, cx: &mut App)
    {
        window.paint_quad(fill(bounds, rgb(0x1e1e1e)).corner_radii(px(4.)));

        window.on_mouse_event({
            let hitbox = paint_state.hitbox.clone();
            move |event: &MouseDownEvent, phase, window, cx| {
                if hitbox.is_hovered(window) && phase.bubble() {
                    // Handle interaction
                    cx.stop_propagation();
                }
            }
        });
    }
}

// Enable element to be used as child
impl IntoElement for MyElement {
    type Element = Self;
    fn into_element(self) -> Self::Element { self }
}
```

## Core Concepts

### Three-Phase Rendering

1. **request_layout**: Calculate sizes and positions, return layout ID and state
2. **prepaint**: Create hitboxes, compute final bounds, prepare for painting
3. **paint**: Render element, set up interactions (mouse events, cursor styles)

### State Flow

```
RequestLayoutState → PrepaintState → paint
```

State flows in one direction through associated types, passed as mutable references between phases.

### Key Operations

- **Layout**: `window.request_layout(style, children, cx)` - Create layout node
- **Hitboxes**: `window.insert_hitbox(bounds, behavior)` - Create interaction area
- **Painting**: `window.paint_quad(...)` - Render visual content
- **Events**: `window.on_mouse_event(handler)` - Handle user input

## Reference Documentation

### Complete API Documentation
- **API**: element-api.md — associated types, hitbox system, event handling, cursor styles
- **Examples**: element-examples.md — text, interactive, complex elements
- **Patterns**: element-patterns.md — text, container, interactive, composite, scrollable
- **Best Practices**: element-best-practices.md — performance, state, common pitfalls
- **Advanced**: element-advanced.md — masonry/circular layouts, memoization, virtual lists

These upstream gpui-kit files are not included here. For GPUI 0.2.2, read the `Element` trait in `gpui-0.2.2/src/element.rs` and the built-in elements in `gpui-0.2.2/src/elements/`.
