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

## Paged content in one scroll view

- **A scroll view's offset outlives the content swapped into it.** Onboarding showed each step in the same
  `overflow_y_scroll` div, so a step opened at the scroll position the last one was left at, its title off the top.
  Give the div a `ScrollHandle` (`track_scroll`) and `set_offset(Point::default())` when the page changes.
- **Actions that end a page go in a footer outside the scroll view**, not at the bottom of the scrolled column: on a
  short window they fall below the fold with nothing saying there is more. Where the column can still overflow, lay a
  short `linear_gradient` fade (background with alpha 0 → background) over its bottom padding: it covers only empty
  space when nothing is hidden, and a div without listeners takes no mouse.

## Docks on a narrow window

- **A side dock's set size has no maximum** (`set_dock_size`; only a drag is clamped): a saved 560 px right dock on a 760 px window left the
  center (the thread) 0 px wide. Fit it yourself from the viewport (`dock_fit.rs`): on restore, on
  `observe_window_bounds` and after a toggle, shrink the dock to leave the center its minimum, and close it (or hide
  the other dock) when even its own minimum doesn't fit.

## Rows that must never draw text over text

- **A child's `min_w(..)` overflows a `flex_1().min_w_0()` parent instead of shrinking it**: the thread header's title
  (min 72 px) spilled over the account chip once the breadcrumb itself reached 0. Put the floor on the shrinking
  container (`min_w(px(72.)).overflow_hidden()`), let the text inside truncate to 0, and let the next item (the chip)
  shrink after it. A chip forced `flex_shrink_0` was the other half of the overlap.
- **A one-line banner with buttons wraps them under the text when there's no room**: `flex_wrap()` on the row and a
  `flex_1().min_w(px(160.))` text group (the min is what decides the wrap; `flex_1`'s zero basis alone never wraps).
- **A meta line that can't fit moves its last item to a second line**: `flex_wrap()` with `gap_x_*`, and the item
  keeps `min_w_0().truncate()` as the last resort. Its hypothetical width is its content, so it wraps before it truncates.
- **Tiles that should span a fixed-width card**: `div().grid().grid_cols(n)` spreads them evenly; `flex_wrap` leaves a
  ragged gap on the right.
