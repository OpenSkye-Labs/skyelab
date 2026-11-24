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
- A native view can't be partly clipped without `gpui-fast` composition: `set_geometry` hides it only when it is
  wholly outside the content mask and otherwise shows all of it, over whatever is around. So never put a webview in a
  scrolling list (the timeline). Visual replies draw a snapshot image there, taken from a hidden webview, and the live
  page only in a modal dialog. A hidden (`set_visible(false)`) WKWebView still loads, runs scripts and snapshots
  (`takeSnapshotWithConfiguration`) at the bounds you give it, so it can render offscreen.
- A dialog that hosts a webview hides it when `window.has_active_dialog` turns false: a closing dialog is still drawn
  while it fades, and the native page would sit over the backdrop until it is gone.
- wry calls `with_navigation_handler` for every frame, not only the main one, so a scheme allowlist must keep the
  in-page documents frames use (`about:blank`, `about:srcdoc`, `blob:`, `data:`). Without a
  `with_download_started_handler`, WKWebView on macOS still saved an attachment response to ~/Downloads (and
  WebView2 saves by default), so a tab that must not download sets one that returns `false`. WebKit drops a
  script's `window.open` without a user gesture before wry's new-window handler sees it.
- An agent `eval_js` expression is wrapped as one expression: a test that navigates must write
  `(setTimeout(.., 0), 1)`, never `a; 1`, and let the reply come back before the page leaves.
- Drop a page's in-flight task (find, eval) when a new page commits: the callback still answers about the old page,
  and landing it showed "No matches" on the new one.
- wry reports only Started and Finished, so a load that fails before it commits (connection refused, DNS, TLS) left a
  blank, black WKWebView. `skyelab_webkit_sys::watch_load_failures` puts a proxy in front of wry's navigation delegate
  that answers `didFailProvisionalNavigation`/`didFailNavigation` and hands every other call to wry's
  (`respondsToSelector:` + `forwardingTargetForSelector:`). WebKit asks `respondsToSelector:` once, when the delegate is
  set, so the proxy is set after wry's and keeps both answers; WKWebView holds its delegate weakly, so the
  `FailureWatch` keeps the proxy and gives wry's back on drop. The failing URL is `NSErrorFailingURLKey`, an `NSURL`
  (there is no `NSErrorFailingURLStringKey` in WebKit's errors). Ignore `NSURLErrorCancelled` (-999, the next load took
  over) and WebKit's 102 (a download). `wkwebview_load_error` proves both, on a real page.
- The error page is GPUI, so the page is hidden and its element not drawn while it shows: drawing the webview's element
  lays it out again and can put the native view back on top. The next commit (`Started`) clears it and shows the page.
