# Architecture

## What we're building

A native ADE: run Claude Code (and later Codex etc.) agents in threads over your projects, with worktrees, diffs,
a terminal, a browser preview, a device hub and agent orchestration, the way T3 Code does
([pingdotgg/t3code](https://github.com/pingdotgg/t3code), MIT). It shares environments between your machines over Tailscale.

**What sets it apart from T3 Code:** T3 Code supports several Claude accounts (one `CLAUDE_CONFIG_DIR` per provider
instance), but you pick the account per thread, by hand. There's no project or folder binding
(see t3code discussion #32072). Here, **the folder decides the account**, so a work repo can't run on the
personal plan by mistake.

## Shape (borrowed from T3, where it works)

```
 clients (GPUI desktop, mobile)  ──RPC over WebSocket──▶  environment daemon (one per machine)
                                  local / tailnet            ├─ accounts + bindings (skyelab-core)
                                                             ├─ agent sessions (claude CLI / Agent SDK, per-account env)
                                                             ├─ worktrees, git, checkpoints
                                                             ├─ terminals (PTY), preview browser, devices
                                                             └─ SQLite (threads, events)
```

- **Execution and credentials never leave the environment that runs them.** A client on your laptop driving your
  desktop's environment sends commands. The desktop's daemon launches the agent with the desktop's account dirs.
- The desktop app embeds the daemon in-process for v1. It becomes a separate process only when remote access ships.
- Agents: launch the `claude` CLI in stream-json mode (or the Agent SDK) with `launch_env(dir)` applied. That is the
  only place `CLAUDE_CONFIG_DIR` is set.

## Crates (add each only when its code exists)

| Crate | Holds |
|---|---|
| `skyelab-core` | accounts/bindings, app settings, domain types; no GPUI, no async runtime |
| `skyelab-app` | GPUI desktop client (binary `skyelab`) |
| `skyelab-agent` | launching Claude Code sessions under the bound account; stream-json events; resume; SkyeLab's MCP server for orchestration |
| `skyelab-store` | SQLite persistence: projects, threads, thread events (sync API, no GPUI) |
| `skyelab-git` | worktrees, status, structured diff (shells out to `git`) |
| `skyelab-terminal` | PTY + terminal grid model and GPUI view (alacritty_terminal) |
| `skyelab-browser` | embedded browser preview (gpui-webview); user vs agent navigation |
| `skyelab-devices` | device hub core: iOS Simulators and Android emulators (simctl/adb/emulator) |
| `skyelab-tailnet` | `Tailnet` trait over the Tailscale CLI: `self_node / peers / whois / cert` |
| `skyelab-protocol` | client↔environment RPC messages (JSON over WebSocket) |
| `skyelab-daemon` | environment server: tailnet-only `wss://`, whois gate, RPC dispatch; hosted in the app |
| `skyelab-client` | the client side of the protocol (desktop switcher, mobile app) |

`mobile/` is a separate workspace (iOS/Android client); see [mobile.md](mobile.md).

## Tailscale plan

- **v1 (desktop):** use the OS-installed Tailscale. Read `status` for your devices and MagicDNS names. Listen only on
  the tailnet IP. For each incoming connection, call LocalAPI `whois`. Accept only if `UserProfile.ID == Self.UserID` and
  the node is untagged.
- **Mobile / no Tailscale app:** embed a userspace node (libtailscale via the `libtailscale` crate, which needs Go to build). It needs
  no VPN entitlement, but each install becomes its own tailnet device.
- **Later:** official `tailscale-rs` (pure Rust, currently v0.7, labelled unstable, no mobile or MagicDNS yet), once it hits 1.0.

## Remote

Sharing this machine, device approval, the trust model and the environment switcher are in
[remote.md](remote.md).

## Orchestration

An agent can delegate work to other threads through SkyeLab's own MCP tools: `list_projects`, `start_thread`,
`thread_status`, `read_thread`, `send_message` and `wait_thread`.

- **Account rule.** A delegated thread runs under its target project's bound account, through the normal launch
  path, pinned to the account the start was approved for. An agent under account A can only list and start in
  projects bound to A. It can read, wait on and message only threads it delegated itself (its descendants), recorded
  under A, and only while they run: a message never restarts a thread that ended or that the user stopped. An unbound
  target is refused; there is never a default account. This keeps a work agent from spending the personal plan, and
  the reverse, and keeps an agent out of the user's other threads.
- **Limits.** Depth 2 at most (a user's thread is depth 0), and 4 children starting or running per thread (a child
  that finished its turn frees its slot). The children limit is claimed in the launch itself, under the store lock
  together with the status write (`child_slots::claim`), on every new child row, every resume of a stopped child
  and every follow-up that gives an idle child a new turn, whatever sent the prompt (the parent, another device,
  the user). A refused resume leaves the child as it was and its tab says why; a Send from another device has
  already been answered Ok by then. Rows left `running` by a crash still count until they are opened and end.
- **Remote mode.** A delegated child (or a thread another device starts here) that opens while the window shows
  another machine is kept out of the dock: the sidebar gets a "This Mac · N new threads here" row, and switching
  back adds the tabs. Its agent runs meanwhile, and delegated sends and Stop still reach it.
- **Transport.** `skyelab-agent::mcp` listens on a unix socket in `<data dir>/skyelab/run/<pid>/` (0700). Each agent
  session gets a grant: a one-use random token and a 0600 `--mcp-config` file that runs `skyelab --mcp-proxy`. The
  proxy's first connection consumes the token and deletes the file. Ending the session revokes the grant and cuts the
  connection. Never TCP.
- **UI.** A child's header says "Delegated by <parent>"; a parent lists its delegated threads. The store records
  `parent_thread_id` (schema 3).
- **Not yet:** Windows (no named-pipe transport, so threads there run without the tools) and delegation to other
  machines.

## Platforms

| | Status |
|---|---|
| macOS, Windows, Linux | GPUI official. v1 targets. |
| iOS | GPUI via upstream `gpui_ios`, already in our pin (`gpui-pre-platform` 0.3.12). Experimental. See [mobile.md](mobile.md). |
| Android | GPUI via longbridge `gpui-pre-mobile`, ported to our pin. Community fork. See [mobile.md](mobile.md). |

Mobile is a **remote client only**: it controls a desktop environment over the tailnet and runs no agents itself.
That keeps it small whichever UI tech it ends up on.

## Roadmap

Each item goes through `/loop` (see CLAUDE.md).

1. **Accounts:** core resolver and config (done). Then an accounts/bindings settings UI, and a binding indicator in every thread header.
2. **Threads:** run `claude` in a folder with the resolved account, stream output into a thread view, and persist to SQLite.
3. **Projects and worktrees:** project sidebar, a worktree per thread, diff view.
4. **Terminal:** `alacritty_terminal` + a GPUI renderer.
5. **Remote:** daemon split, RPC, Tailscale peers and whois, an environment switcher.
6. **Browser preview:** `gpui-webview` (experimental) plus agent automation.
7. **Device hub:** iOS Simulator / Android Emulator control.
8. **Orchestration:** delegate tasks between threads and environments.
9. **Mobile client:** GPUI on iOS and Android, plan in [mobile.md](mobile.md).

## Decisions

- **Browser, agent access:** agents may only navigate to http/https (no `file://`), because `eval_js` on a file page
  would let an agent read any local file, including other accounts' credentials. `file://` stays available to the user
  typing in the URL bar. (2026-11-23)
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
  user, so the daemon also requires an approved per-install device key (keychain on macOS). A key over a TLS
  client cert: the same storage question, and one less TLS mode to get right. (2026-11-23)
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
