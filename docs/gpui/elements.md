# GPUI rules: Elements

Part of [docs/gpui.md](../gpui.md); the same rules apply.

- Anything interactive (click, hover, scroll, drag) needs `.id(..)` for a stable `ElementId`. Ids must be unique among
  siblings. Use the domain id (thread id, path), not the loop index.
- Long lists: `uniform_list` for fixed-height rows, `list` + `ListState` for variable height. Never `.children(10_000 items)`.
- Use `gpui-component` before building a widget: `Input`, `Button`, `List`, `Table`, `Dock`, `Tabs`, `Dialog`, `Notification`,
  `Markdown`, the code editor. Only drop to raw `div()` for layout.
- Colors come from `cx.theme()` (`ActiveTheme`), never hard-coded `rgb(..)` in views. That keeps light/dark themes working.
- gpui-component 0.10's `DialogClose` wraps its trigger in a `size_full` div, so in a `DialogFooter` the Cancel button
  stretches across the footer. Use `ui::dialog_cancel(id)` (a plain `Button` whose `on_click` calls
  `window.close_dialog(cx)`); `DialogClose` appears nowhere in the app.
- A click on a gpui-component `Checkbox` also reaches a clickable row around it, so handlers on both toggle twice
  (no change). Put the handler on the row only and leave the checkbox as its picture.
- `items_baseline()` puts a box with no text (a status dot) on the line's baseline, so it sits below the text's
  middle. A row that mixes a dot or icon with text uses `items_center()`; keep baseline for text-only rows.
- Every `IconName` compiles, but only gpui-kit-assets' default set and the icons listed in `ui::ExtraIcons`
  (`icon_assets!` in `ui/mod.rs`) ship; any other draws nothing, with no error. Add a new icon to that list in the
  same change, and look for it in a live screenshot.
- Text input is not built into GPUI. Use `gpui_component::input`, never hand-roll `EntityInputHandler`.
- `img()` takes its height from its width and the image's aspect ratio, even with `.h(..)` set: a wide box draws a
  square image as tall as the box is wide. Cap it with `.max_h(..)` (the image viewer does), and check a fit in a live
  screenshot, not only by reading the layout.
- `Textarea::on_paste` sees the `ClipboardItem` before the text goes in; returning `true` keeps it out (long pastes
  become attachments that way).
- `ListState::reset_with_uniform_height` gives every item the same height, so a row that grows (a comment under a
  diff line) is clipped. Use the measured `reset` while any row is taller, and lay the list out again (keeping the top
  row) whenever that set changes (`ChangesView::comments_changed`).
- Hover-only buttons over a row (settle, snooze): `.group(name)` on the row, the buttons `absolute()` with
  `.invisible().group_hover(name, |s| s.visible())` and the text under them `group_hover(.., |s| s.invisible())`. A
  hidden div returns before painting, so it registers no mouse listeners and never takes the row's clicks. A
  gpui-component `Button`'s click bubbles on (it stops propagation only while loading), so wrap the buttons in a div
  whose `on_click` calls `cx.stop_propagation()`, or a click also opens the row.
