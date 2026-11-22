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

| Crate | Holds | Exists |
|---|---|---|
| `skyelab-core` | accounts/bindings, domain types, no GPUI, no async runtime | yes |
| `skyelab-app` | GPUI desktop client (binary `skyelab`) | yes |
| `skyelab-store` | SQLite persistence: projects, threads, thread events (sync API, no GPUI) | yes |
| `skyelab-agent` | spawning/streaming agent processes | later |
| `skyelab-daemon` | environment server + RPC, hosted in the app | yes |
| `skyelab-tailnet` | `Tailnet` trait over the Tailscale CLI: `self_node / peers / whois` | yes |
| `skyelab-devices` | device hub core: list/boot/install/launch/screenshot on iOS Simulators and Android emulators (sync, shells out to simctl/adb/emulator) | yes |

## Tailscale plan

- **v1 (desktop):** use the OS-installed Tailscale. Read `status` for your devices and MagicDNS names. Listen only on
  the tailnet IP. For each incoming connection, call LocalAPI `whois`. Accept only if `UserProfile.ID == Self.UserID` and
  the node is untagged.
- **Mobile / no Tailscale app:** embed a userspace node (libtailscale via the `libtailscale` crate, which needs Go to build). It needs
  no VPN entitlement, but each install becomes its own tailnet device.
- **Later:** official `tailscale-rs` (pure Rust, currently v0.7, labelled unstable, no mobile or MagicDNS yet), once it hits 1.0.

## Remote

The app can serve this machine to your other devices. It is **off by default**: the Remote tab's "Share this machine
on my tailnet" switch (`skyelab::ToggleSharing`) turns it on, and the choice is saved as `share_on_tailnet` in
`settings.toml`, next to `accounts.toml` (never in it). The status bar item (it opens the Remote tab) says
`Sharing on <MagicDNS name>:7287`, `Not sharing`, or the exact reason it couldn't start (Tailscale not installed, not
running, logged out). Turning it off stops the listener and closes every connection, even when the choice can't be
saved (the error shows). Closing the window stops it too. A remote call still queued when sharing went off, or one
whose client gave up waiting (30 s), never runs.

**Trust model.** Every TCP connection goes through the daemon's gate, in order, before a single byte is read:

1. The listener binds to this node's tailnet IPs only, never a wildcard. If Tailscale reports no IP, sharing fails.
2. `tailscale whois` on the socket's peer address must name one of **your own, untagged** devices: the node's user
   and the profile's user both equal this node's user, the node has no tags, and it isn't this node itself (another
   OS user on this machine could reach the tailnet IP too). Tagged nodes belong to the tailnet, not to you, so they
   are refused even if you tagged them. A whois error or timeout refuses the connection.
3. A WebSocket handshake carrying an `Origin` header is refused: that is a web page open on one of your devices,
   which would pass whois.
4. The first message must be a compatible `hello`.

**What a connected device can do.** List projects and threads, read thread events, start a thread in an existing
project, send follow-ups and interrupt. It can't add projects, change accounts or bindings, open terminals or run
anything else. Remote calls run through the same `ThreadView` code a local prompt uses, so the account rules are
identical: an unbound folder is refused with an error sent to the client (no fallback account, no thread row), and
the account is recorded on the thread row. A remote-started thread opens as a tab in the local window (without
taking focus), and closing that tab stops its agent, as for any thread. Every stored event, local or remote, is
pushed to connected clients. The Remote tab lists connected devices (MagicDNS name, login, address) with a
Disconnect button.

**Driving another machine (the environment switcher).** The top of the sidebar picks which machine the window
shows: "This Mac", your own online tailnet devices (same ownership rule as the gate: your user, untagged, not this
node), recent ones, or a MagicDNS name typed by hand. Recents are saved as `recent_environments` in `settings.toml`.
On a remote machine the sidebar's projects and threads come from its daemon through `skyelab-client`, and opening a
thread watches its events (the client fills gaps and replays after reconnects) into a remote thread tab that draws
items with the same renderer as a local thread. Send starts a thread or sends a follow-up there; Stop interrupts it.
The status bar and the thread header say which machine you are on and its connection state (connecting, connected,
reconnecting, version mismatch, or "that machine didn't accept this device" when its whois refuses you). The account
shown is the id the remote reports. Local-only actions don't apply: no local terminal for a remote project, no
worktree controls. Switching machines closes every remote tab and drops the client, so local and remote data never mix.

**Restrict the port in your tailnet policy.** The gate already refuses other users and tagged nodes, but an ACL keeps
them from reaching the port at all. A grant that lets only your own devices reach SkyeLab on each other:

```jsonc
// Tailscale admin console → Access controls
{
  "grants": [
    // Each user's own devices may reach SkyeLab on their other devices, and nothing else may.
    { "src": ["autogroup:member"], "dst": ["autogroup:self"], "ip": ["tcp:7287"] }
  ]
}
```

With the default "allow all" policy any tailnet member can open a TCP connection to the port; they are refused at
whois, but replacing the default with grants like the one above is better.

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
