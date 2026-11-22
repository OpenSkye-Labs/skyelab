# Remote environments

Part of [architecture.md](architecture.md).

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
"not connected" when your own whois refuses it, below). The remote's refusal is the client's typed
`DisconnectKind::Refused`: TCP connected, then the server hung up before sending a byte
(a busy daemon or a failed whois look the same). A certificate problem is a separate kind and keeps reconnecting.
The account shows as "Label · id" from the remote's own accounts.toml (protocol 1.1), or the id from a 1.0 server.
Only MagicDNS names can be typed (`skyelab_protocol::is_magicdns_name`, shared with mobile).

**A name that moves.** In a shared tailnet a device name can be reused by another node. So the desktop client checks
the dialled address with its own Tailscale: right after the TCP connect, before TLS, `Options::verify_peer` (backed by
the app's `TailscaleCli`, `skyelab_tailnet::verify_own_peer`) runs `tailscale whois` on the socket's peer address and
requires one of your own, untagged devices, the daemon's `is_own_device` rule from the client's side. Another user, a
tagged node, or a whois error or timeout is `DisconnectKind::NotYourDevice`, and nothing (not even a TLS hello) is
sent. The check runs `tailscale status` and `tailscale whois`, each bounded by the CLI's 5 s limit, inside the 10 s
connect budget; a Tailscale slow enough to use it all shows as a plain reconnect, still refused. Hello also carries
the server's node StableID (1.1); with a verifier it must equal the whois id (`DisconnectKind::IdentityMismatch`
otherwise, checked before the version so another node can't stop the client for good), and the client reports the
whois id as `node_id`. On a mismatch the TLS and hello handshake already happened, but no request follows. The first connect to
a name saves that id as `environment_nodes` in `settings.toml`; a later connect or reconnect that presents another node
closes that machine's tabs, lists nothing, sends nothing, and shows a warning with a "Trust it" button, which saves
exactly the node shown (refused if a different one answers by then). A `settings.toml` that can't be read refuses the
connection rather than trust an unchecked node. A client without a verifier gets the hello's id unchecked, which is
only what the server says about itself (a node that took over the name and replays the old id passes it). Known
limit, mobile: there is no Tailscale CLI and the official app's LocalAPI is sandboxed, so mobile passes no verifier
and pins from the hello (`nodes.json`, same Trust it flow; missing id once pinned or unreadable pins refuse; a
never-pinned name with no id connects unpinned). That is
the server's word about itself: a node that took over the name and replays the old StableID passes. Upgrade path:
an embedded libtailscale/tsnet node, or the app's LocalAPI if exposed. See [mobile.md](mobile.md), step 4.

**Nothing before the check, nothing after a refusal.** Both apps set `Options::approve_each_connection`: after
hello the client sends nothing (watch refills and requests wait or fail `NotConnected`, pushes are dropped and
refetched) until `Client::approve(hello)` for that exact hello, on every reconnect. A refusal or identity failure (a certificate failure included)
clears the remote projects, threads and tabs (mobile: the empty projects screen). Local-only actions don't apply: no local terminal for a remote project, no
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