- A card that floats over other views (the thread details card) is a `deferred(..)` child of the window's root, an
  `absolute()` box placed from what the root knows (the header height, the right dock's width), so it paints after
  and over the dock. Not `anchored()`: that snaps to the window, while this one must sit under a button another view
  draws and beside a dock whose width changes.
- A floating card is an entity drawn as `AnyView::from(e).cached(style)`, or every render of its parent (each streamed
  delta in the thread) renders it too. A cached view can't size to its content: give the outer box a fixed size
  (`top`/`bottom`/`right`/`w`) and let the card inside take `max_h_full()` and scroll. The empty space under it stays
  click-through because only the card has handlers.
- The card itself `occlude()`s, so a click or wheel on it never reaches the chat under it. Outside presses come from
  its `on_mouse_down_out`, emitted as an event the owner decides on: the owner knows whether the card covers the
  chat (T3's popover mode) and skips the press that lands on the page header, where the toggle's own click closes it.
  Closing on the down and reopening on the toggle's click would otherwise flicker it open again.
- A dock's empty state is a panel that stays its first tab (the right dock's "Open a surface", the bottom dock's
  `TerminalsEmpty`), so the dock never goes empty. Letter keys bound in its own key context (`Surfaces`) map straight
  to the workspace's actions and fire only while it has focus.
- `Panel::on_removed` fires only when a panel leaves the dock for good (its tab closed, `remove_panel`), not when it
  moves between tab groups, so it is where a closed tab records how to come back (`workspace/reopen.rs`). It gets the
  `Window`, so per-window state is keyed by `window.window_handle()`.
- A hit target over part of a row (the diff gutter) is an `absolute` child with its own id and mouse handlers. A plain
  `absolute().inset_0()` div with no handlers (a selection tint) draws over the row without taking its clicks, so the
  row's own colors still show under it.

- A draggable divider between flex children (terminal splits) is its own child with a width, not a 1px line with an
  overhanging `absolute()` grab strip: the next sibling is drawn later and wins the hit test over the overhang. Size
  the children with `style().flex_grow = Some(share)` over `flex_basis(relative(0.))`, so the dividers come out of the
  space first. The drag is `on_drag(payload, ..)` on the divider and `on_drag_move::<Payload>` on the split, which
  gets the split's bounds; nested splits all see the move, so the payload names the split's path and each checks it.
  The first move after the press only starts the drag; `on_drag_move` hears the moves after it (a test or
  `SKYELAB_VERIFY_ACTIONS` `drag:` sends two).
- A tab title that needs clicks (rename on double-click, its own ×) implements `Panel` by hand instead of the
  `panel!` macro: `Panel::title` gets `&mut Context<Self>` for `cx.listener`. Name the view's own title getter
  differently (`tab_label`): `self.title()` on `&mut self` resolves to the trait's `title(window, cx)` first.
- Read `ClickEvent::click_count()` for a double-click; the × inside a dock tab calls `cx.stop_propagation()` so the
  tab's own click doesn't also run.

### Text selection

- A selectable `TextView` joins the window's one text selection (`gpui_base::TextSelection`, not re-exported by
  gpui-component: `gpui-base` is a direct dependency at the version gpui-component pins). It has no event for a
  pointer selection changing (`observe_touch_selection` is for long-press). To act on a selection (Quote), listen for
  the mouse-up on the container with `capture_any_mouse_up` and read `TextSelection::selected_text` in
  `cx.defer_in`: by then the text view has ended its drag, and a click on your button has already run.
- A button that acts on the selection gets `ElementExt::keep_text_selection(id)`, or pressing it clears the
  selection first. Float it with `deferred(anchored().position(window_point))`: the mouse event's position is
  already in window coordinates. `simulate_mouse_down/move/up` over a `TextView` makes a real selection in tests.

### Settings modal, themes and text size

- Text sizes are rems (`ui::BODY`…): `Root` sets the rem to `theme.font_size`, so Appearance's text size scales
  every view. A px size never scales.
- Bake a theme choice into the `light_theme`/`dark_theme` configs, then `Theme::change` (refreshes every window).
  Fields written on `Theme` are lost at the next appearance flip (`ui::themes::apply`).
- `window.open_dialog` panics without `Root` (test windows: `threads_test::add_rooted`).
- A `Dialog` sits in `Root`'s layer, outside the workspace: actions dispatched inside it never reach the workspace.
  A modal that must reach it is a `deferred` layer in `Workspace::render` (`settings_tab::settings_modal`).
- A `Dialog`'s `on_close` runs for Esc and the backdrop, not for `window.close_dialog`.
- gpui-component ignores theme JSON keys it doesn't know, so per-theme data it has no field for (the terminal's
  `terminal.ansi`) lives in the same file and SkyeLab reads it itself (`ui::themes::ansi`) into a global.
- `on_focus_out` fires only in an active window (tests: `activate_window()`), and going inactive counts as one:
  check `window.is_window_active()` first.
- A `v_flex` stretches its children across its width (align-items: stretch), so a segmented control or pill row
  dropped straight into one paints its background edge to edge. Wrap it in an `h_flex()` to keep it content width
  (`pr_actions.rs` merge methods).
