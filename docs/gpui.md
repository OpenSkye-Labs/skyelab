# GPUI rules

GPUI is pre-1.0 and its API breaks often. These rules keep us on one consistent version and out of the known traps.
When in doubt, the source of truth is the pinned crate's own code and examples (`~/.cargo/registry/src/*/gpui-pre-0.3.12/`)
and how Zed uses it, not blog posts or memory.

## Version pin

- We use **`gpui-pre` =0.3.12** (snapshot of Zed main, aliased as `gpui`) + `gpui-pre-platform`, because
  `gpui-component` 0.10 pins exactly that. crates.io `gpui` 0.2.x is a year stale; don't use it.
- All three are pinned in `[workspace.dependencies]`. **Bump them together, in one PR, and nothing else in it.**
- The same PR re-ports gpui-mobile: raise the same pins in `mobile/Cargo.toml`, regenerate the patches in
  `mobile/upstream/` against the new `gpui-pre` (see [mobile/UPSTREAM.md](../mobile/UPSTREAM.md)), and build `mobile/` for
  iOS and Android. The mobile CI workflow checks both.
- Never add a second GPUI (a git dep on zed, `gpui-fast`) alongside. Two GPUIs = two `App` types = compile errors.

## Mental model

- `App` owns all state. Views are `Entity<T>` created with `cx.new(|cx| T { .. })`, and `T: Render`.
- `render(&mut self, window, cx)` is a pure function of state. It runs on every `cx.notify()`. Keep it cheap:
  no I/O, no sorting big lists, no allocation you can cache.
- Stateless reusable pieces are `#[derive(IntoElement)]` + `RenderOnce`. Use an `Entity` only when the thing has state
  that outlives a frame (focus, scroll, input text, async work).
- Wrap each window's root view in `gpui_component::Root` and call `gpui_component::init(cx)` once at startup.
  Otherwise dialogs, notifications and theming don't work.

## State and updates

- Mutate via `entity.update(cx, |this, cx| { ..; cx.notify() })`. Call `cx.notify()` only when something visible changed.
- Cross-view communication: `impl EventEmitter<E> for T`, `cx.emit(e)`, `cx.subscribe(&other, ..)`. Prefer events over
  views holding `Entity`s of each other. Keep the returned `Subscription` in a field, or it is dropped and stops firing.
- Shared app-wide state (settings, accounts): `impl Global` and `cx.set_global` / `cx.global::<T>()`, sparingly.
- Never `observe` something that you then `notify` from in the same chain. That's an infinite re-render.

## Async

- UI-thread async: `cx.spawn(async move |this, cx| { ... this.update(cx, |this, cx| ..)?; })`. `this` is a
  `WeakEntity`, and `update` fails if the view is gone. Propagate that, don't `unwrap`.
- CPU/blocking work: `cx.background_spawn(async move { .. })`, then hop back with `this.update`.
- **Never hold a `read`/`update` borrow across an `.await`.** Copy out what you need, await, then update.
- A `Task` is cancelled when dropped. Store it in a field (`_task: Task<()>`) to tie its life to the view, or `.detach()`
  only for true fire-and-forget. Agent processes, PTYs and network streams belong to a stored task, never a detached one.
- A callback that has no `Window` (an observer) but must call something that needs one (`submit_remote`): leave the
  callback with `cx.defer`, then `window_handle.update(cx, |_, window, cx| ..)` (`side_chat::ask_when_ready`).
- Don't use tokio inside GPUI views. GPUI has its own executor. If a crate needs tokio (e.g. a server), run it on its
  own runtime thread and talk to the UI through channels.

## Elements

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
- Text input is not built into GPUI. Use `gpui_component::input`, never hand-roll `EntityInputHandler`.
- `img()` takes its height from its width and the image's aspect ratio, even with `.h(..)` set: a wide box draws a
  square image as tall as the box is wide. Cap it with `.max_h(..)` (the image viewer does), and check a fit in a live
  screenshot, not only by reading the layout.
- `Textarea::on_paste` sees the `ClipboardItem` before the text goes in; returning `true` keeps it out (long pastes
  become attachments that way).
- `ListState::reset_with_uniform_height` gives every item the same height, so a row that grows (a comment under a
  diff line) is clipped. Use the measured `reset` while any row is taller, and lay the list out again (keeping the top
  row) whenever that set changes (`ChangesView::comments_changed`).
