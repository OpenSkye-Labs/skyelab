# Changelog

All notable changes to SkyeLab are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Before 1.0, a minor version may break things.

## [Unreleased]

### Added

- **Bots**: the Bots page lists every scheduled task, and a task can now also start when its repo gets a new pull
  request, a new issue or a pull request whose checks fail (polled every 5 minutes through the account's own `gh`).
  GitHub-started runs are Plan or Ask only, get the item as quoted untrusted text, start without SkyeLab's own tools,
  and have an hourly cap. Each bot keeps a run history.

### Changed

- Settings → Scheduled tasks now links to the Bots page, and the rail's separate Scheduled tasks item is gone. Threads
  a bot started carry a **Bot** badge (was **Scheduled**). New tasks default to Plan.

## [0.1.6]

### Changed

- Every agent, git/gh call and terminal SkyeLab starts gets `DO_NOT_TRACK=1`, so tools that honor it skip telemetry.
- Sidebar rows are two lines, a thread waiting on you leaves **Working**, and **Undo** after a delete sits in the
  sidebar footer.
- The composer's `/` menu has **/model**, **/plan** and **/default**; `@` lists files git doesn't track yet.
- **Worked for** sits above the reply, and question buttons say what they do.

### Fixed

- A connection to another machine no longer drops as "the environment stopped answering" while the app is busy.
- Scheduled task form fixes.

## [0.1.5]

### Fixed

- Buttons always respond: a click right after a menu or popover closed used to do nothing (about 70 buttons).
- **⌘W** closes the focused terminal or the right panel's tab, as in T3 Code, never the window.
- **Devices** closes on a second click or Esc; dock **Zoom In/Out** works.
- The terminal drawer has **Split right**, **Split down**, **New terminal** and **Close terminal** buttons; ⌘J with no
  terminal starts a shell; unnamed tabs in one folder are numbered.
- The sidebar's snooze button opens its menu; questions, plans and the usage banner take less room and keep Approve in
  view; Changes shows Loading instead of "Nothing to commit" while it reads.
- Merging a PR shows progress and a Merged notice; a PR's title opens its diff (**Review with agent** is its own
  button); tooltips no longer stay over dialogs.
- The browser says when a tab is empty or nothing runs at a local address, with Retry.
- The zoomed right panel's "+" tab clears the window controls, and onboarding is never covered by the docks.

## [0.1.4]

### Added

- **Antigravity** as a provider, on a shared ACP driver: each account keeps its own Google login in its own folder.
- Claude agent turns load the account's own MCP servers and **claude.ai connectors**, as T3 Code does; connectors you
  never connected show as "Not connected".

### Fixed

- **Windows:** one top row like macOS, with the window controls in its corner (no extra title strip), and shortcut
  hints say Ctrl instead of ⌘.
- **Codex:** turns no longer hang while the account's hooks run (children start with no blocked signals).
- `git init` (New project) works on git 2.54 and later.

## [0.1.3]

### Added

- **Cursor** as a provider: the `cursor-agent` CLI, each account isolated in its own config folder, with the model
  picker, hand-off and onboarding.
- A slim **left rail**: Home, Bots (coming soon), Pull requests, Issues, Scheduled, Usage and Settings.
- The sidebar's update notice is now a small pill; its actions open in a popover.

### Fixed

- **macOS:** opened from the Dock or Finder, SkyeLab now finds `claude`, `codex` and `gh` through your login shell's
  PATH, so accounts show their real sign-in and agents start. A CLI that's missing says so (#20).
- **macOS:** the keychain is read only when you first connect to another machine, not at every launch.
- **Windows:** no console window opens beside SkyeLab (closing it used to close the app), and the tools it runs don't
  flash windows.

## [0.1.2]

### Added

