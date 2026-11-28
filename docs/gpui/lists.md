# GPUI rules: virtualized lists

Part of [docs/gpui.md](../gpui.md).

- **Selecting text across virtualized rows:** one `gpui_base::TextSelectionHandle` for the whole list, registered each
  frame by an element around it (`diff_viewer_select.rs`). Resolve points to content keys (line, byte) and copy from
  the keys (`copy_with`), so the selection outlives rows that scroll away. `update_runs` paints nothing: draw the
  selected ranges as highlights yourself, and test them with `window.painted_quads()`. Copy is dispatched from the
  focus, so `focus_with` must focus the view; and the window root's copy **trims** the text, so code (a first
  line's indent) needs its own copy action in the view's key context.
- **`list` forgets every row height at a new width**, hints from `with_uniform_item_height` too, its first layout
  included; the wheel and the scrollbar then stop at the last row measured. Give the hint again after the list's
  prepaint whenever its width changed, and test with one big wheel event, also after `simulate_resize`.
- A test reading `window.painted_quads()`: quads come in drawing order (`order` is the layer, often equal), and a
  border is four quads of the full bounds masked to its edges, so check `content_mask` too.
