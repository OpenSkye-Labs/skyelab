# User guide: accounts

[Back to the user guide](user-guide.md)

## Accounts and bindings

SkyeLab runs every agent on the account its folder is bound to: a Claude Code account or an OpenAI Codex account.
A folder with no binding never runs an agent.

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
