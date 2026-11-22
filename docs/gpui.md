# GPUI rules

GPUI is pre-1.0 and its API breaks often. These rules keep us on one consistent version and out of the known traps.
When in doubt, the source of truth is the pinned crate's own code and examples (`~/.cargo/registry/src/*/gpui-pre-0.3.12/`)
and how Zed uses it, not blog posts or memory.

## Version pin

- We use **`gpui-pre` =0.3.12** (snapshot of Zed main, aliased as `gpui`) + `gpui-pre-platform`, because
  `gpui-component` 0.10 pins exactly that. crates.io `gpui` 0.2.x is a year stale; don't use it.
- All three are pinned in `[workspace.dependencies]`. **Bump them together, in one PR, and nothing else in it.**
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
- Text input is not built into GPUI. Use `gpui_component::input`, never hand-roll `EntityInputHandler`.

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
- iOS: upstream has an experimental `gpui_ios`, which isn't in our pinned snapshot. Android: no official support.
  See architecture.md for the mobile plan.
