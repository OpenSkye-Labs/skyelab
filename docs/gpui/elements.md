# GPUI rules: Elements

Part of [docs/gpui.md](../gpui.md); the same rules apply.

- Anything interactive (click, hover, scroll, drag) needs `.id(..)` for a stable `ElementId`. Ids must be unique among
  siblings. Use the domain id (thread id, path), not the loop index.
- Long lists: `uniform_list` for fixed-height rows, `list` + `ListState` for variable height. Never `.children(10_000 items)`.
- A chat-like list (a thread's timeline) sets `ListState::set_follow_mode(FollowMode::Tail)` when made. A plain
  `ListAlignment::Top` list stays where it is while rows grow under it, so an opened thread shows its first message
  and a waiting plan or question streams in below the composer, unseen. Tail mode follows the end, stops when the user
  scrolls up, and starts again once they scroll back down; `scroll_to_end()` on your own send follows again.
- Controls come from `crate::ui` ([docs/design/components.md](../design/components.md)): `Button`, `icon_button`,
  `Switch`, `Checkbox`, `SegmentedControl`, `Tabs`, `TextField`, `Kbd`, `Tooltip`, `Card`, `Divider`. Use gpui-component
  for what `ui` doesn't cover (`Input` editing, `List`, `Table`, `Dock`, `Dialog`, `Markdown`, the code
  editor). Only drop to raw `div()` for layout.
- A `ui` component is a `#[derive(IntoElement)]` + `RenderOnce` builder: no entity, no `cx.notify()`, no state across
  frames. Hover and press are GPUI styles (`.hover(..)`, `.active(..)`, `.focus_visible(..)`), never a hovered flag in
  view state. Build ids from the domain (`SharedString`, `(name, ix)` tuples), never `format!` in a row that redraws often.
  Only a tooltip is a view, built when the pointer rests. Colors come from one function per component over theme
  fields, and geometry from `ui::Size`.
- `ui::Button` is gpui-base's headless `Button` (role, focus, Enter/Space, the disabled click guard) styled by us. It
  takes the caller's own style (`.w_full()`, `.rounded(..)`) after its own, so the caller wins, and on mouse down it
  calls `window.prevent_default()` so a press never moves focus off the field being typed in
  (`ui/button_test.rs`). Without that, clicking any composer button blurs the prompt.
- gpui-component's `DropdownMenu` works for any `Styled + Selectable + InteractiveElement` trigger, so
  `impl DropdownMenu for ui::Button {}` keeps `.dropdown_menu(..)` on our own button.
- Colors come from `cx.theme()` (`crate::ui::ActiveTheme`), never hard-coded `rgb(..)` in views. That keeps light/dark themes working.
- gpui-component's `DialogClose` (0.10 and 0.11) wraps its trigger in a `size_full` div, so in a `DialogFooter` the Cancel button
  stretches across the footer. Use `ui::dialog_cancel(id)` (a plain `Button` whose `on_click` calls
  `window.close_dialog(cx)`); `DialogClose` appears nowhere in the app.
- A click on a `ui::Checkbox` or `ui::Switch` also reaches a clickable row around it, so handlers on both toggle twice
  (no change). Put the handler on the row only and leave the checkbox as its picture.
- `items_baseline()` puts a box with no text (a status dot) on the line's baseline, so it sits below the text's
  middle. A row that mixes a dot or icon with text uses `items_center()`; keep baseline for text-only rows.
- Register app SVGs in `ui/icons.rs` and use `SkyeIcon` at the call site. The registry
  embeds the file and supplies the same path to `AppAssets::load` and `AppAssets::list`.
  GPUI tints the SVG with the element's text color. Check the glyph in a live screenshot.
- Text input is not built into GPUI. Use `gpui_component::input`, never hand-roll `EntityInputHandler`.
- `img()` takes its height from its width and the image's aspect ratio, even with `.h(..)` set: a wide box draws a
  square image as tall as the box is wide. Cap it with `.max_h(..)` (the image viewer does), and check a fit in a live
  screenshot, not only by reading the layout.