- **Composer**: Enter sends by default (Settings → Chat → Composer → Send shortcut: Enter, ⌘Enter for multiline, or
  ⌘Enter always); `@` file, model, `$` skill and `/` command mentions show as inline chips (#17); a T3-style model
  and effort picker.
- **Usage limits per account**, read from the account's own CLI, and the thread's working row names the agent's
  startup step (starting, resuming, MCP servers, hooks).
- **MCP sign-in**: a server that needs auth gets a **Sign in** button, run under the folder's account.
- **GitHub**: open a PR's diff in a Changes tab and post line comments as one review; comment on, close or reopen
  issues.
- **Editable file preview** with a safe save, and a ⌘F find bar in previews and side chats.
- **Thread search** matches every word in any order, keeps quoted phrases exact and shows the matched line.
- Drag the sidebar edge to hide it; settle, snooze and wake another machine's threads.
- **Motion** after Rex: one spring for docks and the sidebar, dialogs that fade in place, SkyeLab's own toasts,
  new rows that fade in once, and Reduce Motion followed when a window comes forward.
- A new README with a banner, fresh screenshots and nine translations.
- **Usage limit banner**, as in T3 Code: a limited thread docks **Usage limit reached · Resets tomorrow at 2:10 AM**
  above the composer. **Resume at reset** continues the thread once at the reset (Continue where you left off., same
  session, its own account), survives a restart, and is cancelled when you send first or archive the thread.
  **Snooze until reset** snoozes it until then. The timeline's limit card is now a quiet history line in the same
  words, "Usage limit reached. Retry after tomorrow at 2:10 AM (Asia/Manila)." (#19)

### Changed

- **New threads start in Full access**, as in T3 Code: with no default set, nothing asks. To keep the old behavior,
  pick **Ask** in **Settings → Chat → Default permissions**, which now also offers Full access. Defaults you picked
  and existing threads keep their mode. Forks, hand-offs and threads started from another machine follow the new
  default; threads an agent starts stay in Ask (Plan in a Plan project), scheduled tasks keep their own confirm.
- **A new default theme.** SkyeLab's theme is now porcelain and ink by day and midnight slate by night, with one calm
  blue. Anyone on the default gets it; the old one stays as **Classic** in Settings → Appearance.

### Added

- **Eight more themes**, light and dark: Catppuccin, Rosé Pine, Tokyo Night, Kanagawa, Everforest, Gruvbox, Nord and
  Flexoki. Credits and licenses are in [NOTICE.md](NOTICE.md).
- WCAG contrast, consistency and accessibility fixes across the UI (#18); SkyeLab Light's colors no longer fall back
  to defaults; a stopped turn reads Stopped, not Failed.

### Fixed

- An Allow or Deny could show as Withdrawn when the agent ended its turn right on the answer.
- Codex hooks could hang: child processes now start with an empty signal mask.
- Windows: git's recorded conflict resolutions work without Developer Mode.

## [0.1.1]

The first published build. 0.1.0 was tagged but never published; everything below is on top of it.

### Added

- **Add a project from the palette**: type a path (with completion), clone an https URL, or start a new project, and
  pick its account right there. No Finder dialog.
- **Onboarding** in five steps, with project import from the folders your agents already worked in, and calmer
  motion (a crossfade with a zooming icon).
- **Visual reply hosts** in Settings → Chat: a visual reply loads scripts, styles and fonts only from the hosts listed
  there (cdnjs, jsdelivr and unpkg by default). When a reply asks for another host, its card offers to allow it,
  and only your click adds it.
- **Codex**: the working row shows a running `sessionStart` hook, and "Allow for this thread" matches commands like
  project rules do.

### Changed

- Visual replies no longer load images or media from the network; agents inline them.

### Security

- On Windows, a visual reply's page can no longer reach the network or local addresses: every request its policy
  doesn't allow is answered locally, new windows are refused, and local names and addresses don't resolve.
- Asks that carry a reason or a network change (a sandbox escape) are never allowed by a saved rule.
- Git calls that can run filters read a frozen copy of the repo's config (git 2.54 or later), so a repo can't swap in
  a program between checks.
- Every child process SkyeLab starts is ended and reaped when it's no longer needed.
- Windows: a forwarded port's listener can't be shared by another program.

### Fixed

- Windows: worktrees in folders with long (`\\?\`) paths, rewind on Windows paths.
- Linux: the daemon retries a port that's briefly busy.
- CI is green on macOS, Linux and Windows, and the mobile builds pass.

## [0.1.0]

The first public release. macOS (Apple silicon) is the main platform; Windows and Linux builds pass CI but get less
testing by hand.

### Added

- **Account bindings.** Every folder is bound to exactly one Claude (`CLAUDE_CONFIG_DIR`) or Codex (`CODEX_HOME`)
  account, the deepest binding wins, and an unbound folder never falls back to a default. A binding whose folder
  later points elsewhere turns stale until you re-confirm it.
- **First-run setup.** The Welcome page lists installed agents and offers existing `~/.claude*` and `~/.codex*`
  folders as accounts. Each account card has a **Log in** button that signs in to that account's folder only.
- **Threads.** Claude Code (stream-json) and Codex (`codex app-server`) threads with streamed text and thinking, tool
  cards (edit diffs, shell output, todos, questions, plans, subagents), folded turns, per-turn changed files,
  checkpoints with rewind, search, rename, pin, archive, delete with undo, forks, and resume after restart.
- **One thread list** across projects, with pinned, working and dated groups, settle and snooze, unread markers and
  notifications, and a project filter.
- **Tool approvals** in the thread (Allow once, Allow for this thread, Deny) and per-thread permission modes (Ask,
  Plan, Accept edits, Full access).
- **Composer** with attachments, `@` file mentions, `/` commands, `$` skills, `#` thread and PR chips, model and
  effort pickers, queued messages and Steer, prompt stash, quotes and dictation on macOS.
- **Send to several models**, side chats (`/side`), scratch threads, title generation, per-thread drafts and prompt
  history, and **Continue in Codex / Claude**.
- **Import sessions** from existing Claude Code and Codex sessions of the folder's own account.
- **Git:** worktrees per thread, a Changes tab with diffs and line comments, branch picker, Commit, Push, Pull and
  Open PR as the account's own GitHub user (`GH_CONFIG_DIR`), https only with credentials from that account's `gh`.
- **Pull requests and issues** lists for every bound project, read with each project's own GitHub login.
- **Terminal** panel with split panes, named tabs that follow their thread, and Add to prompt.
- **Browser preview** tabs, which agents can also drive in their own private tab (macOS and Linux).
- **Device hub** for iOS Simulators and Android emulators, which agents can open, screenshot and (Android) tap.
- **Files:** quick open, content search and a read-only preview with syntax highlighting.
- **Usage** page with limit readings, cost and tokens per account and project.
- **Scheduled tasks** that start a thread on an interval, a daily time or a cron expression.
- **Agent orchestration** through SkyeLab's own MCP tools, limited to projects on the agent's own account (macOS and
  Linux).
- **Tailnet sharing** (off by default): serve this machine over `wss://` to your own approved Tailscale devices, and
  switch to another machine's projects and threads from the machine chip.
- **Several windows**, a command palette, rebindable keys in `keybindings.toml`, and light and dark themes.
- **Updater** that checks GitHub releases at most once a day, verifies downloads against the release's `SHA256SUMS`
  (and the code signature where there is one), and installs only when you click **Restart to update**.
- **Debug mode** with an overlay, a redacted debug log, Perfetto traces and opt-in profilers.
- Release packages: `.dmg` (macOS arm64), `-setup.exe` (Windows), `.deb` and `.AppImage` (Linux).

[Unreleased]: https://github.com/OpenSkye-Labs/skyelab/compare/v0.1.6...HEAD
[0.1.6]: https://github.com/OpenSkye-Labs/skyelab/compare/v0.1.5...v0.1.6
[0.1.5]: https://github.com/OpenSkye-Labs/skyelab/compare/v0.1.4...v0.1.5
[0.1.4]: https://github.com/OpenSkye-Labs/skyelab/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/OpenSkye-Labs/skyelab/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/OpenSkye-Labs/skyelab/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/OpenSkye-Labs/skyelab/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/OpenSkye-Labs/skyelab/releases/tag/v0.1.0
