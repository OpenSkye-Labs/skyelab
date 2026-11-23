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

1. Sign in to the account in its own config folder:

   ```sh
   CLAUDE_CONFIG_DIR=~/.claude-work claude auth login
   ```

2. Open Settings (<kbd>⌘,</kbd>). In **Accounts & bindings**, under **Add account**, pick **Claude** or **Codex**, enter an id (`work`), a label
   (`Work (Team)`) and the absolute config folder (`/Users/you/.claude-work`), then click **Add**.

For a Codex account the config folder is its `CODEX_HOME` (for example `/Users/you/.codex-work`). After adding it,
press **Log in to Codex** on its row: that opens a terminal running `codex login` with `CODEX_HOME` set to that
folder (SkyeLab creates the folder first, private to you). A config file written before Codex support has no
`provider` key; those accounts are Claude. An account's provider is fixed once added (its threads hold that provider's
sessions); to switch, add a new account and re-bind the folder.

Codex threads run `codex app-server` in the `workspace-write` sandbox by default: Codex can edit files in the folder
but not outside it, and has no network for its commands. When it wants to run a command it doesn't trust or step
outside the sandbox, the thread asks you (see [Permissions](user-guide.md#permissions)). The repo's own `.codex/` config, hooks and rules never load; your account's own
`CODEX_HOME/config.toml` (MCP servers, model) does.

Accounts and bindings are saved in `accounts.toml` in the platform config folder:
`~/Library/Application Support/skyelab/` on macOS, `~/.config/skyelab/` on Linux, `%APPDATA%\skyelab\` on Windows.

### Bind a folder

In Settings, under **Folder bindings**, pick the account and click **Bind folder…**. Everything under that folder
now runs on that account. A deeper binding wins over a shallower one, so you can bind `~/work` to your work account
and `~/work/oss-fork` to your personal one. Use **Test folder** to check which account a path resolves to.

The status bar shows the account for the current folder, or **No account** in the warning color. If you send a prompt in an unbound
folder, the thread asks you to bind it first and offers a **Bind this folder to an account** button.

### Stale bindings and Re-confirm

When you bind a folder, SkyeLab records the real folder the path pointed to at that moment. If the path later
points somewhere else, the binding turns **stale**: it shows a yellow warning and no agent starts under it. This
happens when:

- the path became a symlink, or a symlink was pointed at another folder (**Not the folder you confirmed any more**),
- the binding was written by an older SkyeLab that didn't record the folder (**Not confirmed yet**),
- the path can't be resolved at all, for example a broken link (**Can't be resolved now**).

The warning shows the folder the path points to now and the one you confirmed. If the new folder is right, click
**Re-confirm**. If not, fix the folder on disk or **Remove** the binding.

## GitHub per account

Give an account its own GitHub login: in Settings → **Accounts & bindings**, set the account's GitHub config dir (an
absolute path such as `/Users/me/.config/gh-work`), then in Settings → **GitHub** press **Log in to GitHub**. That
opens a terminal running `gh auth login` with `GH_CONFIG_DIR` set to that dir. SkyeLab never sees the token. The
**GitHub** section then shows which GitHub user is logged in.

Agents, project terminals and the Changes tab's **Commit**, **Push** and **Open PR** use the folder's account's
GitHub login. If the account has none, Push and Open PR refuse instead of using your global `gh` login. Push only
works for https remotes without credentials in the URL, and checks every push URL of the remote; ssh remotes are
refused, because an ssh key isn't tied to an account.

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
Turn the sparkles off in **Settings → Threads → Generate commit messages and PRs**.
