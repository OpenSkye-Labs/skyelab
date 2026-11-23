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
- gpui-component 0.10's input keeps its undo stack private (`InputBaseState::undo_manager` is `pub(super)`, no
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
