# User guide: daily setup, coming from T3 Code

[Back to the user guide](user-guide.md)

A short checklist to move your daily work from T3 Code to SkyeLab. Each step links to the full page.
For a longer walk-through, see [Getting started](getting-started.md).

## 1. Install the release build

1. Open the GitHub releases page and download the file for your computer:
   - **macOS** (Apple silicon): the `.dmg`. Open it and drag SkyeLab to Applications.
   - **Windows:** the `-setup.exe` installer.
   - **Linux:** the `.deb` (install it with your package manager) or the `.AppImage`.
2. To check a download by hand, compare its SHA-256 with the `SHA256SUMS` file on the same release.
3. Updates come in the app. A pill in the sidebar says **Update available**. The app checks each download against
   `SHA256SUMS` before it installs it. **Check for Updates** in the palette checks right away.
   More: [updates](user-guide-updates.md).

SkyeLab does not install the agents. Install Claude Code (`claude`), and Codex, Cursor CLI, Antigravity and the
GitHub CLI (`gh`) if you use them.

## 2. Add your accounts

An account is one login, kept in its own folder. Add as many as you need.

1. Open Settings (<kbd>⌘,</kbd>) → **Accounts** → **Add account**.
2. Pick **Claude**, **Codex**, **Cursor** or **Antigravity**, give it a name, and choose its config folder (such as
   `~/.claude-work`). Two accounts can't share a folder.
3. Press **Log in** on its card. A terminal opens with that account's folder set, so the login lands there only.

On first start, onboarding finds your existing `~/.claude*`, `~/.codex*`, `~/.cursor*` and `~/.antigravity*`
folders and offers to add them. More: [accounts](user-guide-accounts.md).

## 3. Bind your folders

1. Open Settings → **Folder bindings**, pick an account and click **Choose folder…**.
2. Everything under that folder now runs on that account.
3. The deepest binding wins: bind `~/work` to your work account and `~/work/oss-fork` to your personal one.
4. Use **Test a folder** to check which account a path gets.

There is no default account. A folder with no binding never runs an agent; the thread asks you to bind it first.

## 4. MCP servers, per account

MCP servers belong to an account, not to SkyeLab. A thread loads only its own account's servers.

- **Claude:** add a server with that account's folder set:

  ```sh
  CLAUDE_CONFIG_DIR=~/.claude-work claude mcp add --scope user <name> -- <command>
  ```

- **Codex:** add it to `mcp_servers` in that account's `config.toml`.
- **claude.ai connectors** (Slack, Notion, …) you connected on claude.ai load in that Claude account's threads.
- **Plugins:** the MCP servers of the account's enabled Claude Code plugins load too, as `plugin:<plugin>:<server>`.

A repo's `.mcp.json` never loads. After you add or change a server, run **Restart Agent Session** in the thread.
More: [accounts](user-guide-accounts.md#accounts-and-bindings).

## 5. GitHub login per account

1. Open Settings → **Source control** and press **Set up** on the account's row.
2. Log in with `gh` in the terminal that opens. The login stays in that account's own GitHub folder.

Push, Pull and Open PR then use that account's GitHub user. An account without a GitHub login can't push.
More: [GitHub per account](user-guide-accounts.md#github-per-account).

## 6. Bring over your T3 threads

T3 ran Claude Code and Codex in the same account folders, so SkyeLab can read those sessions.

1. Bind the project's folder to the account T3 used for it.
2. Open the project's menu → **Import sessions…** (or **Import Sessions** in the palette).
3. Tick the sessions you want. Each becomes a thread, and sending a message continues the same session.

Only Claude Code and Codex sessions can be imported. SkyeLab reads the account's files and never changes them.
More: [managing threads](user-guide-managing-threads.md).

## What differs from T3

- **One account per folder, enforced.** A thread can't start under the wrong login.
- **Quitting stops agents.** While an agent works or waits for you, Quit asks first and lists what will stop.
  More: [windows and quitting](user-guide-windows.md#quitting).
- **A repo's `.claude/settings.json` is ignored**, with its hooks and `.mcp.json`, so a cloned repo can't add tools
  or allow itself anything. Your account's own settings still load. To stop being asked for a command or file in a
  project, use **Allow for this project** on the permission card. That rule lives in SkyeLab, per project and
  account, never in the repo. More: [permissions](user-guide-permissions.md#allow-for-this-project).
- **GitHub over https only.** Push and Pull use the account's `gh` login over https. ssh remotes are refused.
- **No telemetry.** Every agent, git and gh call and terminal SkyeLab starts gets `DO_NOT_TRACK=1`. SkyeLab itself
  sends none.
