# Fitting content to a width

Lessons from the sidebar's update pill (`workspace/update_notice.rs`).

- **A layout that depends on a container's width** (the sidebar's update pill: label, short label or icon-only
  circle) reads it from a `canvas(..).absolute().size_full()` child that stores the width in a cell kept by
  `window.use_keyed_state` and calls `cx.defer(move |cx| cx.notify(view))` only when the width crosses the line that changes the layout (a
  plain notify from prepaint, mid-draw, was sometimes lost and the pill stayed collapsed until the next redraw).
  The value is one frame old, so the first frame draws the narrow form; ride a `gpui_base::spring` over a 0..100 px
  "fraction" (not over the width, which moves on every resize drag) so that first expand animates and Reduce
  Motion snaps it.
- **Whether a label fits** is `window.text_system().shape_line(text, size, &[window.text_style().to_run(len)],
  None).width` (+1 px for rounding), checked in render against the measured room; truncation alone turned
  "Downloading… 42%" into "Downlo…". Keep a short form to fall back to (`update_notice::fit`).
- **A gpui-component `Popover` trigger can be our own `RenderOnce`** that implements `Selectable` (the popover passes
  its open state through `selected`). A smaller clickable inside it (the pill's Retry) needs
  `on_mouse_down(Left, |_, _, cx| cx.stop_propagation())`, or the press also toggles the popover.
- **`theme.muted` can equal the sidebar's background** in light themes (the pill vanished at rest). For a control
  surface on the sidebar, wash the sidebar's own ink over it (`sidebar_foreground.opacity(0.06)`, 0.1 hovered): it
  contrasts in every theme.
