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
  stretches across the footer. Use a plain `Button` whose `on_click` calls `window.close_dialog(cx)`.
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
- Blocking or O(n) work (store writes, ranking thousands of rows, polling a CLI) goes to `background_spawn`, and waits
  use `background_executor().timer`, never `std::thread::sleep` on the pool.
- Subscriptions to a view that can close live in a map keyed by its `EntityId` and are removed when it closes. A
  one-shot wait removes its own `Subscription` once it fires.

## Keyboard and focus

- Commands are `actions!(skyelab, [NewThread, ..])`, bound with `KeyBinding::new("cmd-n", NewThread, Some("Workspace"))`. `KeyBinding::new` panics on `secondary-` (it has no OS to resolve it): use `KeyBinding::load(.., cx)` for those.
  Use `secondary-` (cmd on mac, ctrl elsewhere) for cross-platform bindings.
- Handlers: `.key_context("Workspace").on_action(cx.listener(Self::new_thread))`. Every user command is an action,
  so it is bindable and shows in the command palette. No ad-hoc `on_key_down` for commands.
- Focusable views hold a `FocusHandle` (`cx.focus_handle()`), render `.track_focus(&self.focus)` and implement `Focusable`.

## Testing

- Logic goes in `skyelab-core` and gets plain `#[test]`s. Views stay thin.
- View behavior: `#[gpui::test] fn t(cx: &mut TestAppContext)` with the `test-support` feature in dev-deps.
  Use `cx.add_window_view`, `simulate_keystrokes`, `run_until_parked`. The executor is deterministic, so no sleeps.
- Visual or behavior check of the real app: `/loop-verify-ui`.

## Platform notes

- macOS needs the Xcode Metal Toolchain (see CLAUDE.md). Linux needs X11 for the webview (WebKitGTK has no Wayland
  embedding in `gpui-webview`). Native webviews draw over GPUI overlays, so don't put menus over a webview.
- iOS: our pinned `gpui-pre-platform` already includes the experimental `gpui_ios`. Android: no official support; we use
  longbridge's `gpui-pre-mobile`, ported to our pin. A GPUI bump means re-porting it. See [mobile.md](mobile.md).
