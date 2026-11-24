# <img src="crates/skyelab-app/icons/128x128.png" width="44" height="44" alt="Porcelain Bean"> SkyeLab

SkyeLab is a native agentic development environment (ADE) written in Rust with [GPUI](https://www.gpui.rs/).
You run Claude Code agents in threads over your projects, review their changes, and keep a terminal, a browser
preview and your simulators next to them. It is inspired by [T3 Code](https://github.com/pingdotgg/t3code) and
borrows its overall shape.

**Why it exists: every folder is bound to exactly one account (Claude or Codex), and SkyeLab enforces it.** An agent in your
work repo always runs on the work account, and one in a side project always runs on your personal account. A folder
with no binding doesn't fall back to a default. SkyeLab asks you to bind it first.

> **Status: early, pre-release.** Expect rough edges and breaking changes. macOS is the main platform.

![A thread with its Changes open in the right panel](docs/images/thread.png)

## Features

What's built today:

- **First-run setup.** With no accounts or projects, the Welcome page shows which agents are installed (and their
  versions), offers the Claude and Codex config folders it finds in your home folder as accounts, and binds a project
  folder to the account you pick. It only checks that those folders exist; it never reads a login or token.
- **Account bindings.** Add Claude accounts (one `CLAUDE_CONFIG_DIR` each) or OpenAI Codex accounts (one
  `CODEX_HOME` each) and bind folders to them in Settings. The folder decides both the account and the agent.
  The deepest binding wins. Each binding remembers the real folder you confirmed, so if the path later points
  somewhere else (a swapped symlink, a moved folder), it turns *stale* and no agent starts there until you re-confirm.
- **One thread list.** The sidebar lists every project's threads together (pinned, working, then Today, Yesterday,
  Previous 7 days and Older), each row with the agent's last reply, its project, state (with a running timer while it works), branch, pull
  request and the account its folder is bound to; hovering a row shows where it runs, on which account and binding. Settle a
  finished thread or snooze it until later (both undoable); settled and snoozed ones wait in folded shelves at the
  bottom, and a snoozed one comes back marked **Woke**. A project filter narrows it, and New thread asks
  which project when it can't tell.
- **Several windows.** **New Window** (<kbd>⇧⌘N</kbd>) opens another window on the same projects, threads and
  accounts, with its own page, sidebar filter and panels. A thread open in two windows runs one agent; both show it.
- **Update check.** Once a day at most, and from **Check for Updates**, SkyeLab asks GitHub for its latest release
  and shows a small notice with **Download** (opens the release page) and **Skip this version**. It sends nothing but
  the request, installs nothing, and Settings → General turns it off.
- **Keyboard.** Every command is an action you can rebind in `keybindings.toml` (reloaded as you save; bad lines
  are reported with their line number and skipped), listed with its current keys in Settings → Keybindings.
  F6 / Shift+F6 move between the sidebar, conversation, message input and the open panels.
- **Projects and threads.** Each thread runs the `claude` CLI in stream-json mode, or `codex app-server` for a Codex
  account, streams its text and thinking as they arrive, shows tool calls as cards (inline edit diffs, shell
  commands with their output, a live todo list, answerable questions and plans, subagent cards), folds a finished
  turn's work under **Worked for …**, lists the files each turn changed, shows a failed turn's error with Retry, finds
  text with <kbd>⌘F</kbd>, and saves the conversation to a
  local SQLite database (complete messages; streamed partial text is shown, not saved).
  Reopening a thread resumes the same session. The thread header names the provider. Rename, pin, archive or delete
  threads (with undo), search them by title and message text, see which ones finished while you were away (unread
  markers and notifications), and move between them with <kbd>⌘N</kbd>, <kbd>⇧⌘[</kbd> / <kbd>⇧⌘]</kbd> and
  <kbd>⌘1</kbd>–<kbd>⌘9</kbd>. <kbd>⌘O</kbd> opens the folder in your editor. The header is a plain breadcrumb;
  the **thread details** card (<kbd>⇧⌘B</kbd>, floating over the chat) holds the folder, branch (with how far behind or ahead of its upstream, kept
  fresh by a background fetch, and a fast-forward Pull), git actions, scripts, delegated agents, pull request (its
  state, checks and review) and editor, the thread's context window (also in the composer footer as **42% context**,
  with a `/compact` hint from 80%), plus every account's latest usage reading. Dialogs dim the window behind them.
