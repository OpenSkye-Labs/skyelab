# User guide: accounts

[Back to the user guide](user-guide.md)

## Accounts and bindings

SkyeLab runs every agent on the account its folder is bound to: a Claude Code account or an OpenAI Codex account.
A folder with no binding never runs an agent.

### First-run setup

With no accounts or no projects, the Welcome page shows setup instead of its usual shortcuts. To open it again later,
run **Set Up Accounts** from the command palette.

- **Agents** lists Claude Code and Codex, found on your `PATH` the same way threads find them, with the version each
  reports. "Not found" means a thread can't start on that provider yet.
- **Accounts** offers each `~/.claude`, `~/.claude-*`, `~/.codex` and `~/.codex-*` folder that looks like a login
  (it holds `settings.json`, `.claude.json` or `projects/`; for Codex `config.toml`, `sessions/` or `history.jsonl`).
  SkyeLab only checks that those names exist: it never opens a file in them, so no login or token is read. Tick the
  ones you want, edit a label if you like, and click **Add account**. Nothing is saved before that click. Folders
  already added are not offered again.
- **Projects**: click **Choose folder…**, or one of the git repos found directly under `~/code`, `~/projects`, `~/dev`
  or `~/src`. Then pick the account it runs on and click **Bind and add project**. With two or more accounts none is
  picked for you; with exactly one it is picked, and still waits for your click. If the folder already runs on that
  account (through a parent folder's binding), no new binding is added.

**Done** (once you have an account and a project) or **Skip for now** returns to the usual Welcome page.

### Add an account

1. Open Settings (<kbd>⌘,</kbd>) → **Accounts** and click the **Add account** card. Pick **Claude** or **Codex**, give
   it a name (`Work`; the id is made from it unless you type one), choose its config folder
   (`/Users/you/.claude-work`, or **Choose…**), then click **Add account**. Each account shows as a card: its provider
   mark in the account's color, its name (click it, or the pencil, to edit in place) and its config folder, with a
   copy button.
2. Press **Log in** on its card. For Claude that opens a terminal tab running `claude auth login` with
   `CLAUDE_CONFIG_DIR` set to the account's folder (and its GitHub folder as `GH_CONFIG_DIR`, when it has one), so
   the login lands in that folder and nowhere else. SkyeLab creates the folder first, private to you, and never sees
   the token. Signing in from a shell works too: `CLAUDE_CONFIG_DIR=~/.claude-work claude auth login`.

For a Codex account the config folder is its `CODEX_HOME` (for example `/Users/you/.codex-work`). Its **Log in** runs
`codex login` with `CODEX_HOME` set to that folder, created the same way. A config file written before Codex support has no
`provider` key; those accounts are Claude. An account's provider is fixed once added (its threads hold that provider's
sessions); to switch, add a new account and re-bind the folder.

