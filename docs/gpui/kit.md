# GPUI rules: SkyeLab's own UI kit

Part of [docs/gpui.md](../gpui.md). SkyeLab draws every control from its own kit in `crates/skyelab-app/src/ui/`, which
we design and improve ourselves. gpui-component and gpui-base are on their way out, stage by stage
(`.agent/plans/2026-11-27-own-ui-kit.md`): when one of their pieces falls short, replace it with ours rather than
patching around it.

## How a kit component is built

- **Stateless controls are `RenderOnce` builders** (`Button::new(id).label(..).on_click(..)`), as Zed's `crates/ui`.
  Anything that owns state across frames (an open dialog stack, a menu's highlighted row, a text buffer, a dock
  layout) is an `Entity` that implements `Render`, made with `cx.new`, and calls `cx.notify()` only when its state
  changed.
- **Ids:** an element that is interactive or holds state has an `ElementId`, unique among its siblings; a list item
  takes its index or its own key (`.id(("row", key))`). Ids never come from display text alone: two siblings showing
  the same text would share an a11y node id (accessibility.md, "Groups of the same placeholder").
- **Keyboard:** each keyboard behavior is an `actions!` action bound in the component's own key context
  (`KeyBinding::load` for `secondary-` keys), handled with `.on_action(..)`; focus goes through `FocusHandle`s
  (`track_focus`, `Focusable`) and `focus_handle.dispatch_action`. Name the context after the component
  (`SkyeDialog`), not after a gpui-base one, which binds its own keys.
- **Overlays** draw through `deferred(..).with_priority(..)` and `anchored(..)`: SkyeLab's own overlays 1–2, dialogs 50,
  popups and toasts 100. Nothing positions an overlay by hand from window coordinates it guessed.
- **Accessibility** is part of the component: role, name, state, a Click action (accessibility.md). Register what tests
  read with `gpui_base::TestSupportExt::test_support()` (before `track_focus`) until stage 8 brings our own helper.
- **Tests** are `#[gpui::test]` with `TestAppContext` / `VisualTestContext`. Behavior is tested through the keystroke or
  the action, never by calling the handler. A behavior a test guards is shown to fail once by breaking it (a mutation
  probe) before the test counts.
- **References:** GPUI's own source and examples (Apache-2.0) for how; Zed's `ui`, `workspace` and `editor` crates are
  patterns to learn from, not to copy (GPL-3.0); T3 Code for the look (CLAUDE.md). gpui-component's source may be read
  for behavior details; nothing is pasted from it.

## Windows

Every window, test windows included, is built with `ui::window_root(content, window, cx)`: gpui-component's `Root`
(until stage 8) around our `WindowRoot`, which draws the window's content and our layers over it.

## Dialogs (`ui/dialog.rs`, `ui/dialog_layer.rs`)

- Open one with `ui::open_dialog(window, cx, |dialog, window, cx| dialog.title(..).w(..).child(..).footer(..))`; the
  closure runs every frame while it is open. Close the top one with `ui::close_dialog`; `ui::has_active_dialog` says
  whether one is up. Clippy refuses gpui-component's `Dialog` and its `WindowExt` dialog calls.
- The window's `DialogLayer` owns the backdrop (a click closes the top dialog), Esc, the focus trap (Tab / Shift-Tab step
  the window's tab stops until focus is back inside), focus moved in on open and given back on close, clearing a text
  selection behind it, and the fade (`motion::FADE` in, `FADE_OUT` out; `DialogEntrance::None` for none).
- A dialog is a `Role::Dialog` node named by its title, and its ✕ is "Close <title>".
- Closing never runs the dialog's build closure: it is often closed from inside an update of the entity that closure
  reads. `on_close` and the entrance come from the dialog's last frame, so a dialog closed before it ever drew doesn't
  call its `on_close`.