- A hit target over part of a row (the diff gutter) is an `absolute` child with its own id and mouse handlers. A plain
  `absolute().inset_0()` div with no handlers (a selection tint) draws over the row without taking its clicks, so the
  row's own colors still show under it.

## Redraw discipline

- A `cx.notify()` re-renders the view **and every ancestor view** up to the window root (`Window::mark_view_dirty`).
  A cached view (a dock panel) is reused only while nothing under it notified. So moving a ticking label into its own
  entity doesn't spare its parent's render; keeping the parent's render cheap does (a virtualized `list`).
- `cx.observe(&view, ..)` fires on every notify of that view: each streamed delta, keystroke and timer tick. Don't
  rebuild derived models from it. Emit a specific event when the thing you show changes, or compare against the last
  value you drew and return early (the sidebar keeps a `RowSig` per open thread).
- A per-second tick lives in the smallest view that shows the time, runs only while something ticks, and only
  notifies. It never rebuilds a model.
- `render` only reads. No `entity.update`, no `set_value`, no restoring state: do that where the state changes (an event
  handler, the task that got the result). To draw another entity's content, read it (`view.read(cx)`) and build its
  click handlers from a `WeakEntity` (see `thread_details::On`).
- Per-frame work is per visible row. Borrow items, don't clone them. Cache what's derived from text (linkified
  markdown) in element state (`window.use_keyed_state`), keyed by the element's id.
- Something slow to derive that a frame needs (a drawn diagram) is a `gpui::Asset` read with `window.use_asset`: it
  starts once per source, is cached for the session and redraws the view when it lands. A render that must be
  capped in time runs on its own thread, raced against a `background_executor().timer`, so a runaway never holds a
  pool thread. The timeout only stops the wait, not the thread: gate those threads with a small semaphore and skip
  the work when the receiver is already gone, or a reply full of diagrams starts one CPU-bound thread each
  (`mermaid::Gate`). Never do that work in a markdown block parser: gpui-component parses small texts on the UI thread.
- A poll that lands every few seconds (PR status) notifies its views only when what it read differs from what they
  show; most polls find nothing new, and each notify rebuilds the sidebar.
- Custom markdown blocks (the mermaid card) go through `TextView::markdown_block_parser` / `markdown_block_renderer`;
  equal parser counts keep the parsed document across frames. Match the plain code block by reading the same tokens:
  `theme.muted` background and `theme.semantic_tokens().radius.md`, not `theme.accent`.
- `img(path)` caches the decoded image by path for the whole session: a file rewritten on disk keeps drawing its old
  pixels. For a file the user or agent may change (Preview, the image viewer), read the bytes off the main thread and
  pass an `Arc<gpui::Image>` (`Image::from_bytes` keys it by a hash of the bytes).
- Work queued for a GPUI `Asset` load can't be cancelled when its view goes: the asset cache owns the future. Bound it
  instead (a fixed worker pool, a timeout after which queued jobs are skipped), never a thread per load.
- Never hand untrusted SVG to GPUI's `img()`: its usvg loads `<image href>` from any path. Rasterize with our own
  resvg `Options` whose `image_href_resolver` refuses both data and string hrefs, and hand GPUI the PNG.
- Blocking or O(n) work (store writes, ranking thousands of rows, polling a CLI) goes to `background_spawn`, and waits
  use `background_executor().timer`, never `std::thread::sleep` on the pool.
- A view drawn from another entity (the Details tab reads the thread view) observes it but compares a signature
  of what it draws (`DetailsSig`: every field `render_details` reads, plus the minute for "3m" labels) and notifies
  only when it differs. GPUI coalesces notifies per flush, so the observer runs once per frame at most.
- Element ids and keyed element state of list rows use a key the row keeps (`thread_model::Items` gives each row one
  from a global counter), never the row's index: the timeline is spliced, and an index id hands a row's markdown
  cache, selection or open fold to its neighbour. A list built again gets new keys, so stale state never attaches.
- Measure redraws with `SKYELAB_DEBUG_RENDERS=1` (debug builds): every view's `render` calls
  `crate::debug_renders::count("Name")`, and stderr gets `renders/s Workspace=17 ThreadView=16 timeline row=144 …`
  once a second. `Workspace` is the frame count per window. A new view adds the call as its render's first line.
  Measured: the focused composer's caret blink re-renders `ThreadView` (and rebuilds its visible timeline rows)
  twice a second while idle; only moving the timeline into its own cached view would stop that.
