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

## Theme (`ui/theme.rs`, `ui/theme_load.rs`, `ui/themes.rs`, `ui/theme_bridge.rs`)

- Views read colors, fonts and sizes from `crate::ui::ActiveTheme` (`cx.theme()`), which is our `ui::Theme`. Never
  import gpui-component's `ActiveTheme` or `Theme`: `kit_boundaries_test` fails on it.
- `theme_load::resolve(json, dark, settings)` builds a `Theme` from a family file with the accent and sizes written in
  (`themes::adjust`). Colors are hex (`#rgb`, `#rrggbb`, `#rrggbbaa`); anything else is a load error naming the theme
  and key, never a silent default. A field a view needs and the files lack goes into every family file first.
- `theme_bridge.rs` is the only writer of gpui-component's theme, fed the same adjusted files, so that crate's widgets
  still in use match ours (`theme_match_test` checks every family, mode and accent). It never reads that theme back.
- Before `install_theme` (a test that doesn't need one), `Theme::global` is the default family's light theme.

## Layout and icons (`ui/layout.rs`, `ui/icon.rs`, `ui/icons.rs`)

- Rows and columns are `crate::ui::h_flex()` (centers children on the cross axis) and `ui::v_flex()` (stretches
  them). Never import gpui-component's or gpui-base's: `kit_boundaries_test` fails on it.
- Icons are `ui::Icon::new(SkyeIcon::…)`: the text size and text color around it unless `.size_*()` / `.text_color()`
  say otherwise, and it never shrinks in a crowded row. Size steps: `size_3` (12 px, was `xsmall()`), `size_3p5`
  (14 px, was `small()`), `size_4`. A new glyph is an SVG in `icons/ui/` plus a line in `icons.rs`.
- `SkyeIcon` implements gpui-component's `IconNamed` only in tests, for the parity tests that draw theirs.

## Menus and popovers (`ui/menu.rs`, `ui/menu_render.rs`, `ui/dropdown.rs`, `ui/popover.rs`)

- A menu is a `ui::PopupMenu` entity (`PopupMenu::build(window, cx, |menu, window, cx| ..)`): rows from `item`,
  `menu` (dispatches an action to `action_context`), `menu_with_check`, `menu_with_icon`, `label`, `separator`,
  `submenu`; `checks_right()` moves checks to the row's end. `PopupMenuItem::element(name, ..)` draws its own row and
  must be named. It emits `DismissEvent` when it closes and gives focus back to what had it when it was built,
  unless a pick moved focus on.
- Keys live in the `SkyeMenu` context (`ui::bind_keys`): Up/Down skip labels, separators and disabled rows and wrap;
  Right opens a submenu (a hover opens it too), Left closes only it; Enter picks; Escape closes the whole chain.
- `Button::dropdown_menu(..)` / `dropdown_menu_with_anchor(..)` open a menu from a button through `ui::Popover`,
  which keeps the menu while open and gives focus back to what had it before the press (a press on a button never
  takes focus, so a pick leaves you typing where you were).
- `ui::Popover::new(id).trigger(|open| ..).content(|window, cx| ..)` draws its content in a deferred layer at
  priority 100 (`menu::POPUP_PRIORITY`, above dialogs), anchored to the trigger with an 8 px window margin; a click
  toggles it, a press outside or Escape closes it. `ui::HoverCard` opens after its open delay on hover and stays
  while the pointer is on the trigger or the card.
- GPUI has no `aria_disabled`: wrap the element last with `a11y_disabled(el, disabled)` (`ui/a11y_disabled.rs`).
- Never import gpui-component's or gpui-base's menu, popover or hover card: `kit_boundaries_test` fails on it.

## Dock (`ui/dock/`)

- Two layers. gpui-base's dock is the engine (layout tree, tab groups, dock sizes, zoom, open/closed state); our
  `ui::dock::Skin` draws it (tab bars, the toolbar with zoom and the "…" menu, resize handles, the closed bottom
  strip) through `DockAreaRenderer` / `TabGroupRenderer`. Stage 5b replaces the engine behind the same names, so the
  app imports every dock name from `ui::dock`, never from gpui-base or gpui-component (`kit_boundaries_test`).
- A panel implements gpui-base's `BasePanel` plus our `ui::dock::Panel` (`title`, `title_suffix`, `dropdown_menu`
  on `ui::PopupMenu`, `zoom_control`, `title_bar`) and is installed with `ui::dock::panel_handle(entity)`; a panel
  added without it draws its panel name and no menu items.
- The "…" menu's Zoom and Close are sent as `ToggleZoom` / `ClosePanel` from the shown panel's focus handle, so a
  panel's own handler (an unsaved preview) runs before the group's. A locked dock (the workspace's) offers no Close.
- Sizes follow gpui-component's, so the app looks the same: the toolbar buttons are 1.25 rem (the app's rem is under
  16 px, so a pixel size moves them), tabs 32 px. `dock_parity_test` draws both skins at a 14 px rem.

## Diff viewer (`workspace/diff_viewer.rs`, `workspace/diff_viewer_select.rs`)

- A whole change to read is a `DiffViewer` entity (`DiffViewer::new(path, lines, breaks, cx)`; the approval's View all):
  the approval card's rows (`diff_view::row_frame`), every line, nothing folded, only the rows in view built. Give it a
  bounded height and the focus.
- Keys live in the `SkyeDiff` context: arrows, Page Up / Down, Home, End, select all, copy. Copy gives code alone,
  tabs as tabs.
- Syntax is parsed in the background, each MultiEdit edit on its own, lines over 1,000 bytes not at all.
- Never import gpui-component's `diff`: `kit_boundaries_test` fails on it. Markdown is still theirs until stage 7b.

## Small pieces (`ui/spinner.rs`, `ui/copy_button.rs`, `ui/status_bar.rs`, `ui/toast.rs`)

- `ui::Spinner` turns `SkyeIcon::Loader` through `with_animation` and `Icon::transform`; GPUI holds it still under
  reduce motion, so it needs no check of its own.
- Parity tests render at a 14 px rem (`window.set_rem_size`), as gpui-component's `Root` does in the app: a fixed
  `px` size can match theirs at the test's default 16 px rem and still be off by 2.5 px on screen.
- `ui::CopyButton` keeps its copied flag in `window.use_keyed_state` under its id, so give each one an id unique among
  its siblings; one flag picks the check icon and the "Copied" description.
- `ui::StatusBar` centers its middle children only when both ends hold items, pushes them to the end after a left
  alone, and starts them otherwise.
- OS notifications: GPUI keeps one response handler for the whole app, the last one registered. `ui::init` registers
  ours (`toast::init_system`) after `gpui_component::init`, and `lib.rs` calls only `ui::init`; a test reads `lib.rs`
  for it. Our tags start `skyelab/notification/`; a click on any other tag runs nothing, and only the newest 100 are
  remembered.
- Never import gpui-component's or gpui-base's spinner, clipboard, status bar or notification: `kit_boundaries_test`
  fails on it. The command list (the palette, the file picker) is still theirs until stage 4b.

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
