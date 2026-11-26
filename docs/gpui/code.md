# Code and diff rows

Drawing code line by line: the timeline's diffs (`workspace/diff_view.rs`) and anything like them.

- **Rows inside a `max_h` scrolling column get `flex_shrink_0()`.** Flex children shrink by default, so once the
  content passes the max height a wrapped row is squeezed below its text and the next row paints over its last line
  (the approval diff's long line). The scroll box scrolls only if its rows keep their size.
- **Syntax colors for code drawn line by line come from `ui::syntax`**, not gpui-component: its syntect highlighter
  is crate-private. Parse each side of a diff in order (old: context + removed, new: context + added) so a block
  comment or docstring colors through, keep the spans' highlight *names*, resolve them with
  `theme.highlight_theme.style(name)` per drawn row so a theme switch recolors, and cache the parse per diff
  (`diff_view::prepared`): a frame then builds only the visible rows of a `uniform_list`.
- A view's `render` holds its entity leased: `cx.entity().read(cx)` inside a render helper panics. Pass `this: &V`
  in (`diff_view::body`), and give a `uniform_list` closure a `cx.weak_entity()` for its clicks.
