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
| `skyelab-agent` | launching Claude Code sessions under the bound account; stream-json events; resume |
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

The app can serve this machine to your other devices. It is **off by default**: the Remote tab's "Share this machine
on my tailnet" switch (`skyelab::ToggleSharing`) turns it on, and the choice is saved as `share_on_tailnet` in
`settings.toml`, next to `accounts.toml` (never in it). The status bar item (it opens the Remote tab) says
`Sharing on <MagicDNS name>:7287 · certificate valid until <date>`, `Not sharing`, or the exact reason it couldn't
start (Tailscale not installed, not running, logged out, no certificate). Turning it off stops the listener and closes every connection, even when the choice can't be
saved (the error shows). Closing the window stops it too. A remote call still queued when sharing went off, or one
whose client gave up waiting (30 s), never runs.

**Tailnet setting needed: HTTPS certificates.** Every connection is `wss://` with this node's Let's Encrypt
certificate for its MagicDNS name (`machine.tailnet.ts.net`), from `tailscale cert`. In the Tailscale admin console,
open **DNS** (https://login.tailscale.com/admin/dns) and turn on **MagicDNS** and **HTTPS Certificates**. Without
them sharing refuses to start and says which one is off; there is no plaintext fallback. Turning HTTPS certificates
on publishes each machine's name in the public Certificate Transparency logs (not its address or anything on it).

The certificate and key live in `<data dir>/skyelab/tls/` (mode 0700; `~/Library/Application Support` on macOS).
They are fetched when sharing starts and checked again daily; `tailscale cert --min-validity 336h` renews any
certificate with less than 14 days left, and the new one is served at once, without dropping connections. A failed
renewal keeps the current certificate and retries hourly. A fetched certificate is used only if it names this node
exactly, is valid now, and matches its key.

**Trust model.** Every TCP connection goes through the daemon's gate, in order, before a single byte is read:

1. The listener binds to this node's tailnet IPs only, never a wildcard. If Tailscale reports no IP, sharing fails.
2. `tailscale whois` on the socket's peer address must name one of **your own, untagged** devices: the node's user
   and the profile's user both equal this node's user, the node has no tags, and it isn't this node itself (another
   OS user on this machine could reach the tailnet IP too). Tagged nodes belong to the tailnet, not to you, so they
   are refused even if you tagged them. A whois error or timeout refuses the connection.
3. TLS 1.3 with this node's certificate. A client speaking plaintext fails here.
4. A WebSocket handshake carrying an `Origin` header is refused: that is a web page open on one of your devices,
   which would pass whois.
5. The first message must be a compatible `hello`.

The client (`skyelab-client`) checks the server in turn: it dials only addresses in Tailscale's ranges, speaks only
`wss://`, and accepts only a certificate chain that verifies against the Mozilla roots (`webpki-roots`) whose leaf
names exactly the MagicDNS name it dialled (a wildcard isn't enough). It refuses to dial an IP address, because no
certificate can vouch for one. 100.64.0.0/10 is also carrier-grade NAT space, so the address check alone can't stop
a hostile network (a phone with the VPN off) from pointing the name at another subscriber's 100.x host; the
certificate check does.

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
reconnecting, version mismatch, "that machine didn't accept this device" when its whois refuses you, or
"not connected" when your own whois refuses it, below). That last
one is the client's typed `DisconnectKind::Refused`: TCP connected, then the server hung up before sending a byte
(a busy daemon or a failed whois look the same). A certificate problem is a separate kind and keeps reconnecting.
The account shows as "Label · id" from the remote's own accounts.toml (protocol 1.1), or the id from a 1.0 server.
Only MagicDNS names can be typed (`skyelab_protocol::is_magicdns_name`, shared with mobile).

**A name that moves.** In a shared tailnet a device name can be reused by another node. So the desktop client checks
the dialled address with its own Tailscale: right after the TCP connect, before TLS, `Options::verify_peer` (backed by
the app's `TailscaleCli`, `skyelab_tailnet::verify_own_peer`) runs `tailscale whois` on the socket's peer address and
requires one of your own, untagged devices, the daemon's `is_own_device` rule from the client's side. Another user, a
tagged node, or a whois error or timeout (the CLI's 5 s limit) is `DisconnectKind::NotYourDevice`, and nothing is sent.
Hello also carries the server's node StableID (1.1); with a verifier it must equal the whois id
(`DisconnectKind::IdentityMismatch` otherwise), and the client reports the whois id as `node_id`. The first connect to
a name saves that id as `environment_nodes` in `settings.toml`; a later connect or reconnect that presents another node
closes that machine's tabs, lists nothing, sends nothing, and shows a warning with a "Trust it" button, which saves
exactly the node shown (refused if a different one answers by then). A `settings.toml` that can't be read refuses the
connection rather than trust an unchecked node. Without a verifier (mobile: the official app's LocalAPI is sandboxed
and there is no CLI) the pin is the hello's id, which is only what the server says about itself: a server that sends
none connects only to a name never pinned, and a node that took over the name and replays the old id passes. That
limit is mobile's until it can ask its own Tailscale. Local-only actions don't apply: no local terminal for a remote project, no
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
- **Remote is TLS-only, with the node's `tailscale cert` certificate.** The address guard can't tell a tailnet peer
  from a CGNAT neighbour, so the client verifies the Let's Encrypt chain and the exact MagicDNS name. No certificate
  means no sharing, never plaintext. rustls with the `ring` provider (no OpenSSL, no aws-lc build tools), TLS 1.3
  only, webpki roots rather than the platform verifier so mobile needs no JNI setup. (2026-11-23)
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
