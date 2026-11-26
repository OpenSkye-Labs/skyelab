# Getting started: daily setup, coming from T3

This page takes you from nothing to a normal workday in SkyeLab. It is written for people who use T3 Code today.
Each step links to the full guide when you need more. The short checklist version is
[daily setup](user-guide-daily-setup.md).

## 1. Install

SkyeLab runs on macOS 14 or later. Linux and Windows builds exist, but macOS gets the most testing.

**From a release.** When a release is published, download the `.dmg` from the GitHub releases page, open it and drag
SkyeLab to Applications. Later updates come through **Check for Updates** in the app. Until the first release is out,
that check says **No release published yet**.

**From source.** Build the app bundle and disk image yourself:

```sh
cargo install cargo-packager --locked
cargo packager --release -p skyelab-app -f app,dmg
```

This writes `target/release/SkyeLab.app` and a `.dmg`. Copy the app to Applications. If the terminal has no
Automation permission for Finder, put `CI=true` in front of the command. You need full Xcode and its Metal Toolchain
(`xcodebuild -downloadComponent MetalToolchain`).

SkyeLab does not install the agents. Install Claude Code (`claude`) and, if you want it, Codex (`codex`) the usual
way, and the GitHub CLI (`gh`) for pushing and pull requests.

## 2. Accounts: one login folder each

SkyeLab's main rule: **every folder runs on exactly one account.** An account is one login, kept in its own folder:

- **Claude:** a `CLAUDE_CONFIG_DIR`, such as `~/.claude-work`.
- **Codex:** a `CODEX_HOME`, such as `~/.codex-work`.
- **GitHub:** a `GH_CONFIG_DIR` per account, such as `~/.config/gh-work`.

On first start, onboarding walks you through five pages: **Welcome**, **Theme** (the app changes as you click),
**Agents & accounts** (the Claude Code, Codex and GitHub CLIs it found, and your existing `~/.claude*` and
`~/.codex*` folders, each with **Add**), **Import projects** (folders your agents already worked in, each with the
account it will run on) and **Done**. Return or Space goes on, Esc goes back, **Skip** leaves it. Run **Open
Onboarding** from the palette to see it again. To add an account later, open Settings (<kbd>⌘,</kbd>) → **Accounts**
→ **Add account**.

Then press **Log in** on each account card. A terminal opens with that account's folder set, so the login lands
there and nowhere else. SkyeLab never sees the token.

For GitHub, open Settings → **Source control** and press **Set up** on each account, then log in. Push, Pull and
Open PR use that account's GitHub login. An account without one can't push.

More: [accounts](user-guide-accounts.md).

## 3. Bind your folders

In Settings → **Folder bindings**, pick an account and choose a folder. Everything
under that folder now runs on that account. The deepest binding wins, so `~/work` can be your work account and
`~/work/oss-fork` your personal one.

A folder with no binding never runs an agent. SkyeLab asks you to bind it first. It never falls back to a default
account.

## 4. MCP servers, per account

Your MCP servers belong to an account, not to SkyeLab. Add them with the account's own folder set:

```sh
CLAUDE_CONFIG_DIR=~/.claude-work claude mcp add --scope user <name> -- <command>
```

For Codex, add them to the `mcp_servers` of that account's `config.toml`. A thread loads only its own account's
servers. It never loads a repo's `.mcp.json`. After you add a server, run **Restart Agent Session** in the thread.

The MCP servers of the account's enabled Claude Code plugins load too, as `plugin:<plugin>:<server>`.

## 5. Bring over your old sessions

Your T3 sessions live in the same account folders, so SkyeLab can read them. Open a project's menu and choose
**Import sessions…** (or **Import Sessions** in the palette). Tick the sessions you want. Each becomes a thread, and
sending a message continues the same session. SkyeLab only reads the account's files; it never changes them.

## 6. The everyday loop

1. Start a **New thread** in a project (the sidebar's new-thread button, or the palette, <kbd>⌘K</kbd>). Type a prompt and send it.
2. To keep the work apart, click **Local checkout** under the composer before the first prompt. It turns into
   **New worktree**, and the thread gets its own git worktree and branch.
3. When the agent asks to run a command or write a file, a card docks on top of the composer. Check it, then
   **Allow once**, **Allow for this thread** or **Deny**.
4. Review the diff in the **Changes** tab. You can comment on lines and send the comments to the agent.
5. **Commit**, **Push** and **Open PR** in the Changes tab (**Pull request…** opens the title and description
   fields). They run as the folder's account and its GitHub user.
6. **Open Pull Requests** in the palette lists open PRs across your projects, with their checks, and lets you merge.

More: [threads](user-guide-using-threads.md), [git and the diff](user-guide-git.md),
[permissions](user-guide-permissions.md), [worktrees](user-guide.md#worktrees).

## 7. Terminal, browser and devices

- **Terminal:** <kbd>⌥⌘T</kbd> opens a terminal in the project, on the project's account.
- **Browser:** <kbd>⌥⌘B</kbd> opens a browser tab. Agents get their own private tabs; see [browser](user-guide-browser.md).
- **Devices:** <kbd>⇧⌘D</kbd> lists your iOS simulators and Android emulators. Agents can use a booted one.

## 8. Other machines

You can share a machine over Tailscale and run threads on it from this one. Turn sharing on in Settings →
**Sharing & access** on the other machine. On this one, pick it with the machine chip under the composer (or
Settings → **Machines**), then approve this device on the other machine when it asks.
More: [other machines](user-guide-remote.md).

## 9. Reporting a problem

Turn on debug mode: run with `SKYELAB_DEBUG=1`, or Settings → General → Advanced → **Developer mode**. A **Debug**
chip appears in the status bar. Reproduce the problem, then run **Dump Debug State** from the palette. Attach the
dump (`<local data>/skyelab/debug/state-*.json`) and the debug log (`debug.log` in the same folder) to your report.
The log leaves out prompts, environment values and tokens. More: [debugging](debugging.md).

## What's different from T3

- **Account binding.** Every folder is bound to one account, and SkyeLab enforces it. A thread can't start under the
  wrong login, and an unbound folder never runs an agent.
- **Repo settings don't load.** Agents ignore a repo's `.claude/settings.json`, its hooks and its `.mcp.json`, so a
  cloned repo can't add tools or run code. Your account's own settings and servers still load. See
  [security](security.md).
- **Repo hooks are skipped in shared repos.** Commit, Push and Pull skip the repo's git hooks when another account
  can reach that repo (a worktree or folder bound to another account). The note says **Hooks skipped**.
- **Private agent tabs.** An agent's browser tab has its own throwaway profile. It never sees your own tabs or
  logins, and its pages can't open your system browser.
- **GitHub over https only.** Push and Pull use the account's `gh` login over https. ssh remotes are refused, because
  an ssh key isn't tied to an account.
- **Quitting stops agents.** There is no background service yet. When you quit SkyeLab, running agents stop; while
  one works or waits for you, Quit asks first.
