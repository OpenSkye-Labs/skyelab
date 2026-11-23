# Decisions: Browser

Part of [decisions.md](../decisions.md). The embedded browser and what agents may do with it. Each entry is dated;
newest at the bottom.

- **Browser, agent access:** agents may only navigate to http/https (no `file://`), because `eval_js` on a file page
  would let an agent read any local file, including other accounts' credentials. `file://` stays available to the user
  typing in the URL bar. (2026-11-23)
- **Browser, localhost for agents:** allowed. `parse_agent_url` takes `http://localhost` and loopback IPs, since a
  dev server is the point of the preview, and the agent process can already reach those over the network itself.
  (2026-11-23)
- **Browser screenshots: the web view's own snapshot, macOS only.** wry has no snapshot API, so
  `skyelab-webkit-sys` holds the one `unsafe` WKWebView `takeSnapshotWithConfiguration` call (nil configuration: the
  view's own bounds). Never a window capture: that needs Screen Recording permission and shows whatever is drawn in
  the tab's rect, which can be the user's content. Linux (webkit2gtk's snapshot returns a cairo surface) and Windows
  (WebView2 `CapturePreview`, COM) answer "not supported yet". (2026-11-23)
- **Browser eval reads its result back with a second script:** wry's macOS bridge aborts the app on a script value
  it can't serialize (`NaN`), and an agent's expression can break out of any text wrapper. So the agent's script
  runs with no result callback and leaves its answer in the page. A fixed script then hands it over as a
  well-formed string, or refuses. Rejected: wry's IPC handler, because any page can post a lone surrogate and wry
  passes the NULL from `UTF8String` to `CStr::from_ptr`. Also rejected: `eval` of the expression as a string,
  because CSP without `unsafe-eval` blocks it. (2026-11-23)
