# Decisions

Part of [architecture.md](architecture.md). Each entry is dated; newest at the bottom.

- **Browser, agent access:** agents may only navigate to http/https (no `file://`), because `eval_js` on a file page
  would let an agent read any local file, including other accounts' credentials. `file://` stays available to the user
  typing in the URL bar. (2026-11-23)
- **Browser, localhost for agents:** allowed. `parse_agent_url` takes `http://localhost` and loopback IPs, since a
  dev server is the point of the preview, and the agent process can already reach those over the network itself.
  (2026-11-23)
- **Browser, no screenshot tool:** wry has no snapshot API; WKWebView's `takeSnapshotWithConfiguration` is an
  `unsafe fn` in objc2-web-kit (it would need a `-sys` crate); a window capture needs Screen Recording permission and
  shows whatever is drawn in the tab's rect, which can be the user's content. (2026-11-23)
- **Minimum macOS: 14.** wry crashes on macOS < 14 for URLs with unusual characters. (2026-11-23)
- **Agent env:** parent env is cleared, then an allowlist passes through. Network and session plumbing (proxy, CA certs,
  TZ, XDG) passes. Anything that can pick an account, provider or base URL does not. (2026-11-23)
- **Project settings:** agents launch with `--setting-sources user --strict-mcp-config`, so a cloned repo's
  `.claude/settings*.json` and `.mcp.json` can't redirect credentials or run hooks. (2026-11-23)
- **Remote is TLS-only, with the node's `tailscale cert` certificate.** The address guard can't tell a tailnet peer
  from a CGNAT neighbour, so the client verifies the Let's Encrypt chain and the exact MagicDNS name. No certificate
  means no sharing, never plaintext. rustls with the `ring` provider (no OpenSSL, no aws-lc build tools), TLS 1.3
  only, webpki roots rather than the platform verifier so mobile needs no JNI setup. (2026-11-23)
- **Orchestration tools are ours, over a private socket.** No `rmcp` (it needs tokio; four JSON-RPC methods are
  enough). Agents get `--mcp-config <grant file> --allowedTools mcp__skyelab`, and `--strict-mcp-config` keeps
  every other server out. Starts and sends run on the UI thread through `ThreadView`, which re-checks the account rule
  with the config the launch uses. (2026-11-23)
- **Remote devices are approved one by one.** Whois can't tell the SkyeLab app from another process of the same
  user, so the daemon also requires an approved per-install device key (keychain on macOS release builds). A key
  over a TLS client cert: the same storage question, and one less TLS mode to get right. (2026-11-23)
- **Remote calls reuse the thread view.** The daemon's agent calls are forwarded to the UI thread and run through
  `ThreadView::send_prompt`, so there is one launch path and one place the account is resolved. Sharing is off by
  default and only an explicit toggle turns it on. (2026-11-23)
- **Remote machine names:** clients (mobile and desktop switcher) accept only MagicDNS names (`<machine>.<tailnet>.ts.net`).
  TLS certs from `tailscale cert` exist only for those names, and the client checks the cert against the exact name.
  Raw IPs, `localhost` and short names are rejected at input. (2026-11-23)
- **GitHub identity per account:** an account may name a `gh` config dir (`gh_config_dir`, absolute). `launch_env` sets
  `GH_CONFIG_DIR` from it, and agents, terminals and every git/gh call SkyeLab runs drop the parent's `GH_TOKEN`,
  `GITHUB_TOKEN`, enterprise tokens, `GH_HOST`, `GH_REPO` and `GH_CONFIG_DIR` (terminals keep a `GH_CONFIG_DIR` the
  account sets). Push and Open PR (Changes tab) run only
  while the folder still resolves to the thread's account, and refuse when that account has no GitHub dir, never
  falling back to the parent's `gh` login. A push goes to the branch's same-named upstream, else `origin`, and only to
  a plain https URL (no credentials in it, after `insteadOf` rewrites) or a local path: an ssh key or a token in the
  URL can't be bound to an account, so those are refused. It resets git's credential helpers (and `http.extraHeader`)
  to the bound `gh auth git-credential`, so an https push can't use the keychain's login. Settings' Log in runs `gh auth login --insecure-storage`: gh's
  keyring entry for the active user is shared by every config dir, so the token lives in `<dir>/hosts.yml` instead.
  Limits: a shell rc that exports `GH_TOKEN` sets it again inside a terminal; an agent whose account has no GitHub
  dir uses gh's default config; a GitHub dir inside a project holds a plaintext token that Commit all would stage.
  (2026-11-23)
