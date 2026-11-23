# Decisions: Git and GitHub

Part of [decisions.md](../decisions.md). Git and gh calls, pushes, commits, checkpoints, scripts. Each entry is
dated; newest at the bottom.

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
- **SkyeLab's own git calls:** run with `env_clear` plus the allowlist, and `-c core.hooksPath=/dev/null` for
  everything except Commit, Push and Pull (status, diff, add, worktree add, the snapshot index, the background fetch).
  Commit, Push and Pull run the repo's hooks, since users rely on pre-commit, pre-push and post-merge, with the scrubbed env plus the bound account's
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
- **Checkpoints and rewind (2026-11-23):** a checkpoint is a commit (fixed SkyeLab identity, signing off) of the
  snapshot tree, at `refs/skyelab/checkpoints/<thread id>-<created at>/<n>`: `n` is rewritten right before turn
  `n + 1` starts, and `n + 1` is taken when it ends, so Turn N diffs `n - 1` against `n`. The refs keep the snapshots
  from `git gc` until the thread is purged (its delete outlives the undo window): then `delete_checkpoints` drops
  them, off the UI thread. Archive and an undone delete keep them. A prompt queued behind a running turn takes no starting checkpoint, and a turn
  stopped before its result still gets its end one. Rewind
  deletes what was created since itself and writes the rest with `checkout-index` from a scratch index, so the
  user's index stays; it refuses a path under a symlinked folder, skips submodules, overwrites an existing ignored
  file only when the user ticks it (an unrecorded file or folder in the way of a checkpoint path counts as ignored),
  checks every path before changing anything, stays inside the thread's folder (not the whole repo), and refuses if
  the snapshot moved after the confirm. Filters run during the checkout, as for diff. Limit: the refs are in the repo,
  so an agent could repoint one; the confirm lists paths, not contents. It is allowed on a project
  folder (T3 refuses file restore outside an isolated worktree); the confirm lists every file instead. Limit: with a
  prompt queued, the agent starts the next turn before the end checkpoint is taken, so it can include early edits
  of that turn.
- **Project scripts live in the store, not the repo (2026-11-23):** an agent can write the repo, and a script runs
  in a terminal, which gets the user's `SSH_AUTH_SOCK` (agents don't). The command is typed into the folder's login
  shell with a leading space; the env is the terminal's for the bound account plus `SKYELAB_PROJECT_ROOT` /
  `SKYELAB_WORKTREE_PATH`. An unbound folder runs nothing. Setup scripts run only for worktree threads the user
  starts, never for an agent's delegated one, and a command must be one line with no control characters. The table is schema version 8, a step that needs only
  `projects`.
- **Branch switch:** `git switch --no-overwrite-ignore` with hooks off (post-checkout hooks don't run), refused while the thread's agent
  runs and, with changes, until the user picks stash (`--include-untracked`)
  or carry (git refuses an overwrite). A failed switch applies back the stash it made, by id. The confirm is re-checked
  against the status and snapshot it showed. (2026-11-23)