- `Textarea::on_paste` sees the `ClipboardItem` before the text goes in; returning `true` keeps it out (long pastes
  become attachments that way).
- Something inline in a text field that must act as one unit (the composer's mention chips) is a gpui-base inline token:
  `InputContent::with_token` over the text's own bytes, `set_tokens` / `set_value(content)`, drawn by
  `Textarea::token(..)`. The engine already steps the caret over it, expands edits to the whole token, keeps it in
  undo, copies its text and refuses to change tokens mid IME composition, so no overlay or custom text element.
  `set_value` drops the tokens and emits no `Change`: re-mark them after it (`ThreadView::set_prompt`), and move a
  field's text with `content()`, not `value()` (`composer_mentions.rs`).
- `ListState::reset_with_uniform_height` gives every item the same height, so a row that grows (a comment under a
  diff line) is clipped. Use the measured `reset` while any row is taller, and lay the list out again (keeping the top
  row) whenever that set changes (`ChangesView::comments_changed`).
- A popover or `DropdownMenu` marks its trigger open through `Selectable::open` and reads it back with `is_open`
  every frame, and both default to `selected`/`is_selected`. A trigger whose `selected` means something else (a
  filter that is on) overrides both with a field of its own (`ui::Button::menu_open`), or it reads as open for good.
  A button hides its tooltip while open: shown late, it covers the menu's first item.
- GPUI checks a shown (or late-showing) tooltip against its trigger's bounds, not whether something now covers it, so
  the tooltip of the button that opened a dialog stayed drawn over the dialog. `ui::open_dialog` records the opening
  (`tooltip::dialog_opened`), and a `ui::Tooltip` built before it, or at the pointer that opened it, draws nothing
  while the dialog is up. Open every dialog through `ui::open_dialog`. Our own modal (Settings)
  calls `tooltip::modal_shown` as it opens and closes.
- A `PopupMenu` centers a left check on a multi-line item (an icon, a name and a description), so it sits beside
  the description; use `.check_side(Side::Right)`.
- A `ThemeConfig` with no `highlight` keeps the code colors applied before it (gpui-component starts with its light
  ones), so a dark theme draws code navy on black. `ui::themes::adjust` fills each mode's default.
- `ListState::scroll_to_reveal_item` doesn't stop `FollowMode::Tail`: the next layout puts the list back at its end.
  Call `pause_following_tail()` first (find-in-thread, "View output").
- Hover-only buttons over a row (settle, snooze): `.group(name)` on the row, the buttons `absolute()` with
  `.invisible().group_hover(name, |s| s.visible())` and the text under them `group_hover(.., |s| s.invisible())`. A
  hidden div returns before painting, so it registers no mouse listeners and never takes the row's clicks. A
  button's click bubbles on (`ui::Button` stops propagation only while loading), so wrap the buttons in a div
  whose `on_click` calls `cx.stop_propagation()`, or a click also opens the row.
  Give the absolute buttons the row's own fill (`sidebar_rows.rs`): with none they draw over the title's letters.
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
- A menu opened from triggers in other views (the machine menu: the composer chip, the status bar badge, the palette)
  is drawn by the workspace root as `deferred(div().absolute().inset_0().occlude().on_mouse_down(close))` holding
  `anchored().position(click).anchor(Anchor::BottomLeft).snap_to_window_with_margin(..)`. The backdrop eats the press,
  so a second click on the trigger closes the menu instead of closing and reopening it, and nothing under it reacts.
  The trigger passes `ClickEvent::position()` in its action, since only it knows where it was drawn. The card itself
  stops `on_mouse_down` propagation so its own rows don't reach the backdrop. In gpui-pre 0.3.12 and 0.3.14 the corner enum is
  `gpui::Anchor`, not `Corner`.
- A dock's empty state is a panel that stays its first tab (the right dock's "Open a surface", the bottom dock's
  `TerminalsEmpty`), so the dock never goes empty. Letter keys bound in its own key context (`Surfaces`) map straight
  to the workspace's actions and fire only while it has focus.
