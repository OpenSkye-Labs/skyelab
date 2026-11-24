# SkyeLab user guide: other machines

Part of the [user guide](user-guide.md).

## Sharing this machine over Tailscale

You can drive this machine's projects and threads from your other devices. Its agents keep running here, on its
own accounts.

### One-time tailnet setup

1. Install Tailscale on each machine and sign in with the same user.
2. In the Tailscale admin console, open **DNS** (<https://login.tailscale.com/admin/dns>) and turn on **MagicDNS**
   and **HTTPS Certificates**. SkyeLab only speaks `wss://` with the machine's Tailscale certificate, so sharing
   won't start without them. Turning on HTTPS certificates publishes your machine names in the public Certificate
   Transparency logs (names only, not addresses).
3. Recommended: in **Access controls**, limit the port to your own devices:

   ```jsonc
   {
     "grants": [
       { "src": ["autogroup:member"], "dst": ["autogroup:self"], "ip": ["tcp:7287"] }
     ]
   }
   ```

### Turn sharing on

Click **Not sharing** in the status bar to open Settings at **Sharing**, then turn on **Share this machine on my tailnet**.
The status bar then reads `Sharing on <machine>.example.ts.net:7287` with the certificate's expiry date, or tells
you why it couldn't start (Tailscale not installed, not running, logged out, or no certificate).

Only your own devices can connect: same Tailscale user, not tagged, and not this machine. Each one must also be
allowed here once:

- The first time a device connects, the status bar reads **<device> (<you>) wants to connect**, and Settings → Sharing & access
  lists it under **Waiting for approval** with a code of four groups of four characters. The device shows the same code. Click
  **Allow** only if the codes match; any program on one of your devices can ask, and a different code means it isn't
  the app you just opened. **Deny** turns it down until sharing restarts here.
- Allowed devices are listed under **Allowed devices** (saved in `settings.toml`). **Revoke** closes its connection
  at once and forgets it; it has to ask again.
- An approval is for one SkyeLab install on one machine. A reinstall, or another app on the same machine, asks again.

Connected devices are listed in Settings → Sharing & access, each with a **Disconnect** button, which closes the connection at once.
A connected device can list projects and threads, start a thread in an existing project, send follow-ups,
interrupt, answer a permission request, answer an agent's questions and approve or reject its plan (approving moves the
thread out of Plan mode here, as a click does). Each is answered once the agent took it, or with why it couldn't
(within 20 seconds). It can't add projects, change accounts or
bindings, or open terminals. Turning sharing off closes every connection.

## The environment switcher

The machine chip at the start of the strip under the composer (**Studio Mac ⌃⌄**, your machine's name) picks which
machine the window shows. Click it to see this machine, your online devices (a green dot) and recent ones, with full
names; hover a row for its MagicDNS name and state. The palette's **Switch Machine** and the status bar's **Remote**
badge open the same menu. Its last row, **Manage machines…**, opens **Settings → Machines**.

**Settings → Machines** (under Connections) is where machines are managed: whether this machine is shared (the same
switch as in Sharing & access), every machine with its status and **Connect**, **Disconnect** (back to this machine)
or **Forget** (drops a recent name; connecting to it again still checks it is the same machine), and **Add by MagicDNS
name**: type a name such as `desktop.example.ts.net` and click **Connect**. Only full MagicDNS names work, not IP
addresses or short names. The other machine must have sharing turned on. The first
time, it must also allow this device: the status bar says **Waiting for approval** with this device's code; check
that the other machine shows the same code, click **Allow** there, and this window connects within a few seconds.

On a remote machine, the sidebar shows the same list as here: every project's threads under **Pinned**, **Working**,
then **Today**, **Yesterday**, **Previous 7 days** and **Older**, each row with its project, the agent's last reply,
its status and its account. Its projects follow; the selected one is where **New thread** starts. You can open threads,
start new ones, send follow-ups and stop them, answer permission requests and questions, approve plans, and find in a
thread with ⌘F (Ctrl+F). The thread filter, scope menu, unread and approval marks are local only, and a machine on an
older SkyeLab shows no pins or reply previews and can't take question answers. A screenshot the other machine's
store didn't keep reads **Image (205 KB) not kept**, on the desktop and in the mobile app. The account shown is the one that machine resolved. Terminals and worktree controls are
local only. A remote thread is shown as the page, like a local one. Switching back to this machine closes the remote
threads. A thread that starts on this machine while you look at another one (a delegated thread, or one started from
your phone) waits: the sidebar shows **This Mac · 1 new thread here**, and clicking it switches back, where the
thread is marked unread.

### Forwarding a port from the other machine

When an agent on the other machine starts a dev server (say on port 5173), you can open it here. Press ⌘K (Ctrl+K)
while that machine is shown and type the port number. **Forward Port 5173 from <machine>** forwards it. A note then
says which address it got on this machine: `127.0.0.1:5173` when nothing here uses that port, or another free port.
For each forwarded port, the palette then lists **Open Forwarded Port** (a browser tab at its address),
**Copy Forwarded Address** and **Stop Forwarding**. Type "forward" to find them.

- Only ports on the other machine's own loopback (127.0.0.1 or ::1) can be forwarded, and only from 1024 up. It
  refuses SkyeLab's own port, and services that would hand over the machine without a login (Docker's API, Screen
  Sharing/VNC, Remote Desktop, adb, databases such as Redis, Postgres and MongoDB, kubectl proxy, browser and Node
  debuggers). A port with nothing listening is refused with a note.
