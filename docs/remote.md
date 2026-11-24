# Remote environments

Part of [architecture.md](architecture.md).

The app can serve this machine to your other devices. It is **off by default**: Settings → Sharing & access's "Share this machine
on my tailnet" switch (`skyelab::ToggleSharing`) turns it on, and the choice is saved as `share_on_tailnet` in
`settings.toml`, next to `accounts.toml` (never in it). The status bar item (it opens Settings → Sharing & access) says
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
   are refused even if you tagged them. A whois error or timeout refuses the connection. At most 16 connections wait
   on whois at once, and at most 4 from one address, so one tailnet member can't take every slot from your devices.
   An address whois just refused as someone else's is dropped without another whois for 30 s.
3. TLS 1.3 with this node's certificate. A client speaking plaintext fails here.
4. A WebSocket handshake carrying an `Origin` header is refused: that is a web page open on one of your devices,
   which would pass whois.
5. The first message must be a compatible `hello` carrying a **device key the user approved for that node**
   (below). Nothing but hello is answered before that.

**Device approval.** Whois proves "one of my machines", not "the SkyeLab app I opened": any process running as the
user on any of their tailnet machines passes it, an agent with network access included. So each client install has
a device key (32 random bytes, `skyelab_protocol::new_device_key`) that it sends in hello (protocol 1.2), and the
sharing machine admits a hello only if `(whois node StableID, SHA-256 of the key)` is in `approved_devices` in its
`settings.toml`. The daemon enforces it (`Daemon::set_approved`, `Config.approved` from the first connection);
the app only edits the list.