- **Usage.** Settings → Usage compares every account: limit readings, and cost and tokens for today, 7 and 30 days,
  per account and per project, with a daily chart, from what the agents reported (no account is asked).
- **Scheduled tasks.** Settings → Scheduled tasks saves a prompt that starts a new thread in a project every N
  minutes or hours, daily or on weekdays at a time, or on a cron expression, while SkyeLab runs. Each run uses the
  folder's bound account as resolved then; a rebound or unbound folder pauses the task instead. **Schedule this
  prompt…** in the palette starts one from a composer ([guide](docs/user-guide-scheduled.md)).
- **GitHub per account.** An account can have its own `gh` config dir (`GH_CONFIG_DIR`). Agents, project terminals
  and the Changes tab's Commit / Push / Open PR all run as that account's GitHub user (a sparkle writes the commit
  message or PR text, from the PR template too, with that account's agent); inherited `GH_TOKEN` /
  `GITHUB_TOKEN` never pass through. Push accepts https remotes (credentials bound per account) and refuses ssh.
- **Pull requests and issues.** **Open Pull Requests** / **Open Issues** (palette) list the open PRs and issues of
  every bound project, each read with its own account's GitHub login (mine, review requested, assigned, all). A PR
  starts a review thread with the PR linked; an issue starts a thread with the issue as a context chip.
- **Composer.** Attach images and files (paperclip, drag or paste), mention tracked files with `@`, run commands with
  `/` and skills with `$`, add another thread's gist or its pull request with `#` (same account only), and pick the model and effort and the permission mode per thread from two small chips. A message sent while the agent works queues
  until the turn ends; **Steer** sends it in now. The strip under it shows the machine, account, checkout, changed
  files and branch. A long paste (8 KB or 200 lines) attaches as a text file instead, and images you sent or preview
  open large in a zoomable viewer. <kbd>⌘S</kbd> stashes a prompt for later (a **Stash** list to restore it), and
  **Quote** on text you select in a reply adds it to the message as a Markdown quote. On macOS you can dictate into
  it (microphone button or <kbd>⌘⇧M</kbd>), recognized on your Mac when macOS supports your language there.
- **Scratch threads.** **New Scratch Thread** asks for an account and runs a quick question in that account's own
  scratch folder, bound to it on first use and listed as **Scratch · <account>**. No default account, even here.
- **Titles, drafts, history and forks.** After the first reply, the thread's own account names it (off in Settings →
  Chat, next to the model and permission defaults for new threads). Unsent drafts stay per thread across restarts, <kbd>↑</kbd>
  recalls earlier prompts, **Fork from here** on a reply starts a new thread from that point, and **Continue in
  Codex** / **Continue in Claude** carries a thread's history to the other provider once the folder is bound to it
  ([guide](docs/user-guide-threads.md)).
