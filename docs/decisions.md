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
  asks to log in rather than using the default account. A login made there stays in that dir; agents never use it,
  since they never start unbound. Limit: a login shell's rc files may export credentials again. (2026-11-23)
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
  header can't win over the reset. (2026-11-23)
- **Commit snapshot:** the Changes tab records the working tree's tree id (a scratch index, `add -A`, `write-tree`)
  before reading the diff. Commit stages the listed files by name (`--literal-pathspecs`, after `--`) and refuses if
  the tree id changed, so files that appeared or changed after the user looked are never committed. Limit: a change in
  the milliseconds between that check and `git add`. (2026-11-23)
- **MCP socket path:** `<data dir>/skyelab/run/<pid>/mcp.sock` can exceed sun_path (104 bytes on macOS) with a long
  HOME. Then the socket goes in a fresh dir under the temp dir (else `/tmp`), created exclusively with mode 0700 and
  checked to be a real dir owned like the run dir; grant files stay in the run dir. Removed when the server drops; a
  crash leaves it for the OS's temp cleanup. (2026-11-23)