- An unknown pair is refused `not_approved` and listed as a request: at most 16, two per node (the newest
  replaces the oldest, so one machine making keys can't push other devices off the list), each dropped 10 minutes
  after its last try. Settings → Sharing & access and the status bar show "<device> (<user>) wants to connect" with a code: the
  first 16 hex chars (64 bits) of the key's hash, too many to grind a look-alike key for a code seen on screen.
  The client works the code out from its own key and shows it too, so the user can tell their app's request from
  another program's. Allow saves the pair, then admits it. Deny drops the request and answers `denied` until
  sharing restarts (in memory only: "not now"). A connection is listed under connected devices only once its
  hello was approved.
- Revoke applies first, then saves (an off switch works even when saving fails): `set_approved` closes every open
  connection whose pair is gone. The hello check and the record of which pair a connection used happen under one
  lock that `set_approved` also takes, so a revoke can't miss a hello passing at that moment. A revoke, a Disconnect
  or the connection closing also cancels its `CallGate`: a call already read but not yet dispatched is refused, and an agent
  call still waiting for the UI (`UiAgents`) is abandoned, so it never runs. A read the store is already serving
  still finishes. A connection blocked writing to a client that stopped
  reading closes when that write times out.
- The client maps the codes to `DisconnectKind::NotApproved` / `Denied` (additive kinds; match with `_`) and, while
  not approved, retries at least every 3 s, so an Allow takes effect within seconds. Desktop shows "Waiting for
  approval … (code …)" or "That machine denied this device"; mobile shows the client's message.
- A `settings.toml` that can't be read approves nobody. A client older than 1.2 sends no key, so it is always
  refused `not_approved` (and never listed: there is nothing to approve).

**Where the key lives, and what that stops.** The key is what separates SkyeLab from another program of the same
user on an approved machine, so it lives outside every project folder and, where the OS can, where other programs
can't read it:

- macOS release builds: a keychain item SkyeLab creates (service `dev.openskye.skyelab`). Its default access list
  trusts only the program that made it, so an agent running `security find-generic-password` or reading files gets
  a system prompt, not the key. Debug builds use the file below, so a throwaway `HOME` never touches the login
  keychain.
- Windows, Linux: `<config dir>/skyelab/device-key`, mode 0600. That keeps other OS users out, but not another
  program of the same user: an agent on such a machine that reads the file and replays it passes. Their secret
  stores have no per-app access control either, so they wouldn't do better.
- Mobile: a file in the app's sandboxed data dir. Other apps can't read it and the phone runs no agents.

It stops: a script, tool or agent on one of your machines that dials the port without SkyeLab's key, including one
that makes its own key (it gets a prompt with a different code, which you deny); a key replayed from another node.
It doesn't stop:

- on Windows and Linux, a same-user process that reads the key file;
- on macOS, a user who answers "Always Allow" to that prompt, a process that can debug or patch SkyeLab itself,
  or one that plants the keychain item (readable by every app) before SkyeLab's first release run;
- a machine you connect to learns your key (it is sent in hello). A compromised one could replay it, but only
  from your own approved node, since the node is part of the approval. A challenge-response (HMAC of the key over
  a server nonce) would close that;
- agents on the sharing machine itself: they run as you there and can edit `settings.toml`. The approval guards
  the network, not the host.

TLS client certificates would add nothing here: their private key would sit in the same places.

The client (`skyelab-client`) checks the server in turn: it dials only addresses in Tailscale's ranges, speaks only
`wss://`, and accepts only a certificate chain that verifies against the Mozilla roots (`webpki-roots`) whose leaf
names exactly the MagicDNS name it dialled (a wildcard isn't enough). It refuses to dial an IP address, because no
certificate can vouch for one. 100.64.0.0/10 is also carrier-grade NAT space, so the address check alone can't stop
a hostile network (a phone with the VPN off) from pointing the name at another subscriber's 100.x host; the
certificate check does.

**What a connected device can do.** List projects and threads, read thread events, start a thread in an existing
project, send follow-ups, interrupt, and answer a permission request the thread's agent waits on
(`answer_permission`, 1.3: Allow once, Allow for this thread or Deny, exactly as a click in the thread there; on an
ExitPlanMode request an allow approves the plan and moves the thread out of Plan, as Approve does there; the reply waits
until the agent took it), and answer an AskUserQuestion (`answer_questions`, 1.4: one list of labels per question, sent
as a Submit there; the agent refuses a count that doesn't match). Both go through `UiAgents` and its `CallGate` like
every agent call. It can forward ports from this machine's loopback (below). It can't change a thread's permission mode, add projects,
change accounts or bindings, open terminals or run anything else. Remote calls run through the same `ThreadView` code a local prompt uses, so the account rules are
identical: an unbound folder is refused with an error sent to the client (no fallback account, no thread row), and
the account is recorded on the thread row. A remote-started thread appears in the local window's sidebar with an
unread dot (it never takes the page or focus), and closing it stops its agent, as for any thread. Every stored event, local or remote, is
pushed to connected clients. A reply to `thread_events_since` holds at most 500 events and 8 MiB (half the
client's 16 MiB message limit), and the client asks again from the last seq until it has caught up. A single event
over 8 MiB keeps its seq but arrives as a short notice to open the thread on its own machine. Settings → Sharing & access lists connected devices (MagicDNS name, login, address) with a
Disconnect button.

**Port forwarding (1.5).** A connected device may also forward a port on this machine's loopback to itself, so a
dev server an agent started here opens in a browser there. The main connection asks with `forward_port {port}` and
gets a random token for that forward. The daemon refuses ports below 1024, its own listening ports, and a denylist
of services that hand over the machine without a login of their own: 2375/2376 Docker, 2379 etcd, 3389 RDP, 5037
adb, 5432 Postgres, 5900 VNC, 6000 X11, 6379 Redis, 8001 kubectl proxy, 9222 browser debugging, 9229 Node
inspector, 11211 memcached and 27017 MongoDB. It also refuses more than 32 forwards per connection, and a port
nothing accepts on 127.0.0.1 or ::1 (it dials once to check). It never dials any other host.

Each TCP connection the client accepts locally opens **a new connection to the same listener**, through the whole
gate again (whois, TLS, no `Origin`, approved key in hello). That hello declares the `tunnel` capability. A tunnel
connection gets no pushed events, isn't listed as a connected device, and may call only `open_tunnel
{port, token}`: anything else is refused and closes it. That rule keeps a hidden connection from forwarding or
driving anything out of Disconnect's reach. `open_tunnel` is admitted only while another live, non-tunnel connection
with **the same approval** (node and key hash) holds the forward that token names. So a port nobody asked for, or one
another device, key or window asked for, is never reached. After its `ok`, the connection carries raw bytes in binary
frames to and from that port. No new listener is opened on the tailnet.

A tunnel ends when its forward does (`stop_forward {port, token}`, or the owning connection closes, is revoked or
Disconnected), when it is kicked itself, on EOF from either side, or when a write stalls past the idle timeout.
Each forward has at most 64 tunnels.

The client (`Client::forward_port`) binds 127.0.0.1 only, through `skyelab_core::process::exclusive`. It keeps the
remote port number when nothing here answers on it at 127.0.0.1 or ::1, else it takes any free port. A connect
check is needed because std sets SO_REUSEADDR, and on macOS that lets a bind to 127.0.0.1:P succeed over a local
server on the wildcard address. The desktop opens and copies `http://127.0.0.1:<port>/`, the socket the client owns,
never `localhost`, which a browser may send to whatever holds ::1. The client dials at most 3 tunnels at a time
across all its forwards, because the gate admits 4 pending per address and drops the rest. It keeps at most 64
open per forward. A tunnel's hello must name the node the approved main connection named, and the client skips
any pushed event while it waits for the `open_tunnel` reply. A port is forwarded once per client.

Dropping the `Forward`, or the main connection ending, closes the listener and its tunnels. Dropping it also sends
`stop_forward`, and so does a forward that the server allowed but the client couldn't start. A `forward_port`
request never times out on the client, so a late reply can't leave a forward that nobody stops (the daemon's probe
bounds it to 10 s). The desktop also drops every forward on a refusal, on a machine switch and when the window
closes. Half-close isn't carried: EOF on either side closes the tunnel. A client checks `hello.protocol.minor >= 5`
and never sends the calls to an older server, and the desktop palette says that machine is too old. An older client
never sends them.

**Driving another machine (the environment switcher).** The top of the sidebar picks which machine the window
shows: "This Mac", your own online tailnet devices (same ownership rule as the gate: your user, untagged, not this
node), recent ones, or a MagicDNS name typed by hand. Recents are saved as `recent_environments` in `settings.toml`.
On a remote machine the sidebar comes from its daemon through `skyelab-client`: every project's threads
(`list_threads` per project, listed again a second after pushed events) in the local flat list's sections, with each
thread's pin and last reply (1.4: `Thread.pinned`, `Thread.preview`, the reply's first 400 chars; absent from an older
server), then its projects. Opening a thread watches its events (the client fills gaps and replays after reconnects) into a remote thread page that draws
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
closes that machine's threads, lists nothing, sends nothing, and shows a warning with a "Trust it" button, which saves
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
clears the remote projects and threads (mobile: the empty projects screen). Local-only actions don't apply: no local terminal for a remote project, no
worktree controls. Switching machines closes every remote thread view and drops the client, so local and remote data never mix.

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
