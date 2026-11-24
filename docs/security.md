# Security model

- **Account invariant.** A folder resolves to its account or to nothing. There is never a default account. An
  unknown account id in `accounts.toml` is a load error.
- **Clean environment, one allowlist.** Each agent starts with an empty environment. A short allowlist passes
  through (`PATH`, `HOME`, locale, proxy and CA settings, and the like), and then SkyeLab sets `CLAUDE_CONFIG_DIR`
  or `CODEX_HOME` for the bound account. Nothing that could pick another account, provider or API base URL
  (`ANTHROPIC_*`, `CLAUDE_CODE_*`, `OPENAI_*`, `CODEX_*`, `GH_TOKEN`, `AWS_*`, the parent's own homes) is inherited.
  Project terminals and every git/gh call SkyeLab runs use the same list (terminals add shell vars such as
  `EDITOR`, `PAGER` and `SSH_AUTH_SOCK`). An agent's own `git push` asks only the account's `gh` for https
  credentials, and gets no ssh-agent unless the account sets `ssh_agent = true`.
- **Project settings don't load.** Agents run with `--setting-sources user --strict-mcp-config`, so a cloned repo's
  `.claude/settings*.json` and `.mcp.json` can't redirect credentials or run hooks. The MCP servers you added to the
  bound account itself (Claude's user scope and its enabled user-scope plugins, Codex's `config.toml`) do load. Claude asks for their tools as for any
  other tool (per the thread's permission mode and your "Allow for this thread" answers); Codex follows its own
  approval setting for them (not yet tested against the real Codex). The thread
  details card lists them with their state, and **Restart Agent Session** reloads them. Codex runs with the folder and
  every folder above it (and a worktree's main repo) marked untrusted, so a repo's `.codex/config.toml`, hooks and
  `.rules` never load. Codex keeps its `workspace-write` sandbox (writes stay in the folder, no network) unless a
  thread picks Plan (read-only) or Full access, and its approval requests come to the thread.
- **Push, Pull and Fetch.** They go only over https (or a local path), with credentials only from the bound
  account's `gh`, and TLS checks forced on. Push, Pull and the background fetch refuse to run while the repo's own
  config (`.git/config`, which an agent can write) sets a key that runs a program or reroutes or re-trusts the
  connection: `remote.<name>.uploadpack`, `receivepack`, `proxy` or `vcs`, `http.proxy`, `http.sslCAInfo` (also per
  URL), `sslCAPath`, `sslCert`, `sslKey`, `curloptResolve`, `cookieFile`, `pinnedPubkey`, and
  `core.alternateRefsCommand` / `alternateRefsPrefixes`. The message names the key; set it in your global config
  instead if you mean it. Nothing reaches the remote and `gh` is never asked for the token.
- **Commit, Push and Pull run the repo's own hooks only on a one-account repo.** `pre-commit`, `commit-msg`,
  `pre-push`, `post-merge` and the like run with the bound account's env and the same git credential binding its
  agents get, so a hook can ask that account's `gh` for its GitHub token. An agent can write `.git/hooks` or set
  `core.hooksPath`, and one `.git` can serve folders bound to different accounts (a worktree whose main checkout is
  bound elsewhere, a project in a subfolder of a repo, a binding nested inside another). So before each Commit, Push
  or Pull SkyeLab lists where a hook could be written from: the repo's worktrees (`git worktree list`), its git dir
  (a submodule's or a separate one lives elsewhere) and the hooks dir git would use (`core.hooksPath` included). It
  runs the hooks only when each of those, every bound folder inside one and every bound folder above one resolve to
  the acting folder's account. Otherwise (another account, an unbound folder, a stale binding, a repo that sits
  inside another account's folder, or one whose folders can't be listed) it runs with hooks off, as SkyeLab's own
  background git calls (status, diff, fetch, checkpoints, worktrees) always do, and the note under the buttons says
  "Hooks skipped: this repo is shared with another account" (or "couldn't confirm this repo is single-account" when
  the folders couldn't be listed). Git's worktree records can be edited by whoever writes the shared `.git` to hide a
  checkout, so SkyeLab also reads every bound folder's own `.git` (its `gitdir:` and `commondir`, or a folder git
  takes as a git dir itself): one that leads into the repo shares it, listed or not, and a `.git` it can't read as a
  small file (a FIFO, say) counts as sharing. Hooks set in config (`hook.<name>.command`, newer git) are off whenever
  hook scripts are. A push to a local-path remote always starts that repo's `receive-pack` with its hooks, its
  `core.alternateRefsCommand`, work-tree updates and gc off: git doesn't pass SkyeLab's config across a local
  transport, and that repo may be another account's.
- **Git never runs a program from the repo's own config unless hooks may run.** Every background git call (status,
  diff, fetch, checkpoints, worktrees, branch switches), Commit, Push and Pull with hooks off, and the git that
  `gh pr create` runs in the repo, override with `-c` every config key that names a program, since an agent can write
  `.git/config` (and whatever it includes): `core.sshCommand`, `gpg.program` / `gpg.<format>.program` and
  `gpg.ssh.defaultKeyCommand` get your global (or system) value, else git's own default (`ssh`, `gpg`, `gpgsm`,
  `ssh-keygen`, no key command), so signing still uses your tools; `credential.helper` and `core.askPass` are emptied
  (push and fetch then add only the account's gh); `core.fsmonitor`, `core.hooksPath` and `submodule.recurse` are
  off (`core.hooksPath=/dev/null`; on Windows `*`, a name NTFS refuses, since `\dev\null` there is a folder any user
  can create), as are automatic gc and maintenance (gc runs `gc.recentObjectsHook`). A submodule's dirty check (git run
  inside it, under its own config, which `.gitmodules` can switch back on) never runs: status and diffs pass
  `--ignore-submodules=dirty`, the review snapshot and a branch switch's stash leave checked-out submodules out of
  their pathspec and read only their HEAD. So the Changes tab shows a submodule's new commit but not edits inside it,
  and a branch-switch stash doesn't take a submodule's changes along. A new worktree is checked out by SkyeLab's own git in it, so a
  config only that worktree's gitdir or branch includes (`includeIf`) is seen too. Each clean/smudge/process filter and merge driver the repo's own config defines is emptied unless your global
  config defines the same one (Git LFS installed globally keeps working): an empty filter passes content through, and
  a `required` one (git-crypt, a `--local` LFS install) fails the command instead, so an encrypting filter never
  commits plaintext; set such a filter in your global config to use it in SkyeLab. A driver whose name has `=` in it
  can't be overridden, so the command is refused. Diff drivers (`diff.external`, `textconv`) were already off: every
  diff passes `--no-ext-diff --no-textconv`. Editors, pagers and trailer commands are never reached (messages come
  from SkyeLab, there's no terminal). Limit: filters and merge drivers are found by listing the repo's config just
  before each call, so one written in the few milliseconds between that listing and git's own read isn't seen; fixed
  keys have no such gap.
- **Approvals stay with you.** An agent's permission ask waits in its thread until you answer it there, or from
  a device you approved for sharing. "Allow for this thread" lasts while that agent runs, and is never written to
  the project's or your Claude settings. Full access asks before it turns on.
- **Project rules are SkyeLab's, not the repo's.** A repo can't allow itself anything: its `.claude/settings*.json`
  stays ignored. "Allow for this project" saves a rule in SkyeLab's own database, keyed by the project and the account
  id the agent runs under, so it never applies under another account or in another project (rebinding the folder to
  another account shows and uses that account's rules; an account later created with a removed account's id gets its
  old rules back). The rule is shown (and can be narrowed) before it is saved, and must cover the call waiting then.
  SkyeLab matches each later ask itself and answers it "allow once", so the agent's own rule grammar never sees it:
  - A command rule is the exact command, or a word prefix of one plain command. A command or prefix with `;`, `&`,
    `|`, `$`, a backtick, `<`, `>`, parentheses, a backslash or a control character (a trailing newline too) never
    matches a prefix; words split only on ASCII spaces and tabs, as the shell does. A prefix can't start with a
    program that runs whatever its arguments say (a shell, `env`, `xargs`, `sudo`, `python`, `node`, `find`, …) or
    end in a flag (`git -c`). An ask that carries anything beyond the command, its description, timeout and folder
    (Claude's `dangerouslyDisableSandbox`, a newer agent's field) is never matched. The arguments after a prefix are
    still the agent's: a test runner's flags or a config option can run code, so pick prefixes knowingly.
  - A file rule is a glob relative to the thread's folder, with no `..` or absolute paths. The file must be inside
    the folder by its text and after symlinks are resolved (a dangling symlink never matches), and never under
    `.git`, `.claude`, `.codex`, `.vscode`, `.idea`, `.husky`, `.mcp.json` or `.envrc` (any case). The check runs when
    the ask arrives: a process the agent already runs could still swap a folder for a symlink before the write.
  - An MCP rule names one tool. Codex's folder-wide write grants (`apply_patch`) and every other tool can't take one.
  Rules never answer in Plan mode or a side chat, and the thread row of an ask a rule answered names that rule.
- **Private tabs stay private.** An agent's browser tab and a forwarded port's page have their own in-memory cookies
  and storage, and the tab itself records that (not whether a forward still exists). Duplicate tab on one (also
  after Take over or after the forward ends, and on any copy) opens another private, throwaway tab, never one on your
  own browser profile. Reopen Closed never brings these tabs back, and Open in system browser is refused for them.
  Their pages may only navigate to http(s) (and in-page `about:blank`, `about:srcdoc`, `blob:` and `data:`
  documents), never `file:` or another app's scheme; downloads are refused, and a popup opens as another private tab
  only when it is a web page.
- **The prompt stash is shared across accounts.** There is one stash for all threads, and a restore brings back its
  text, files and quote, terminal and diff-comment chips on whatever account the restoring thread runs, so one
  account's agent output can reach another account's agent when you restore it there. A thread, pull request, issue or
  side-chat chip comes back only on the account it was picked under, while its folder is still bound to it.
- **Delegation stays on one account.** An agent can start threads only in projects bound to its own account, and can
  read or message only the threads it started. Its tools reach SkyeLab over a private socket with a one-use token.
- **Updates.** Only the shown release's own assets download (redirects only to GitHub's download hosts, size
  capped), and an asset must match that release's `SHA256SUMS`. That proves integrity, not authorship: the list sits
  on the same release. On macOS the new app must have the installed one's bundle identifier, and a signed install
  takes only a new app that verifies with `codesign`, has the same Developer ID team and passes Gatekeeper
  (`spctl`). On Windows a signed install runs only an installer whose Authenticode signature is valid and from the
  same publisher. An unsigned or ad-hoc-signed install (no Developer ID team) relies on the checksum, and a signature that can't be read refuses the update.
  Nothing installs until the user's Restart click, which checks the staged copy again first (hash or signature).
- **Tailnet trust.** Sharing listens only on this machine's tailnet addresses and serves only TLS 1.3 with the
  machine's own Tailscale certificate. Each connection must come from one of your own, untagged devices (checked
  with `tailscale whois`), and carry a device key you approved on this machine for that device (kept in the
  macOS keychain by release builds on a Mac). Browser pages (any `Origin` header) are refused. A connected device can list and drive
  threads, and forward ports from this machine's loopback (1024 and up, not SkyeLab's own port or a denylist of
  services such as Docker's API or debuggers), each tunnel through the same gate and only while that device's
  connection asked for it. It can't add projects, change accounts or bindings, or open terminals. The client checks the
  certificate against the exact MagicDNS name it dialled, and the desktop app also asks its own Tailscale who it
  dialled: only your own untagged devices, and the same Tailscale node it saw for that name before. The phone
  remembers the node too, but only from what the machine says about itself (it has no Tailscale CLI to ask).

The full rules are in [docs/remote.md](remote.md) and [docs/decisions.md](decisions.md).