- **Send to several models.** A new thread's prompt can go to 2–4 models of the folder's account at once, each its own
  thread (in its own worktree if you like), compared side by side with cost, time and changed files; keep the best
  ([guide](docs/user-guide-threads.md#send-to-several-models)).
- **Side chats.** `/side <question>` (or `/btw`) asks it in a small chat beside the thread, in the same folder under
  the same account, without interrupting the thread's agent. It is read-only (Plan mode, no SkyeLab tools);
  **Bring back** adds its answer to the thread's composer.
- **Import sessions.** Existing Claude Code and Codex sessions of a project, read from the folder's own account
  only, become threads with their history; sending continues the same session.
- **Worktrees and diff review.** Switch "Local checkout" to "New worktree" under the composer and the thread gets its own git worktree on a
  `skyelab/<slug>` branch. The Changes tab, in the right panel beside the thread, shows a coloured diff of what the agent did, and you can discard the worktree.
  Click or drag down a line's gutter to comment on lines; **Send N comments** puts them in the composer as one chip.
- **Command palette.** <kbd>⌘K</kbd> / <kbd>⇧⌘P</kbd> (<kbd>Ctrl</kbd> on Linux and Windows) finds and runs any action,
  switches project, opens a thread (also found by its messages, in any project) or goes to a terminal, Changes or
  browser tab.
- **Files.** <kbd>⌘P</kbd> finds a file in the thread's folder, <kbd>⇧⌘F</kbd> searches file contents in a right-panel tab,
  and a read-only Preview tab shows a file with syntax highlighting (or an image). File paths in the agent's replies
  open there at their line. Nothing outside the thread's folder is read.
- **Branches, checkpoints and scripts.** A branch picker in the thread details card (switch, create, and stash or carry
  changes), a base branch for new worktrees, a checkpoint per agent turn with "Turn N" diffs and "Rewind to before
  turn N", and per-project scripts (run from the card or the palette) with setup scripts for new worktrees.
- **Terminal.** An embedded terminal panel (`alacritty_terminal`), opened in the project or worktree folder, with
  side-by-side and stacked split panes you can resize, tabs named after what runs in them (rename with a double-click)
  that follow their thread and reopen after a restart, a confirm before closing a running command, theme colors, and
  **Add to prompt** to send selected output to the agent.
- **Browser preview.** A browser tab for your dev server, with zoom, find in page, Copy URL, Duplicate tab, Open in
  system browser and (development builds) DevTools. Agents can open their own tab and navigate it, read its
  URL, evaluate JavaScript in it and take a screenshot of it (http and https only, localhost included; screenshots
  on macOS and Linux). The tab says an agent controls it;
  **Take over** detaches the agent. An agent never reaches a tab you opened or another thread's, and its tab keeps
  its own cookies and storage. macOS and Linux only (like delegation).
- **Device hub.** Lists iOS Simulators and Android emulators. Boot, shut down, take screenshots, open URLs and
  install apps. Agents can open a booted device for their thread, screenshot it and (Android) tap, type and press
  Home or Back; the panel shows who uses it and its latest screen, and **Take back** stops the agent.
- **Share a machine over your tailnet.** Off by default. When on, the app serves this machine over `wss://` with its
  Tailscale HTTPS certificate, and only your own, untagged devices may connect, each one after you allow it once
  (matching a code shown on both screens). Revoke cuts a device off at once.
- **Environment switcher.** Pick another of your machines from the machine chip under the composer (manage them in
  Settings → Machines) to see its projects and threads, start threads there and send follow-ups. Its agents run there,
  on its own accounts. Link a machine (command palette: "Show threads from … in this list") and its threads also show
  in This Mac's list, marked with its name, and open in place.
- **Mobile client (in progress).** An iOS and Android app in GPUI under [`mobile/`](mobile). It connects to a
  shared machine over your tailnet (`*.ts.net` names, `wss://`), shows the desktop's flat thread list, streams a
  thread live with the desktop's timeline (folded turns, shell, diff, todo and subagent cards), sends or stops, and
  answers the agent's permission requests, questions and plans. Not packaged for distribution yet.
- **Agent orchestration.** An agent can start and drive other threads through SkyeLab's own MCP tools, only in
  projects bound to its own account. A child's breadcrumb links its parent, and the parent's thread details card lists its
  delegated threads under Lineage, with their status. macOS and Linux only for now.

- **Tool approvals in the thread.** When an agent needs permission for a tool (a command, an edit), the thread shows
  what it wants to run and waits: **Allow once**, **Allow for this thread** or **Deny**. Each thread has a
  permission mode (Ask, Plan, Accept edits, Full access, the last behind a confirm). Phones and other machines viewing
  the thread can answer too. When a turn fails or the agent exits with an error, its stderr shows as an
  **Agent output** row.

Not built yet: agents beyond Claude Code and Codex, and delegation to other machines.

| Changes tab | Accounts and bindings | Device hub |
|---|---|---|
| ![Changes tab with a diff](docs/images/changes.png) | ![Settings with a stale binding](docs/images/settings.png) | ![Devices tab](docs/images/devices.png) |

## Quick start

### 1. Platform setup

You need the latest stable Rust ([rustup](https://rustup.rs)); `rust-toolchain.toml` picks it. You also need
[Claude Code](https://docs.claude.com/en/docs/claude-code) installed, with `claude` on your `PATH`, and for Codex
accounts the [Codex CLI](https://github.com/openai/codex) (`codex` on your `PATH`).

- **macOS 14 or later:** full Xcode plus the Metal toolchain:
  `xcodebuild -downloadComponent MetalToolchain`. Without it the build fails with "metal shader compilation failed".
- **Linux:** `libxkbcommon-x11-dev libwayland-dev libx11-xcb-dev libvulkan1 libfontconfig-dev clang cmake libwebkit2gtk-4.1-dev libgtk-3-dev`
- **Windows:** the MSVC toolchain, nothing else.

### 2. Run it

```sh
cargo run -p skyelab-app
```

### 3. Set up accounts

Add each account in Settings → Accounts and press **Log in** on its card: a terminal tab signs it in to its own
config folder (`claude auth login` with its `CLAUDE_CONFIG_DIR`, or `codex login` with its `CODEX_HOME`). From a
shell it is the same:

```sh
CLAUDE_CONFIG_DIR=~/.claude-work claude auth login
CODEX_HOME=~/.codex-work codex login
```

Then start SkyeLab. On first run the Welcome page lists the config folders it found (`~/.claude`, `~/.claude-*`,
`~/.codex`, `~/.codex-*`): tick the ones to add, then choose a project folder and the account it runs on. You can
also add accounts by hand in Settings (<kbd>⌘,</kbd>, or <kbd>Ctrl+,</kbd> on Linux and Windows). The [user guide](docs/user-guide.md) covers the rest.

## Architecture

The desktop app hosts an environment daemon in-process. Clients (this window, another machine's window, the mobile
app) talk to it over a WebSocket protocol. Details, decisions and the roadmap are in
[docs/architecture.md](docs/architecture.md).

| Crate | What it holds |
|---|---|
| `skyelab-core` | Accounts, bindings and app settings. No GPUI, unit-tested without a window. |
| `skyelab-app` | The GPUI desktop app (binary `skyelab`). |
| `skyelab-agent` | Launches `claude` (stream-json) or `codex app-server` with the bound account and maps both onto one event stream. |
| `skyelab-store` | SQLite storage for projects, threads and thread events. |
| `skyelab-git` | Git status, diffs, worktrees, branches and per-turn checkpoints. |
| `skyelab-terminal` | PTY terminal and its GPUI view. |
| `skyelab-browser` | The webview browser tab. |
| `skyelab-devices` | iOS Simulator and Android emulator control (`simctl`, `adb`, `emulator`). |
| `skyelab-protocol` | The RPC messages between clients and the daemon. |
| `skyelab-daemon` | The environment server and its tailnet connection gate. |
| `skyelab-client` | The client side of the protocol, used by the environment switcher and the mobile app. |
| `skyelab-tailnet` | Talks to the installed Tailscale: this node, peers, `whois`, certificates. |

## Security model

One account per folder, never a default; agents start from a clean environment; project settings don't load;
approvals stay with you; sharing is tailnet-only. The full summary is in [docs/security.md](docs/security.md).

## Platform support

| Platform | Status |
|---|---|
| macOS 14+ (Apple silicon) | Primary. Used and tested daily. |
| Windows | Builds and passes tests in CI. Less tested by hand. |
| Linux | Builds and passes tests in CI. Less tested by hand. |
| iOS, Android | Remote client in progress under [`mobile/`](mobile). See [docs/mobile.md](docs/mobile.md). |

## Contributing

Read [CLAUDE.md](CLAUDE.md) first. It holds the rules this repo is built with: the accounts invariant, Rust and
error-handling rules, and the GPUI rules in [docs/gpui.md](docs/gpui.md). Every change goes through the `/loop`
workflow (plan, build, verify, review), and done means all three pass:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

To find out why something is slow or leaks, turn on debug mode (`SKYELAB_DEBUG=1`, or Settings → General → Advanced →
Developer mode): an overlay with frame times, redraws and live object counts, a redacted log of network calls and
child processes, Perfetto traces, flame graphs (`scripts/profile.sh`) and opt-in profilers (hotpath, dhat, Tracy).
[docs/debugging.md](docs/debugging.md) explains each one.

## License

[MIT](LICENSE). Credit to [T3 Code](https://github.com/pingdotgg/t3code) (MIT) for the design this follows.
