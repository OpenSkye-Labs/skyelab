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
  ssh-agent, TZ, XDG) passes. Anything that can pick an account, provider or base URL does not. (2026-11-23)
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
- **`AccountsConfig::resolve` stays on the UI thread.** Measured on 2026-11-23 (Apple silicon, internal APFS, release
  build, `crates/skyelab-core/tests/resolve_cost.rs`): resolve() cost with 20 bindings is about 245 µs per call for an
  existing folder and 265 µs for a missing one. It runs on keystrokes in Settings' test field, on send and on folder
  changes, never per frame, so it stays synchronous. Limit: a binding on an unmounted or hung network volume makes
  each `realpath` wait for the volume's timeout and the UI freezes for that long; move resolve off the UI thread if
  that shows up. (2026-11-23)