Codex threads run `codex app-server` in the `workspace-write` sandbox by default: Codex can edit files in the folder
but not outside it, and has no network for its commands. When it wants to run a command it doesn't trust or step
outside the sandbox, the thread asks you (see [Permissions](user-guide-permissions.md#permissions)). The repo's own `.codex/` config, hooks and rules never load; your account's own
`CODEX_HOME/config.toml` (MCP servers, model) does.

MCP servers you added to an account load in its threads, next to SkyeLab's own: for Claude, the user-scope servers
in that account's `.claude.json` (`claude mcp add --scope user`, run with that account's `CLAUDE_CONFIG_DIR`); for
Codex, the `mcp_servers` of its `config.toml`. Only the bound account's servers load, never another account's, and
never a repo's `.mcp.json` or a folder's local-scope servers, so a cloned repo can't add tools. Their tools ask for
permission like any other tool. A server whose name starts with `skyelab` is left out, with a note in the thread (SkyeLab's own server keeps that
name). If the account's
`.claude.json` can't be read, the thread starts without those servers and says why. Claude Code connects no plugin MCP
servers in this mode (plugin skills and commands still load). After you add or change a server, run **Restart Agent
Session** in a thread to pick it up.

Accounts and bindings are saved in `accounts.toml` in the platform config folder:
`~/Library/Application Support/skyelab/` on macOS, `~/.config/skyelab/` on Linux, `%APPDATA%\skyelab\` on Windows.

### Bind a folder

In Settings → **Projects & folders**, under **Folder bindings**, pick the account and click **Choose folder…**. Each
binding shows as folder → account. Everything under that folder now runs on that account. A deeper binding wins over a
shallower one, so you can bind `~/work` to your work account and `~/work/oss-fork` to your personal one. Use **Test a
folder** to check which account a path resolves to.

The status bar shows the account for the current folder, or **No account** in the warning color. If you send a prompt in an unbound
folder, the thread asks you to bind it first and offers a **Bind this folder to an account** button.

### Stale bindings and Re-confirm

When you bind a folder, SkyeLab records the real folder the path pointed to at that moment. If the path later
points somewhere else, the binding turns **stale**: it shows a yellow warning and no agent starts under it. This
happens when:

- the path became a symlink, or a symlink was pointed at another folder (it now leads to a different folder),
- the binding was written by an older SkyeLab that didn't record the folder (bound without confirming),
- the path can't be resolved at all, for example a moved folder or a broken link (it can't be found).

The row says which, in one sentence, and shows where the path leads now. If the new folder is right, click
**Re-confirm**. If not, fix the folder on disk or remove the binding (the unlink button).

## GitHub per account

Give an account its own GitHub login: in Settings → **Accounts**, edit the account and set its **GitHub folder** (an
absolute path such as `/Users/me/.config/gh-work`), then in Settings → **Source control** press **Log in**. That
opens a terminal running `gh auth login` with `GH_CONFIG_DIR` set to that dir. SkyeLab never sees the token. The
**Source control** section then shows which GitHub user is logged in.

Agents, project terminals and the Changes tab's **Commit**, **Push**, **Pull** and **Open PR** use the folder's
account's GitHub login. If the account has none, Push, Pull and Open PR refuse instead of using your global `gh` login.
Push only works for https remotes without credentials in the URL, and checks every push URL of the remote; ssh remotes
are refused, because an ssh key isn't tied to an account. The background fetch and Pull follow the same rules for the
remote's fetch URLs, and also refuse while the repo's own `.git/config` sets an upload-pack, a proxy or its own
certificates (the message names the key to remove); set those in your global git config instead. A thread's pull request status is read with the same account's `gh` (`gh pr view`), and never
for an account with no GitHub login: Details shows **Sign in to GitHub** instead.

An agent's own `git push` (in its shell) also asks only the account's `gh` for https credentials, never your
keychain or a helper from `~/.gitconfig`. Agents don't get your ssh-agent, and git's ssh transport fails for them.
To let one account's agents push over ssh anyway, add `ssh_agent = true` to that `[[account]]` in `accounts.toml`.

**Commit** commits the ticked files, or with none ticked exactly the working tree the Changes tab shows (the
button says how many files). If anything changed since the tab loaded, Commit refuses; refresh, look again, and
commit. Repo hooks run on Commit and Push, with the same clean environment; if a commit hook adds files you
didn't review, the commit is undone and they stay staged. Push never pushes submodules.

**Generated commit messages and PRs.** The sparkle next to the commit message asks the thread's own account's agent
for a conventional commit message (a subject of 72 characters at most, and a short body when it helps) from the
diff Commit would commit: the ticked files, or every listed file with none ticked. With none ticked, Commit
commits everything changed in the project's folder; when the project is a subfolder of a bigger repo, the rest of
that repo is left alone. The sparkle next to the PR title
writes the title and description from the branch's commits and its diff against its base (`origin/HEAD`, else
`main` or `master`), filling the repo's PR template (`.github/pull_request_template.md` and the usual other
places) when it has one. Both send only files inside the thread's folder, never ignored files, and at most 60 KB of
diff (the rest is cut, with a note). The thread's title and last prompt go along as context. The agent gets no
tools and keeps no session, and if the folder is now bound to another account it refuses. The text only fills the
fields: nothing is committed or opened until you click. If you edit the field while it writes, your edit stays.
Turn the sparkles off in **Settings → Diffs & pull requests → Generate commit messages and PRs**.