- gpui-component 0.11 always draws a dock tab bar's "…" menu, and it always holds Zoom In, disabled unless the panel
  zooms (`BasePanel::zoomable` and a menu-visible `Panel::zoom_control`; `Toolbar` alone disables it too). A disabled
  item looks enabled and a click on it keeps the menu open, so every dock panel keeps the defaults (zoomable,
  `PanelControl::Menu`, or `Both` for a toolbar button). A zoomed group fills the window, so on macOS `CenterTabs` pads
  its bar past the traffic lights. Panel controls in the bar go in `Panel::title_suffix` (it takes any element);
  `toolbar_buttons` wants gpui-component's banned `Button`.
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

- gpui-component's markdown `TextView` takes per-block action rows: `code_block_actions` (the code block's corner) and
  `table_actions` (a full-width row under each table, given a `TableData` with plain-text cells and the table's
  markdown; align it yourself with `h_flex().justify_end()`). Their ids are scoped per block, so a plain `"copy"`
  id is fine. Pass a fn that returns `AnyElement`: an `fn(..) -> impl IntoElement` fails the hook's higher-ranked
  bound ("one type is more general than the other").
- Edition 2024: a `fn(.., cx: &App) -> impl IntoElement` captures `cx`'s lifetime, so a `'static` slot closure that
  returns it (gpui-component `Command::footer` / `header`) fails with "lifetime may not live long enough". Return the
  concrete type (`gpui::Div`, `Stateful<Div>`) from such helpers (`palette_view.rs`).
- GPUI's `ClipboardItem` carries only text, images and paths: no `text/html`. A rich copy (a table as HTML) goes
  through `arboard` (`set().html(html, Some(plain))`), with one `Clipboard` kept for the app's life, since on Linux
  the owner serves pastes (`table_copy.rs`).
- To mark words in a markdown `TextView` (find in thread), own its state: `TextView::markdown(id, text)` keeps a
  hidden `TextViewState` you can't reach. Make it with `window.use_keyed_state(..)`, `set_text` it every render
  (cheap when equal; an extension streams as an append), draw `TextView::new(&state)`, and call
  `set_range_highlights` with ranges of `state.rendered_text()` (the plain text, not the markdown source). Parses
  land async, so set them again whenever that snapshot (`==` per parse), the query or the current match changes; a
  highlight paints behind glyphs, so it never changes layout (`find_marks::mark_words`).
- Plain `String` text marks the same way with `StyledText::with_highlights` and a `HighlightStyle` background.

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
- `window.open_dialog` panics without `Root` (test windows: `threads_test::add_rooted`). Toasts are SkyeLab's own
  (`ui::toast`) and need no `Root`; a toast's action button closes it after running, and its wrapper carries the
  debug selector `toast-action`. Debug bounds are the last drawn frame's, and clock ticks alone don't redraw: call
  `window.refresh()` before reading them after a toast appears.
- A `Dialog` sits in `Root`'s layer, outside the workspace: actions dispatched inside it never reach the workspace.
  A modal that must reach it is a `deferred` layer in `Workspace::render` (`settings_tab::settings_modal`).
- A `Dialog`'s `on_close` runs for Esc and the backdrop, not for `window.close_dialog`.
- gpui-component ignores theme JSON keys it doesn't know, so per-theme data it has no field for (the terminal's
  `terminal.ansi`) lives in the same file and SkyeLab reads it itself (`ui::themes::ansi`) into a global.
- To show a theme that isn't applied (onboarding's theme tiles, `ui::themes::swatch`), clone `Theme::global` and
  `apply_config` the registry's `ThemeConfig` on it, then read the colors. Don't parse the config's strings yourself:
  a key the theme leaves out has no string, and the accent preset is baked in by `themes::adjust`.
- `apply_config` swaps any theme color it can't parse for the default, with no error. Theme files use hex or a
  color name gpui-component knows (0.11 has no `zinc`, `slate` or `stone`); `themes_test::every_theme_color_parses`
  runs every value through `try_parse_color`.
- A hex color loaded into a theme is stored as `Hsla`, and the round trip moves its contrast by a few hundredths
  (`scripts/gen-themes.py` computed 4.50:1, `contrast_test` read 4.48). A generator aiming at a floor aims a little
  over it (the script uses 4.6 for 4.5).
- `gpui_base::Button` sets `line_height(relative(1.))` for its one-line label, and its children inherit it. A button
  holding wrapping text (Appearance's theme cards) sets `line_height(gpui::phi())`, GPUI's default, on that text.
- `on_focus_out` fires only in an active window (tests: `activate_window()`), and going inactive counts as one:
  check `window.is_window_active()` first.
- A `v_flex` stretches its children across its width (align-items: stretch), so a segmented control or pill row
  dropped straight into one paints its background edge to edge. Wrap it in an `h_flex()` to keep it content width
  (`pr_actions.rs` merge methods).
- gpui-component's `Textarea` / `Input` element writes its own `disabled` and `readonly` builder values into the state
  on every render, so `state.set_readonly(true)` is undone by the next frame. Drive read-only from the element
  (`.readonly(dictating)` in `composer.rs`). Programmatic `set_value` / `insert` still edit a read-only field.
- An `img()` of a 2x bitmap (a webview snapshot) given only `.w_full()` takes its height from the bitmap's pixel
  height, not its aspect, and letterboxes the picture inside (blank bands above and below). Give it its logical size
  (`.w(px(css_w)).h(px(css_h))`), or put it `size_full` in a div that has `.max_w_full().aspect_ratio(..)`
  (`html_card`); `.aspect_ratio` on the `img` itself didn't change it.
- Text of unknown length in an `h_flex` beside buttons (a confirm naming files) doesn't wrap: a `flex_1` div without
  `min_w_0` grows to its text's width and pushes the buttons out of the panel, past the window. Put the text on its own
  line in a `v_flex` and the buttons in a row under it (`changes_turns.rs`'s rewind confirm), or give it `min_w_0`.
- **Text the user approves must wrap, never clip.** `whitespace_nowrap()` inside an `overflow_hidden()` or
  y-only-scroll box cuts a long line at the edge with no sign, so a permission card could show `# config` while the
  line goes on to `; curl … | sh`. Approval diffs use `diff_view::line_row(.., wrap: true, ..)` (rows `min_h`, not
  `h`); clipping is fine only for text that is shown in full elsewhere (the Changes tab, the edit card).
- **A long read-only text or diff: gpui-component's `diff::Diff`**, not a hand-made list. `DiffFile::parse` a unified
  patch into a `DiffState` entity and draw `Diff::new(&state).soft_wrap(true).size_full()`: it lays out only the
  lines in view, wraps long ones and highlights by the path's language (the approval's View all, `ask_approval.rs`).
- **Dialogs need gpui-component's `Root` at the window's top**, tests included: `window.open_dialog` panics with
  "component window state is missing" otherwise. `one_view` (`claude_folded_turns_test.rs`) wraps its `ThreadView`
  in `Root` for that.
- **`svg()` and `Icon` draw an SVG as a one-color mask in the text color; a multi-color or brand-colored SVG needs
  `img(path)`.** `img` with an embedded path loads through the app's `AssetSource` and keeps the file's own fills. Provider
  logos go through `ui::provider_logo` for that reason (docs/design/third-party-marks.md). Never `img()` an SVG from
  outside the app's own assets (usvg follows `<image href>`).
