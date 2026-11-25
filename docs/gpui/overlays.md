# Menus and hover overlays

Rules for popups and hover-only controls, each with the bug that taught it.

- **A `dropdown_menu`/`Popover` inside a `list()` row never paints its menu**: its open state flips (the
  `on_open_change` callback fires) but nothing draws. Own the `PopupMenu` in the list's view and draw it there with
  `deferred(anchored().position(..))`, closing on its `DismissEvent` (the sidebar's snooze menu, `Sidebar::open_snooze`).
- **`.invisible()` still records `debug_selector` bounds but registers no mouse handlers**: a hover-only overlay
  (`group_hover(.., visible)`) takes no `simulate_click` in tests even though `debug_bounds` finds its button. Test
  what the click does by calling it directly, and check the click itself in the running app.
