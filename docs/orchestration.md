# Agent orchestration

Part of [architecture.md](architecture.md).

An agent can delegate work to other threads through SkyeLab's own MCP tools: `list_projects`, `start_thread`,
`thread_status`, `read_thread`, `send_message` and `wait_thread`. It can also drive a browser tab of its own with
`browser_open`, `browser_navigate`, `browser_url`, `browser_eval` and `browser_screenshot`, and simulators and
emulators with `device_list`, `device_open`, `device_screenshot`, `device_tap`, `device_type`, `device_press` and
`device_close`. It shows a visual reply in its own thread with `html_render`, after checking it with `html_preview`.

- **Account rule.** A delegated thread runs under its target project's bound account, through the normal launch
  path, pinned to the account the start was approved for. An agent under account A can only list and start in
  projects bound to A. It can read, wait on and message only threads it delegated itself (its descendants), recorded
  under A, and only while they run: a message never restarts a thread that ended or that the user stopped. An unbound
  target is refused; there is never a default account. This keeps a work agent from spending the personal plan, and
  the reverse, and keeps an agent out of the user's other threads.
- **Limits.** Depth 2 at most (a user's thread is depth 0), and 4 children starting or running per thread (a child
  that finished its turn frees its slot). The children limit is claimed in the launch itself, under the store lock
  together with the status write (`child_slots::claim`): on every new child row, every resume of a stopped child,
  and every follow-up that gives an idle child a new turn, whatever sent the prompt (the parent, another device,
  the user). A child counts as busy until the result of its last queued prompt (follow-ups sent mid-turn are
  counted), not the first result after them. A result ends the turns of the prompts it answered: claude names them
  in `user_message_uuids` (a follow-up it folded into the running turn ends with it, a background wake ends none),
  and an older CLI without the list answers one; a Codex thread that fails to open answers every queued prompt. An
  agent that exits ends its row within about a second even if a tool it started still holds its stdout, and that
  tool (in its process group) is ended with it. A refused resume leaves the child as it was and its page says why (the
  refused prompt still shows there, though it was never stored or sent); a Send from another device has
  already been answered Ok by then, but a delegated `send_message` answers only after the child's slot is claimed,
  so one that would pass the cap is refused to the parent. A row counts only while its agent is live: ending, Stop, a failed start, closing the thread (even while it
  starts) and quitting all write a non-busy status, and opening the store at app start marks rows the last run left
  `starting` or `running` as `interrupted`.
- **Starts.** `start_thread` answers with the child's `thread_id` as soon as its row exists, before its agent is up
  (`thread_status` then says `starting`, `running` or `failed`), so a slow start (a big worktree) can't outlast the
  agent's 30 s wait and leave a child it never heard of. It takes an optional `request_id` (at most 128 chars): a
  retry with the same one from the same thread returns the same child, whether the first call is still starting or
  done, and even after the child was closed; the first call's project and prompt win. The ids live in memory, app-wide,
  while the calling thread is open: a start whose window closes mid-start still answers, and a retry served by
  another window gets the same child. A start without one can still outlast the wait while a big worktree is checked out (the
  row is saved after the checkout).
- **Turn result.** Once the thread is done, `wait_thread` adds `turn` next to `thread_id`, `status` and `done`: the
  child's last finished turn as data (`is_error`, `summary`, `duration_ms`, `cost_usd`, `files_changed`), read from its
  stored events by `skyelab_agent::turn_result` (T3 #38874's typed result, without its output schema). `summary` is
  the result text, else the turn's last assistant text, cut to its last 8000 bytes. `files_changed` comes from the
  edit tool calls (Edit, Write, MultiEdit, NotebookEdit, Codex `apply_patch`) since the previous turn that had a
  result, not from git, so a file a shell command changed isn't listed. `cost_usd` is the timeline's own price for
  that turn (its Done row). A turn that ended with no result (the agent crashed or was stopped) comes back as
  `is_error` with the agent's last stderr line, or "The turn ended without a result.", never as the turn before it.
  No `turn` while the child is busy or before its first prompt.
- **Live parent.** Starts and sends run only while the calling thread's agent is live in this window: a parent the
  user stopped, or that was closed, is refused, even for a call it queued before.
- **Current binding.** A launch re-reads accounts.toml right before it spawns the agent and checks the folder
  against that, so a rebind saved while the launch was under way (a worktree being made) is seen. An unreadable file
  binds nothing, and the launch is refused.
- **Background starts.** A delegated child (or a thread another device starts here) never takes the page: its
  sidebar row (and its project's row) gets an unread dot until the user opens it. While the window shows another
  machine, the sidebar gets a "This Mac · N new threads here" row that switches back. Its agent runs meanwhile, and
  delegated sends and Stop still reach it.
- **Transport.** `skyelab-agent::mcp` listens on a unix socket in `<local data dir>/skyelab/run/<pid>/` (0700), or on
  Windows on a named pipe `\\.\pipe\skyelab-<pid>-<64 random hex>` whose DACL lets only the current user open it
  (local clients only, created as the first instance; the proxy opens it only if the process named in it serves it). Each agent session gets a grant: a one-use random token and a
  0600 `--mcp-config` file in the run dir that runs `skyelab --mcp-proxy`. The proxy's first connection consumes the
  token and deletes the file. Ending the session revokes the grant and cuts the connection. Never TCP. The run dir is
  created 0700 (and chmod'ed again, past the umask), the token line must arrive whole within 5 s of connecting, and
  an accept error backs off 100 ms instead of spinning. Run dirs of SkyeLab processes that are gone are removed at
  start. Only `mcp/transport/{unix,windows}.rs` differ per OS.
- **Lost connection.** There is no reconnect: if SkyeLab's end closes while the agent still talks (the app quit or
  restarted, the thread was stopped, the token was refused), the proxy answers every request in flight and after it
  with an error saying SkyeLab's tools are gone for this session and to start the thread again from SkyeLab (a tool
  call gets an `isError` result, anything else JSON-RPC error -32000). Why: [decisions](decisions/orchestration.md).
  A `wait_thread` stops polling, with an error, as soon as the caller's grant is revoked or its row is no longer
  live (stopped, exited, or the thread closed). Browser calls from a thread that isn't running here any more are refused like start and
  send.
- **Device tools.** `skyelab_devices::Claims` (an app global, `DeviceClaims`) maps a device id to the thread that
  opened it, or to the user. `device_open` claims first (so a thread closed during the listing leaves nothing behind), lists afresh, and
  keeps the claim only for a booted simulator or emulator (physical devices are refused); a device another thread holds,
  or one the user took back, is refused, and so is every other call on a device the thread doesn't hold. **Take back**
  in the Devices panel makes the user the holder until **Let agents use**; closing or archiving a thread releases its
  claims. Every screenshot or input lists afresh first and refuses (dropping the claim) when the
  device is no longer booted under the name it was opened with: an emulator serial passes to the next AVD. Calls run on the background executor through the panel's `DeviceSource` (`xcrun simctl` / `adb`, each from
  `env_clear()` plus the agent allowlist and the Android/Xcode SDK vars). Input is Android only (`adb shell input
  tap|text|keyevent`; text is printable ASCII, at most 500 chars, not starting with `-` and without `%s`, which `input` reads as a space; taps 0..=10000 px). `device_id` is 1-128 chars of
  `[A-Za-z0-9._:-]` not starting with `-`. A screenshot is the device's full-size PNG (so tap coordinates match) as
  an MCP image block plus `{width, height, bytes}`. After each agent action the panel gets one new screenshot: no
  polling. Claims live in memory, so a restart frees every device.
