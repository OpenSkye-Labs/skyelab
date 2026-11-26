# Menus and hover overlays

Rules for popups and hover-only controls, each with the bug that taught it.

- **A `dropdown_menu`/`Popover` inside a `list()` row never paints its menu**: its open state flips (the
  `on_open_change` callback fires) but nothing draws. Own the `PopupMenu` in the list's view and draw it there with
  `deferred(anchored().position(..))`, closing on its `DismissEvent` (the sidebar's snooze menu, `Sidebar::open_snooze`).
- **`.invisible()` still records `debug_selector` bounds but registers no mouse handlers**: a hover-only overlay
  (`group_hover(.., visible)`) takes no `simulate_click` in tests even though `debug_bounds` finds its button. Test
  what the click does by calling it directly, and check the click itself in the running app.
- **A popover's search field holds the focus after the popover is gone** unless closing hands it back: record where
  the focus goes (the composer's prompt, not whatever had it, since a click on the trigger button may own it) and
  restore it in a `cx.defer` only while the hidden field is still focused (`composer_picker_keys::close_picker`).
- **Reset a list highlight on the field's `InputEvent::Change`, not on `cx.observe`**: an `InputState` notifies on focus
  and on every cursor blink, so an observer resets the highlight all the time.