- **Codex provider:** `codex app-server` under `CODEX_HOME`, the folder's account picks the provider. Protocol,
  event mapping, hardening and limits: [codex.md](codex.md). (2026-11-23)
- **One env allowlist** (`skyelab_core::env`) for agents, project terminals and every git/gh call SkyeLab runs. A
  terminal gets the agent list plus harmless shell vars (`EDITOR`, `PAGER`, colours, display, tool homes such as
  `CARGO_HOME`) and keeps relative PATH entries; no credential, provider or base-URL var (`ANTHROPIC_*`,
  `CLAUDE_CODE_*`, `OPENAI_*`, `CODEX_*`, `GH_*`/`GITHUB_TOKEN`, `AWS_*`, `GOOGLE_*`, `CLOUD_ML_*`, `GIT_*`) is ever in
  it. A terminal always names both provider homes: the bound account's, and an empty private dir under
  `<data dir>/skyelab/unbound` for the other one (and for `GH_CONFIG_DIR` when the folder is unbound), so the CLI
  asks to log in rather than using the default account. Those dirs are per account (`unbound/account-<hex id>/`),
  and one more set for unbound folders, so a login made in one account's terminal never answers in another's;
  agents never use them. git's https credentials come only from the account's gh (no helper without one), as for
  agents. Limit: a login shell's rc files may export credentials again. (2026-11-23)
- **Agent git credentials and ssh:** agents get `GIT_CONFIG_COUNT`/`KEY_n`/`VALUE_n` that reset `credential.helper`
  and add `gh auth git-credential` (under the account's `GH_CONFIG_DIR`; no helper at all without one, so nothing
  answers), and clear `http.extraHeader`, generically and for every URL-scoped `http.<url>.extraHeader` git config
  holds at launch. `SSH_AUTH_SOCK` no longer reaches agents, and `core.sshCommand` fails with a message: an ssh key
  isn't bound to any account, so an ssh push would land as whoever owns the key. An account opts in with
  `ssh_agent = true` (accounts.toml only, not the form). Terminals keep `SSH_AUTH_SOCK`: a terminal is the user's own
  shell, so their ssh keys are theirs to use there; https pushes from it still go through the account's gh when it
  has one. This stops accidents, not a hostile agent, which can still read `~/.ssh` (HOME passes). (2026-11-23)
- **SkyeLab's own git calls:** run with `env_clear` plus the allowlist, and `-c core.hooksPath=/dev/null` for
  everything except Commit and Push (status, diff, add, worktree add, the snapshot index). Commit and Push run the
  repo's hooks, since users rely on pre-commit and pre-push, with the scrubbed env plus the bound account's
  `launch_env` (Push: its `GH_CONFIG_DIR`), so a hook an agent planted sees nothing the agent couldn't already.
  Filters (`filter.<x>.clean`/`process`, from repo config and `.gitattributes`) can't be turned off generically
  without breaking LFS-style repos, so they still run during status, diff and add, with the scrubbed env only: the
  residual risk is code the agent wrote running with no more than the agent has. Commit gets `SSH_AUTH_SOCK` (ssh
  commit signing) only for an account with `ssh_agent = true`, since its hooks see the same env. GPG signing with a
  custom `GNUPGHOME` isn't passed. (2026-11-23)
- **Push URLs:** every URL of `git remote get-url --push --all` must be plain https or local (git pushes to all of
  them), and `-c http.<url>.extraHeader=` is passed for each https one, the most specific scope, so a URL-scoped
  header can't win over the reset. `--recurse-submodules=no`, since a submodule's remote is never checked.
  `core.askPass` is cleared (for agents too), so only gh answers. An https URL with `=` in it is refused (it would
  end the `-c` key). Limit: libcurl still reads `~/.netrc` (HOME passes), so a netrc entry for the host answers
  before gh. (2026-11-23)
