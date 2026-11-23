# Decisions: Platform

Part of [decisions.md](../decisions.md). OS support and process plumbing. Each entry is dated; newest at the bottom.

- **Minimum macOS: 14.** wry crashes on macOS < 14 for URLs with unusual characters. (2026-11-23)
- **`AccountsConfig::resolve` stays on the UI thread.** Measured on 2026-11-23 (Apple silicon, internal APFS, release
  build, `crates/skyelab-core/tests/resolve_cost.rs`): resolve() cost with 20 bindings is about 245 µs per call for an
  existing folder and 265 µs for a missing one. It runs on keystrokes in Settings' test field, on send and on folder
  changes, never per frame, so it stays synchronous. Limit: a binding on an unmounted or hung network volume makes
  each `realpath` wait for the volume's timeout and the UI freezes for that long; move resolve off the UI thread if
  that shows up. (2026-11-23)
- **Child output through temp files, not pipes.** On macOS std creates a pipe and sets close-on-exec in a second
  step, so a process another thread spawns in between inherits the write end, and a read to EOF waits for that
  process (a launch's `git config` once waited 22 minutes on a sibling agent). Every short-lived child SkyeLab reads
  (git, gh, simctl/adb, tailscale) goes through `skyelab_core::process`: stdin, stdout and stderr are unnamed temp
  files, read after the child exits. A test scans those crates for `.output()`, `wait_with_output` and
  `Stdio::piped()`. The agent's own pipes stay (they stream); nothing decides on their EOF alone: the pump also
  watches the agent's exit. The terminal env (which runs `git config` and reads accounts.toml) is built off the UI
  thread. (2026-11-23)
- **Windows agent tabs and processes.** wry turns InPrivate on with no profile name, and WebView2 shares one
  InPrivate session between the webviews of a user-data folder, so each agent tab gets its own private temp folder
  (a `WebContext`), removed with the tab; with `WEBVIEW2_USER_DATA_FOLDER` set (which WebView2 may prefer) an agent
  tab is refused instead. Each such folder is its own browser process group, which costs memory per agent tab. Agent
  tabs exist on Windows since orchestration runs there (a named pipe, see [orchestration](orchestration.md)).
  An agent runs in a kill-on-close Job Object (`win32job`), so Stop ends node and its tools, not only the
  `claude.cmd` shim. Limits: compile-checked only (`scripts/check-windows.sh`), not run on Windows; a process the
  shim starts before the job assignment (microseconds after spawn) is outside the job; a tab folder WebView2 still
  holds when the tab closes stays in the temp dir (a registry policy that pins the folder isn't checked either).
  (2026-11-23)