- **Browser tools.** A thread reaches only the tab it opened (`BrowserPanel::agent` names its thread); the user's
  tabs, another thread's and one the user took over are never matched, and there is no fallback. Calls run on the UI
  thread through `skyelab-browser`'s agent path: `parse_agent_url` (http/https), `check_agent_eval` and the in-page
  guard. Limits: one tab per thread (`browser_open` reuses it), URL 2048 chars, expression 32 000 chars, eval 10 s,
  result 64 KiB (a bigger one is an error, never cut JSON; a thrown message is cut). A screenshot follows eval's
  page rules (it runs the in-page guard first, since the engine URL can name a pending load while a local file page
  is still painted, and drops an image taken while a new page committed) and its 10 s timeout, which covers the PNG
  encoding too. It captures only the web view (WKWebView's own snapshot, macOS only; elsewhere an error) and comes
  back as an MCP image block plus `{width, height, bytes}`: a PNG at most 1600 px wide, halved until it is under
  1 MiB. Like any screenshot it shows cross-origin iframes, which eval can't read. Each agent tab has its own
  non-persistent WebKit data store (`with_incognito`), so it shares no cookies or storage with the user's tabs (on the
  default persistent store) or other threads' tabs. Open and navigate answer once the new page commits. Take over
  reloads the page, ending the agent's scripts, and an eval still running answers with an error.
- **Visual replies.** T3's `html_render` and `html_preview` (`skyelab-agent/src/mcp/html.rs`), T3's limits: html
  1-512 000 chars, title 1-200, height 80-2000, preview width 240-1600 (default 728), appearance dark (default) or
  light. Only inline HTML (T3's `path` input and local image inlining are left out). `html_render` stores nothing of
  its own: the tool call, already saved with the thread's events, is what the timeline draws (`html_card.rs`), and only
  a call that came back without an error. Both answer only while the caller is live. `html_preview` lays the page out
  hidden at the width, waits 600 ms after the load, measures `scrollHeight` and returns a PNG (the browser
  screenshot's caps) with `{width, contentHeight, capturedHeight, bytes}`; 10 s timeout. Every page runs in
  `skyelab_browser::page`: a fresh webview per render on its own in-memory store (Windows: a temp folder), loaded from
  a string (opaque `about:blank` origin, no file access), every later navigation, new window and download refused, no
  IPC, no devtools, and a CSP meta put before anything in the document: scripts, styles, images, fonts and media
  inline or from `https:` (T3 lets pages load public resources too; it also blocks private addresses in previews, which
  a CSP can't do, so an `https` name on your network can still be requested, never read), `connect-src`, frames,
  workers, objects, forms and `base` all `'none'`, no `http:`. Theme variables are T3's names, from the app theme.
  Snapshots render one at a time per app, cached in memory by tool call and theme. Not yet: T3's console messages,
  link opening and zoom.
- **UI.** A child's header says "Delegated by <parent>"; a parent lists its delegated threads. The store records
  `parent_thread_id` (schema 3).
- **Codex** sessions get the same tools and grant; see [codex.md](codex.md).
- **Windows:** the same tools over the named pipe, compile-checked only (`scripts/check-windows.sh`), not yet run on
  Windows. Screenshots there answer "not supported yet".
- **Not yet:** delegation to other machines.

