# GPUI rules: Keyboard and focus

Part of [docs/gpui.md](../gpui.md); the same rules apply.

- Commands are `actions!(skyelab, [NewThread, ..])`, bound with `KeyBinding::new("cmd-n", NewThread, Some("Workspace"))`. `KeyBinding::new` panics on `secondary-` (it has no OS to resolve it): use `KeyBinding::load(.., cx)` for those.
  Use `secondary-` (cmd on mac, ctrl elsewhere) for cross-platform bindings.
- Handlers: `.key_context("Workspace").on_action(cx.listener(Self::new_thread))`. Every user command is an action,
  so it is bindable and shows in the command palette. No ad-hoc `on_key_down` for commands.
- The palette lists the actions the focused element can handle (`is_action_available`), so an action that only makes
  sense in some states is registered only then (`ThreadView::handoff_actions`: a Claude thread registers
  `ContinueInCodex` and not `ContinueInClaude`). A handler that is always registered and does nothing shows a dead entry.
- A helper that registers listeners from `render` takes `&Context<Self>` (`cx.listener` needs only `&`): a `&mut cx`
  inside a builder `.map` conflicts with the `cx.theme()` borrow the rest of the element still holds.
- A user keymap is Zed's reload: snapshot `cx.key_bindings()` once every `init` has bound (`KeyBinding` is `Clone`,
  and the snapshot keeps gpui-component's bindings too), then on each load `clear_key_bindings`, the snapshot, the
  user's rules last. Later wins at equal context depth, so the rules override. A binding added after the snapshot is
  gone at the first reload (⌘Q was), so every `bind_keys` runs before `keymap::init` (`main.rs`'s `init_keys`).
- `NoAction` only hides bindings whose `meta` is not smaller than its own, and a binding without a meta counts as 0
  (the strongest). Give defaults a larger meta than the user's rules, or a user `none` hides nothing.
- When several bindings match a key, GPUI dispatches them in precedence order until one is handled, so a broad
  `Workspace` binding (Esc → composer) is a fallback a deeper context's handler still beats, and a handler can
  `cx.propagate()` to pass the key on.
- A ring drawn inside a dock panel's view (the composer) only changes when that view is notified: panels are cached,
  so whoever moves the ring notifies the view too. A ring over a dock is drawn by the workspace over the `DockArea`
  from `dock_size`, with no bounds bookkeeping.
- `InputState::set_value` emits no `InputEvent::Change`. A test of a search field types into it
  (`simulate_input`), and the field must be drawn (its section shown) to take the keys.
- Focusable views hold a `FocusHandle` (`cx.focus_handle()`), render `.track_focus(&self.focus)` and implement `Focusable`.
- A modal that closes on focus-out must not give the focus back on that path: whatever took it (the palette, F6)
  keeps it. Restore the saved focus only when the focus is still inside the modal or nowhere (Esc, ×, backdrop).
- A read-only gpui-component text field drops ⌘↩ instead of emitting `InputEvent::PressEnter`, so a submit hung on
  that event stops working while it is read-only: bind the send action in a key context set only then
  (`ComposerDictation > Input`, `composer_voice.rs`).
- A textarea with `submit_on_enter` handles a plain Enter by calling `cx.propagate()`, so if nothing above it handles
  `input::Enter` the keystroke falls through to text input and still types a new line. Put a no-op
  `.on_action(|_: &input::Enter, ..| {})` on an ancestor (the composer card, `composer.rs`), and update
  `set_submit_on_enter` on every Change: the field has no getter, and the composer's send shortcut depends on the text.
- A popup the workspace draws (even through `deferred`) still has the Workspace on its key dispatch path, and key
  bindings run before any element's `on_key_down`. So the workspace's `escape` → `FocusComposer` takes Escape before an
  open menu's own handler sees it, whenever a composer is on screen. Close such a popup from the bound action
  (`focus_composer` closes the machine menu, as it closes the details card) and test Escape with a thread open, not
  only over the Welcome page.
- The palette lists an action only when a handler sits on the focused element's dispatch path
  (`Window::is_action_available_in` reads the rendered dispatch tree, not App-level `cx.on_action`). A new palette
  action goes on the workspace (`render.rs`), and its test reads `palette_entries` from a real workspace
  (`palette_gallery_test.rs`), never only `all_action_names`.
- Moving focus to something that just appeared (a docked question): do it from `render` once per item, remembering its
  id (`ComposerState::dock_focused`), never on every render, or a click elsewhere is taken back the next frame.
  `window.focus` while drawing is safe: `refresh` is a no-op mid-draw. Don't steal it from a text field holding text.
- `window.focus` sets `window.focused()` at once, not at the next frame, so when several views may take the focus in
  one draw (side chat panes on a Compare page each getting a question), "only while nothing is focused" lets the first
  one win and keeps the rest from stealing it back (`focus_new_question`).
- To override a gpui-component widget's own key action (Command's Escape first clears the query), wrap it in a div
  with `.capture_action::<gpui_base::actions::Cancel>` and `cx.stop_propagation()`: capture runs root-down before the
  widget's bubble handler (`palette_view.rs`). Don't call a widget's `set_query` inside your own view's `update` when its
  query callback updates that view: do it after the update (`show_page`).
- A dock panel that wraps another view (a side chat around a `ThreadView`) must return that view's focus handle from
  `Focusable`: F6 and the dock focus a panel by that handle, and a handle of the wrapper's own leaves the inner
  view's key context (`Thread`) off the dispatch path, so mod+F, Esc and the rest do nothing (`side_chat.rs`). A click
  on a dock tab focuses the tab button itself, not the panel: keys need a click inside the panel.
- gpui-component's `EditorState::new` is `searchable(true)`: mod+F opens the library's own search panel. For our find
  bar set `.searchable(false)` (the editor then propagates its `Search` action, and GPUI tries the next binding for the
  key, ours in an outer context) and drive the editor's matcher with `set_search_query` / `next_search_match`, which
  highlights only the visible matches (`files/preview.rs`).
