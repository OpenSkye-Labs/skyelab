# User guide: accounts

[Back to the user guide](user-guide.md)

## Accounts and bindings

SkyeLab runs every agent on the account its folder is bound to: a Claude Code account or an OpenAI Codex account.
A folder with no binding never runs an agent.

### First-run onboarding

On a fresh install (no accounts or no projects), onboarding opens in place of the Welcome page, one page per step,
with progress dots on top. **Return** or **Space** goes to the next page and **Esc** to the one before, while no field
or button has the keyboard; **Back** and **Skip** do the same with the mouse. **Skip** or **Start building** on the last
page saves `onboarded = true` in `settings.toml`, so it never opens by itself again. Run **Open Onboarding** from the
command palette to go through it again. Pages slide in and lists fade up; with macOS **Reduce motion** on (read at
launch), they appear without moving.

1. **Welcome**: the one idea, a folder bound to an account.
2. **Theme**: the five theme families and System / Light / Dark. The whole app changes as you click (a miniature shows
   it too), and the choice is saved like Settings → Appearance.
3. **Agents & accounts**:

- **Agents** lists Claude Code, Codex and the GitHub CLI, found on your `PATH` the same way threads find them, with the
  version each reports. "Not found" means a thread can't start on that provider yet. While **Check for updates** is on, SkyeLab
  also asks the npm registry for each CLI's latest release and shows **2.2.0 available** beside an older one. Update
  it the way you installed it; SkyeLab never installs anything.
- **Accounts** offers each `~/.claude`, `~/.claude-*`, `~/.codex` and `~/.codex-*` folder that looks like a login
  (it holds `settings.json`, `.claude.json` or `projects/`; for Codex `config.toml`, `sessions/` or `history.jsonl`).
  SkyeLab only checks that those names exist: it never opens a file in them, so no login or token is read. Tick the
  ones you want, edit a label if you like, and click **Add account**. Nothing is saved before that click. Folders
  already added are not offered again.
  Each added account shows whether it is signed in, with **Log in** (a terminal in that account's folder) while it
  isn't. **Add account…** opens Settings → Accounts.

4. **Import projects** lists the folders the added accounts' agents worked in, read from each account's own history
   (`projects/` for Claude, `sessions/` for Codex; a folder another account's history names is never read through a
   link). Folders that are already projects or no longer exist are left out. They are grouped by repository (the
   origin URL), plain folders last. A git repo with at least 3 sessions, the newest within 30 days, is ticked for you.
   Each row shows the account it will be bound to: the one whose history has the most sessions there. Click it to
   pick another. A folder that already runs on an account (its own or a parent's binding) shows that account with a
   lock. **Import N projects** binds each ticked folder to its shown account and adds it; nothing is bound before.
5. **Done** lists the keys to start with.

### Add an account

1. Open Settings (<kbd>⌘,</kbd>) → **Accounts** and click the **Add account** card. Pick **Claude** or **Codex**, give
   it a name (`Work`; the id is made from it unless you type one), choose its config folder
   (`/Users/you/.claude-work`, or **Choose…**; left empty it is `~/.claude-<id>`, or `~/.codex-<id>` for Codex), then
   click **Add account**. Two accounts never share a config folder: adding or editing one onto a folder another
   account already uses is refused, since it would be one login under two names. Each account shows as a card: its provider
   mark in the account's color, its name (click it, or the pencil, to edit in place) and its config folder, with a
   copy button.
2. Press **Log in** on its card. For Claude that opens a terminal tab running `claude auth login` with
   `CLAUDE_CONFIG_DIR` set to the account's folder (and its GitHub folder as `GH_CONFIG_DIR`, when it has one), so
   the login lands in that folder and nowhere else. SkyeLab creates the folder first, private to you, and never sees
   the token. Signing in from a shell works too: `CLAUDE_CONFIG_DIR=~/.claude-work claude auth login`.

Each account card says whether that account is signed in: **Signed in as you@example.com**, **Not signed in** (and
**Log in** turns blue), or **Sign-in unknown** when the CLI couldn't answer (hover it for why). SkyeLab asks the
account's own CLI with only that account's folder (`claude auth status`, `codex login status`), never reads a token,
and asks again once a Log in terminal ends, or when Settings opens a minute or more later. A card with a GitHub folder
also shows its GitHub login, as **Source control** does. The setup page shows the same on its account cards, with
**Log in** on one that is signed out.

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
never a repo's `.mcp.json` or a folder's local-scope servers, so a cloned repo can't add tools. Claude asks for
permission for their tools like any other tool; Codex follows its own approval setting for them (not yet tested
against the real Codex). A server whose name starts with `skyelab` is left out, with a note in the thread (SkyeLab's own server keeps that
name). If the account's
`.claude.json` can't be read, the thread starts without those servers and says why. The MCP servers of the account's
enabled Claude Code plugins load too (the plugins on in its `settings.json`, installed for the user, not per project),
named as Claude names them, such as `plugin:posthog:posthog`, and their tools ask like any other. A plugin that can't be
read is left out with a note; so is a plugin server whose name one of your own servers already uses. After you add or change a server, run **Restart Agent
Session** in a thread to pick it up.

Accounts and bindings are saved in `accounts.toml` in the platform config folder:
`~/Library/Application Support/skyelab/` on macOS, `~/.config/skyelab/` on Linux, `%APPDATA%\skyelab\` on Windows
(`skyelab-dev` for a build from source, see [debugging.md](debugging.md#dev-builds-own-dirs)).

### Bind a folder

In Settings → **Projects & folders**, under **Folder bindings**, pick the account and click **Choose folder…**. Each
binding shows as folder → account. Everything under that folder now runs on that account. A deeper binding wins over a
shallower one, so you can bind `~/work` to your work account and `~/work/oss-fork` to your personal one. Use **Test a
folder** to check which account a path resolves to.

The status bar shows the account for the current folder, or **No account** in the warning color. If you send a prompt in an unbound
folder, the thread asks you to bind it first and offers a **Bind this folder to an account** button.

An agent keeps the account it started with. If you rebind (or unbind) the folder while a thread's agent is still
running, your next message to it is not sent, and neither is approving its plan or a permission or answering its
question (declining still goes through): the agent is stopped, your text stays in the composer, and the thread shows
**This thread can't continue here** with the reason, as reopening it would. Each send checks `accounts.toml` as saved
at that moment, so an edit made outside SkyeLab counts too; if the file can't be read, the send waits with an error
and the agent keeps running. When the folder is now bound to the other
provider, the notice offers **Continue in Codex** (or **Continue in Claude**); otherwise **Start a new thread**.

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

Give an account its own GitHub login: in Settings → **Source control** press **Set up** on its row. That gives the
account the GitHub folder `~/.config/gh-<id>` and opens the login below. To use another folder, edit the account in
Settings → **Accounts** and set its **GitHub folder** (an absolute path such as `/Users/me/.config/gh-work`), then
press **Log in** in Source control. That
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
commit. Repo hooks run on Commit, Push and Pull, with the same clean environment; if a commit hook adds files you
didn't review, the commit is undone and they stay staged. When the repo is shared with another account (one of its
worktrees, a folder inside it, a folder around it, its hooks folder, or the repo itself is bound to another account or
to none), the hooks are skipped and
the note says "Hooks skipped: this repo is shared with another account": another account's agent could have written
them (or "couldn't confirm this repo is single-account" when SkyeLab couldn't list the repo's folders). A worktree
counts wherever it is, even one missing from git's own list. Filters, merge drivers and signing programs set only in
the repo's own `.git/config` don't run either (put them in your global git config to use them). Push never pushes
submodules.

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