- The local end listens on 127.0.0.1 only, so other devices on your network can't use it. Any program on this
  machine can, as with any local port, and so can web pages open in your browser here (as with a dev server you
  run yourself).
- Forwards stop when you stop them, switch machines, or the connection drops. After a reconnect, forward again.
- The other machine must run a SkyeLab that knows forwarding (protocol 1.5). For an older one, the palette says
  it runs an older SkyeLab.

## Linked machines: other machines' threads in this list

To see another machine's threads next to this one's without switching, link it: open the command palette and pick
**Show threads from <machine> in this list** (it lists the machines you connected to before, and the ones already
linked, with a checkbox). Pick it again to unlink. Linked machines are saved
as `linked_environments` in `settings.toml` and connect again when SkyeLab starts.

- On **This Mac**, the linked machine's threads join the list under **Pinned**, **Working** and the days, mixed in by
  when they last changed. Each row's bottom line names the machine after a screen icon (`desktop` for
  `desktop.example.ts.net`), and its hover card says **Runs on** the full name.
- Click a row to open the thread right here, as the page. You can send follow-ups, stop it, answer its permission
  requests and questions, as in the switcher. Its agent runs on that machine, on that machine's accounts; nothing
  here can change its accounts, bindings or projects.
- A linked thread that finishes while you look elsewhere reads **Done** until you open it. That mark is kept while
  SkyeLab runs, not saved.
- A linked machine that isn't connected shows one short row under **Linked machines**, such as
  `desktop · Waiting for approval` or `desktop · Reconnecting`. Hover it for the full reason (the code to approve,
  the error). To see the code to approve, or to **Trust it**, switch to that
  machine; once it is approved or trusted there, the linked row comes back too.
- The command palette also lists each linked machine's threads as **Open thread: <title> · <machine>**.
- The thread filter and the project scope cover this machine's threads only, so linked rows hide while either is on.
- Switching to another machine closes linked threads' pages too, as it closes every remote page. Unlinking closes
  that machine's pages.
- A linked machine keeps its own connection. If you also switch to it, the window holds two connections to it;
  that is harmless.

T3 Code can also show a thread on a machine reached only through another machine's link (read-only, "via"). SkyeLab
machines don't link to each other yet, so every linked machine is one this app connects to directly.

Before connecting, SkyeLab asks this machine's own Tailscale who is at the address it dialled. It connects only to
one of your own devices (same Tailscale user, not tagged), and it remembers that machine's Tailscale node for the
name. If the name later answers from another node, its threads close and a warning offers **Trust it**: trust it only if
you replaced or renamed that machine. If your Tailscale says the machine isn't yours or can't answer, the status says
so and nothing is sent to it. If the machine claims to be a different node than your Tailscale sees, the status says
so and nothing is listed or sent after the handshake. After a reconnect nothing is asked of the machine until it is
checked again, and a refusal clears its projects and threads from the window.

On the phone, the first screen after connecting is the same flat thread list, and a thread shows the same timeline in
compact rows: tap a "Worked for …" line, a shell command or a subagent card to open it. Questions, plans and
permission requests wait above the message box with buttons big enough for a thumb.

The mobile app remembers the machine for the name too, with the same **Trust it** warning ("This isn't the machine
you connected to before"). It can't ask a local Tailscale, so it goes by the node id the machine reports about itself:
that catches a reused name or a replaced machine, but not a machine that lies about its id.
