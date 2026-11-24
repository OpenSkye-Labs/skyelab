# GPUI rules

GPUI is pre-1.0 and its API breaks often. These rules keep us on one consistent version and out of the known traps.
When in doubt, the source of truth is the pinned crate's own code and examples (`~/.cargo/registry/src/*/gpui-pre-0.3.14/`)
and how Zed uses it, not blog posts or memory.

GPUI now lives in its own repo, https://github.com/zed-industries/gpui (it used to be `crates/gpui` in Zed), and may
publish its own crate releases as crates.io `gpui`. Read it there for GPUI itself, and Zed's `crates/ui` for how a
component crate is built on it. We still follow gpui-component's pin (`gpui-pre`, a snapshot of that code) until
gpui-component moves to the official `gpui` crate; then we move with it.

## Version pin

- We use **`gpui-pre` =0.3.14** (a snapshot of GPUI main, aliased as `gpui`) + `gpui-pre-platform`, because
  `gpui-component` 0.11 pins exactly that (with `gpui-base`, `gpui-kit-assets` and `gpui-webview` at the same 0.11).
  crates.io `gpui` 0.2.x is a year stale; don't use it.
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
- An entity lives while anything holds a strong `Entity` to it (another view, a test, a task), so dropping your own
  handle doesn't run its `Drop`. A resource that must end when the user leaves it (a remote machine's forwarded ports
  and their listeners) is released explicitly at that point (`switch_environment` calls `drop_forwards`), not only
  in `Drop`.

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

See [gpui/elements.md](gpui/elements.md).

## Redraw discipline

See [gpui/redraw.md](gpui/redraw.md).

## Several windows

See [gpui/windows.md](gpui/windows.md).

## Keyboard and focus

See [gpui/keyboard.md](gpui/keyboard.md).

## Native webviews

See [gpui/webview.md](gpui/webview.md).

## Testing

- Logic goes in `skyelab-core` and gets plain `#[test]`s. Views stay thin.
- View behavior: `#[gpui::test] fn t(cx: &mut TestAppContext)` with the `test-support` feature in dev-deps.
  Use `cx.add_window_view`, `simulate_keystrokes`, `run_until_parked`. The executor is deterministic, so no sleeps.
- `VisualTestContext::dispatch_action` runs until parked, so background reads have already landed when it returns. To
  assert an in-between state (a Loading row), call the handler inside one `update_in` and read the state there
  (`gh_pages_test.rs`).
- Pointer tests: give the target `.debug_selector(..)`, get it drawn (`cx.notify()` after changing state directly,
  then `run_until_parked`), and drive it with `cx.debug_bounds("..")` and `simulate_mouse_down/move/up` at its center.
  `debug_selector` records only with debug assertions on: `mobile/` turns them off in dev (Android emulator), so its
  `[profile.test]` turns them back on, or every `debug_bounds` is `None`.
  A button's id is not a debug selector: give `ui::Button` its own `.debug_selector(..)`, or wrap it in
  `div().debug_selector(..).child(button)` (`thread_failure.rs`, the account card), or `debug_bounds` finds nothing.
  In a `flex_col` the wrapper stretches across the row: give the column `items_start()` when a test clicks the
  wrapper's center (`ui/button_test.rs`).
- `TestAppContext` runs `background_spawn` work one task at a time on the test dispatcher, so two blocking
  background jobs (two `gh` reads) never overlap there: a held one blocks the other. Stage a race by recording the
  newer side's result (`PrLanded::mark` in `pr_newest_test.rs`), not by holding a fake process.
- Tests that wait on real processes or threads (a fake agent, gh, a shell, a daemon) must hold at load 30–40, where
  a process starts seconds late:
  - Wait for the exact thing you assert, with `crate::test_wait::wait_until` / `wait_ticking` (60 s by the clock).
    Never wait for one event and then read another once: a mode flip, a "busy" flag or the first half of a line
    comes before the reply, the command name or the rest of the line (`questions.rs`, `probe.rs`, `view.rs`).
  - No fixed `sleep` before a positive assert, no poll loop bounded by an iteration count, and no helper that
    quietly returns when it runs out of time: it must fail with what it waited for.
  - Short timeouts belong only to the behavior under test. A test harness's own budgets (the daemon's gate
    timeout, socket read timeouts, `recv_timeout`) stay long; only the test about that timeout shortens it.
  - An "under N s" bound sits halfway between the fast path and the slow path it rules out (a 300 s idle
    timeout, a 20 s hang), never just above the fast path. A lower bound measured from the first event is safer
    still: load only stretches it (`backoff.rs`).
  - Waiting for a fake's output file: wait for the file the assert reads, or for something written after it.
- Visual or behavior check of the real app: `/loop-verify-ui`.

## Platform notes

- macOS needs the Xcode Metal Toolchain (see CLAUDE.md). Linux needs X11 for the webview (WebKitGTK has no Wayland
  embedding in `gpui-webview`). Native webviews draw over GPUI overlays, so don't put menus over a webview
  ([gpui/webview.md](gpui/webview.md)).
- iOS: our pinned `gpui-pre-platform` already includes the experimental `gpui_ios`. Android: no official support; we use
  longbridge's `gpui-pre-mobile`, ported to our pin. A GPUI bump means re-porting it. See [mobile.md](mobile.md).
