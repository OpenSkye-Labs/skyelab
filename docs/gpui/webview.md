# GPUI rules: Native webviews

Part of [docs/gpui.md](../gpui.md); the same rules apply. The browser tab is `skyelab-browser` on `gpui-webview` (wry).

- The page is a native view stacked above GPUI, so it covers any GPUI overlay in its bounds. A bar that belongs to
  the page (find in page) is a row above it that pushes it down, never an absolute overlay. A menu that must open
  over it (the tab's ⋯ menu) hides the page while open: gpui-component's `dropdown_menu(..).on_open_change` reports
  both edges. Keep the host's wish (`set_visible` from the dock) apart from the menu's (`shown && !menu_open`), or
  closing the menu shows the page of a tab that went inactive meanwhile.
- A right-click inside the page goes to the engine, never to GPUI, and wry has no hook to add items to its menu.
  Our commands live in the toolbar's ⋯ menu; the page keeps the engine's own menu (WebKit: Reload, Inspect Element).
- WKWebView's Web Inspector opens docked and lays itself out across the whole host window, stretching the page over
  the GPUI views beside the pane. Move it to its own window with `_WKInspector detach`
  (`skyelab_webkit_sys::detach_inspector`), and only after its frontend loads (about 0.5 s after `open_devtools`):
  a `detach` right after `show` does nothing.
- wry has `open_devtools` only with `debug_assertions` (its `devtools` feature calls private API on macOS), so the
  menu item and its handler are debug-only too.
- Find in page is the page's own `window.find(q, false, backwards, wrap)` (WebKit and Chromium): it selects and
  scrolls like WKWebView's `findString`, with no FFI. Quote the query with `serde_json` (a JSON string is a valid JS
  literal). gpui-component's input emits a `Change` when Enter is pressed even though the text didn't change, so a
  find that restarts on `Change` must compare with the query it searched last, or Enter never moves on.
- A script whose result comes back through `evaluate_script_with_callback` must return a value it made itself
  (`window.find(..) === true` inside `try`/`catch`), never one a page function returns: the page owns `window.find`
  and every prototype, and wry's macOS bridge `unwrap()`s the result's JSON encoding, so `NaN` or a lone surrogate
  aborts the whole app (it did, with `window.find = () => NaN`). The page also sees every query it is handed.
- A menu left open when its pane stops being drawn is dropped without `on_open_change(false)`: clear the
  menu-open flag when the host hides the pane, or the page stays hidden when the tab comes back.
- Zoom with `gpui_webview::WebView::set_zoom`, not wry's `zoom`: on Linux it also makes up for GTK's integer scale.
- Drop a page's in-flight task (find, eval) when a new page commits: the callback still answers about the old page,
  and landing it showed "No matches" on the new one.
