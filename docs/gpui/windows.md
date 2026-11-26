# GPUI rules: Several windows

Part of [docs/gpui.md](../gpui.md); the same rules apply.

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
- gpui-component's input (0.10; gpui-base in 0.11) keeps its undo stack private (`InputBaseState::undo_manager` is `pub(super)`, no
  getter or setter), and its focus and blink hooks are bound to the window that made it. A prompt moving windows is
  a new `TextareaState` with the same text: the undo history starts over (Undo then does nothing, never empties it).
  Keep anything else the draft has (files, chips) on the view, not in the input.
- Code that runs inside `ws.update(..)` must not read every workspace entity (that one is mid-update and panics):
  keep cross-window data (each window's saved layout) in the registry global instead.
- App-wide timed work (the scheduler) is one `Global` holding one `cx.spawn` task, started idempotently by whichever
  window claims the hub; each tick looks up the hub window then (`windows::hub_workspace`), so a closed window never
  takes the work with it and a second window never starts a second loop.
- `window.update(..)` fails ("window not found") when called inside that same window's update: a click handler, a
  view's `update_in`, `VisualTestContext::update`. Work that opens tabs on the hub window runs from the App level: a
  click hands it over with `cx.defer`, and a test drives it with `TestAppContext::update(cx, f)`, not `cx.update`.
- Find a closing window with `cx.on_window_closed`, not the root view's `on_release`: a test or a task holding the
  entity keeps it alive after its window is gone. Inside that callback `cx.windows()` no longer lists it.
- A window whose focused element moved to another window has no focus, so its key bindings stop: give the page the
  focus back when the window activates.
- `on_reopen` (the Dock icon with no window open) is on `Application`, before `run`, not on `App`. The last-window
  rule is GPUI's own `QuitMode::Default`: quit off macOS, keep running on macOS; don't add a hook for it.
- An `observe` that must act in a window (a notice when an observed view starts waiting on the user) uses
  `cx.observe_in(&view, window, ..)`: plain `observe` gets no `Window`, and `notify_attention` needs one.
- App-wide work that must outlive the windows (the scheduler's tick) runs on `cx.spawn` from `App`, keeps what it
  needs (the store) in a `Global`, and acts through `windows::hub_workspace`. With no visible window the hub is a
  hidden window (`WindowOptions { show: false, focus: false }`, `workspace::background`): opened when the last visible
  window on a store closes (macOS, or Keep running in background) and closed when a visible one registers, which takes
  the hub and the orphaned thread views. Hub calls, `send_prompt` and `push_system` need a `Window` and work in a
  hidden one; nothing else may treat it as a window: `window_handles` and the saved layouts skip it (its layout save
  would empty the saved list), so every "is a window open" check means a visible one.
- A hidden window counts in `cx.windows()`, and `QuitMode::Default` quits off macOS only when that is empty: open the
  hidden hub off macOS only when the app must stay up (background mode), or closing the last window never quits.
  `App::shutdown` clears windows without running `on_window_closed`, so no hub opens while quitting.
- A system notification's click runs `window.activate_window()` on the window that posted it before its `on_click`,
  which would show the hidden hub: from the hidden hub, the click opens a visible window (deferred, App level) and
  shows the thread there; the visible window's registration then removes the hidden one.
- gpui-macos sets no `applicationShouldTerminate:`, so logout and `terminate:` quit at once (on_app_quit hooks run);
  our quit dialog is only on the Quit action, and a hidden window doesn't hold up logout either.
- The macOS Dock icon's built-in Quit sends `terminate:` and skips the quit dialog; `set_dock_menu` adds our own
  "Quit SkyeLab" (the Quit action), which asks.
- In tests, a `VisualTestContext` whose window was removed panics on `update` ("window not found"); after
  `window.remove_window()` reach the app through `TestAppContext::update(cx, |cx| ..)`, and open a new
  `VisualTestContext::from_window` for a window opened later.
- An app-level action bound to a key (⌘Q's `cx.on_action`) runs inside the focused window's update, so that window
  can't be updated from the handler: open its dialog with `cx.defer`, and treat a `window.update` that fails there as
  "can't tell", not "no dialog" (`quit_guard::asking`). Tests send such actions with `TestAppContext::update`.
- Windows chrome (`ui::window_controls`): no title strip. The caption buttons are drawn at the window's top-right over
  the 52 px top row (as macOS puts its traffic lights in that row's start), and each top row that can sit under them
  reserves `WINDOW_CONTROLS_W` (the page header while the right dock is closed, the right dock's tab bar). Mark only
  empty top-row space as the caption (`ui::drag_area`): gpui-windows answers `HTCAPTION` for any point whose hit test
  includes a Drag hitbox, a button inside it only `prevent_default`s its press (not `stop_propagation`), so the press
  moves the window instead of clicking. A clickable child of a drag area goes in `ui::no_drag` (occludes it). Check the
  layout on macOS with `SKYELAB_VERIFY_WINDOWS_CHROME=1` (debug builds; the buttons are drawn, inert).
- A full-height element left of the dock (the left rail) shifts every window-x the dock's children see: compute drag
  positions from the dock container's `e.bounds.left()`, not the viewport, and start overlays that must leave the rail
  clickable (the Settings modal) at `left(RAIL_W)`, not `inset_0()`. On macOS the traffic lights reach past the 48 px
  rail, so the rail's top `TOP_ROW_H` band has no border or icons and matches the surface beside it.