- **Commit snapshot:** the Changes tab records the working tree's tree id (a scratch index in a fresh private temp
  dir, `add -A`, `write-tree`) before and after reading the diff, and keeps it only if both match. Commit refuses if
  the tree id changed. The commit is then built in a private index from the reviewed tree's blobs, never from the
  files again, so a write after the check can't get in: commit all reads the whole reviewed tree (`read-tree -m`
  on a copy of the real index, to keep its stat cache; staged deletions and a resolved merge work), ticked files are
  HEAD plus those paths' reviewed entries (`--literal-pathspecs`), so the user's other staged files stay out of the
  commit and staged. A ticked commit is refused during a merge, cherry-pick or revert, as git refuses a partial
  commit then. Hooks see `GIT_INDEX_FILE` pointing at the private index, as with `git commit -- <paths>`. The real
  index is reset to the commit (all of it, or just the ticked paths) only after the commit exists, so a failed
  commit leaves the user's staging exactly as it was. After the commit, any path it changed outside the ticked files
  (or outside what changed between `HEAD` and the reviewed tree) means a hook added it: the commit is undone
  (`reset --soft`). Hooks that reformat a reviewed file still work. Commit and Push hooks get the bound account's git
  credential binding (the agents' `GIT_CONFIG_*` env: helpers reset, only its gh answers, ssh off unless the account
  allows it) and `GIT_TERMINAL_PROMPT=0`. Limit: each snapshot writes blobs for changed and untracked files into the
  object store, which `git gc` prunes. (2026-11-23)
- **MCP socket path:** `<data dir>/skyelab/run/<pid>/mcp.sock` can exceed sun_path (104 bytes on macOS) with a long
  HOME. Then the socket goes in a fresh dir under the temp dir (else `/tmp`), created exclusively with mode 0700 and
  checked to be a real dir owned like the run dir; grant files stay in the run dir. Removed when the server drops; a
  crash leaves it for the OS's temp cleanup. (2026-11-23)
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
- **Accounts as saved, for every action.** Agent launches, terminals and the Changes tab's Commit, Push and Open PR
  read accounts.toml when they act (an unreadable file binds nothing), not the window's copy. The launch re-check
  compares the login (`Account::same_login`: id, provider, config dir, gh dir, ssh), not the label. (2026-11-23)
- **Windows agent tabs and processes.** wry turns InPrivate on with no profile name, and WebView2 shares one
  InPrivate session between the webviews of a user-data folder, so each agent tab gets its own private temp folder
  (a `WebContext`), removed with the tab; with `WEBVIEW2_USER_DATA_FOLDER` set (which WebView2 may prefer) an agent
  tab is refused instead. Each such folder is its own browser process group, which costs memory per agent tab. Agent
  tabs only exist where orchestration runs (unix today), so this is in place for when Windows gets them. An agent runs in a kill-on-close Job Object (`win32job`), so Stop ends node and its tools, not only the
  `claude.cmd` shim. Limits: compile-checked only (`scripts/check-windows.sh`), not run on Windows; a process the
  shim starts before the job assignment (microseconds after spawn) is outside the job; a tab folder WebView2 still
  holds when the tab closes stays in the temp dir (a registry policy that pins the folder isn't checked either).
  (2026-11-23)
- **Repo instructions still load:** checked against the real CLI (2.1.330), `--setting-sources user --strict-mcp-config`
  still reads the project's `CLAUDE.md`, so blocking project *settings* does not drop project *instructions*. (2026-11-23)
- **Checkpoints and rewind (2026-11-23):** a checkpoint is a commit (fixed SkyeLab identity, signing off) of the
  snapshot tree, at `refs/skyelab/checkpoints/<thread id>-<created at>/<n>`: `n` is rewritten right before turn
  `n + 1` starts, and `n + 1` is taken when it ends, so Turn N diffs `n - 1` against `n`. The refs keep the snapshots
  from `git gc`; deleting a thread must call `delete_checkpoints` (the app has no thread delete yet). Rewind
  deletes what was created since itself and writes the rest with `checkout-index` from a scratch index, so the
  user's index stays; it refuses a path under a symlinked folder, skips submodules, overwrites an existing ignored
  file only when the user ticks it, and refuses if the snapshot moved after the confirm. It is allowed on a project
  folder (T3 refuses file restore outside an isolated worktree); the confirm lists every file instead. Limit: with a
  prompt queued, the agent starts the next turn before the end checkpoint is taken, so it can include early edits
  of that turn.
- **Project scripts live in the store, not the repo (2026-11-23):** an agent can write the repo, and a script runs
  in a terminal, which gets the user's `SSH_AUTH_SOCK` (agents don't). The command is typed into the folder's login
  shell with a leading space; the env is the terminal's for the bound account plus `SKYELAB_PROJECT_ROOT` /
  `SKYELAB_WORKTREE_PATH`. An unbound folder runs nothing. The table's migration is one step that needs only
  `projects`, planned as schema version 8.
- **Branch switch:** `git switch` with hooks off (post-checkout hooks don't run), refused while the thread's agent
  runs and, with changes, until the user picks stash (`--include-untracked`, popped back if the switch then fails)
  or carry (git refuses an overwrite). The confirm is re-checked against the status it showed. (2026-11-23)