- Subscriptions to a view that can close live in a map keyed by its `EntityId` and are removed when it closes. A
  one-shot wait removes its own `Subscription` once it fires.

## Several windows

- One entity can be drawn in two windows (a shared thread view), and a notify redraws both. But subscriptions made
  with `subscribe_in`/`on_focus`/`on_blur` (gpui-component's `InputState` makes them in `new`) run in the window that
  made them and die with it. A shared view re-creates its window-bound parts when another window takes it over
  (`ThreadView::attach`), and only one window (its owner) acts on its events, or two windows open two tabs.
- A shared view's `ListState` (scroll and row heights) would make both windows scroll together, so it keeps one per
  window (`thread_window::Lists`): splices and resets reach every list, `render` draws `window`'s, and a scroll the
  view starts (send, find, a failure) moves only its owner's. Add a window's list where the window starts showing
  it and drop it in `on_window_closed`.
- Never draw one entity's geometry-reporting element in two windows: gpui-component's input notifies whenever its
  drawn size changes, and a canvas that notifies on a width change does the same, so two windows of different sizes
  redraw each other forever (a test hangs in `flush_effects`; the app burns a core). The owner window draws the live
  input; another draws the draft as text and takes the prompt over on click (`render_borrowed_prompt`).
- Work that answers an outside caller (a delegated start's reply) never lives in one window's view: a
  `Global` holds the state and App-level `cx.observe` waits, so closing the window that took the call loses
  neither the answer nor the request id → thread map.
- gpui-component 0.10's input keeps its undo stack private (`InputBaseState::undo_manager` is `pub(super)`, no
  getter or setter), and its focus and blink hooks are bound to the window that made it. A prompt moving windows is
  a new `TextareaState` with the same text: the undo history starts over (Undo then does nothing, never empties it).
  Keep anything else the draft has (files, chips) on the view, not in the input.
- Code that runs inside `ws.update(..)` must not read every workspace entity (that one is mid-update and panics):
  keep cross-window data (each window's saved layout) in the registry global instead.
- Find a closing window with `cx.on_window_closed`, not the root view's `on_release`: a test or a task holding the
  entity keeps it alive after its window is gone. Inside that callback `cx.windows()` no longer lists it.
- A window whose focused element moved to another window has no focus, so its key bindings stop: give the page the
  focus back when the window activates.
- `on_reopen` (the Dock icon with no window open) is on `Application`, before `run`, not on `App`. The last-window
  rule is GPUI's own `QuitMode::Default`: quit off macOS, keep running on macOS; don't add a hook for it.

## Keyboard and focus

- Commands are `actions!(skyelab, [NewThread, ..])`, bound with `KeyBinding::new("cmd-n", NewThread, Some("Workspace"))`. `KeyBinding::new` panics on `secondary-` (it has no OS to resolve it): use `KeyBinding::load(.., cx)` for those.
  Use `secondary-` (cmd on mac, ctrl elsewhere) for cross-platform bindings.
- Handlers: `.key_context("Workspace").on_action(cx.listener(Self::new_thread))`. Every user command is an action,
  so it is bindable and shows in the command palette. No ad-hoc `on_key_down` for commands.
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

## Testing

- Logic goes in `skyelab-core` and gets plain `#[test]`s. Views stay thin.
- View behavior: `#[gpui::test] fn t(cx: &mut TestAppContext)` with the `test-support` feature in dev-deps.
  Use `cx.add_window_view`, `simulate_keystrokes`, `run_until_parked`. The executor is deterministic, so no sleeps.
- Pointer tests: give the target `.debug_selector(..)`, get it drawn (`cx.notify()` after changing state directly,
  then `run_until_parked`), and drive it with `cx.debug_bounds("..")` and `simulate_mouse_down/move/up` at its center.
- Visual or behavior check of the real app: `/loop-verify-ui`.

## Platform notes

- macOS needs the Xcode Metal Toolchain (see CLAUDE.md). Linux needs X11 for the webview (WebKitGTK has no Wayland
  embedding in `gpui-webview`). Native webviews draw over GPUI overlays, so don't put menus over a webview.
- iOS: our pinned `gpui-pre-platform` already includes the experimental `gpui_ios`. Android: no official support; we use
  longbridge's `gpui-pre-mobile`, ported to our pin. A GPUI bump means re-porting it. See [mobile.md](mobile.md).
