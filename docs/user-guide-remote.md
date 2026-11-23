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

Click **Not sharing** in the status bar to open the Remote tab, then turn on **Share this machine on my tailnet**.
The status bar then reads `Sharing on <machine>.example.ts.net:7287` with the certificate's expiry date, or tells
you why it couldn't start (Tailscale not installed, not running, logged out, or no certificate).

Only your own devices can connect: same Tailscale user, not tagged, and not this machine. Each one must also be
allowed here once:

- The first time a device connects, the status bar reads **<device> (<you>) wants to connect**, and the Remote tab
  lists it under **Waiting for approval** with a code of four groups of four characters. The device shows the same code. Click
  **Allow** only if the codes match; any program on one of your devices can ask, and a different code means it isn't
  the app you just opened. **Deny** turns it down until sharing restarts here.
- Allowed devices are listed under **Allowed devices** (saved in `settings.toml`). **Revoke** closes its connection
  at once and forgets it; it has to ask again.
- An approval is for one SkyeLab install on one machine. A reinstall, or another app on the same machine, asks again.

Connected devices are listed in the Remote tab, each with a **Disconnect** button, which closes the connection at once.
A connected device can list projects and threads, start a thread in an existing project, send follow-ups and
interrupt. A follow-up or interrupt is answered once the agent took it, or with why it couldn't (within 20 seconds). It can't add projects, change accounts or
bindings, or open terminals. Turning sharing off closes every connection.

## The environment switcher

The top of the sidebar (**SkyeLab on This Mac ▾**) picks which machine the window shows. Open it to see your online
devices and recent ones, or type a MagicDNS name such as `desktop.example.ts.net` and click **Connect**. Only full
MagicDNS names work, not IP addresses or short names. The other machine must have sharing turned on. The first
time, it must also allow this device: the status bar says **Waiting for approval** with this device's code; check
that the other machine shows the same code, click **Allow** there, and this window connects within a few seconds.

On a remote machine, the sidebar lists its projects and threads. You can open threads, start new ones, send
follow-ups and stop them. The account shown is the one that machine resolved. Terminals and worktree controls are
local only. Switching back to **This Mac** closes all remote tabs. A thread that starts on this machine while you
look at another one (a delegated thread, or one started from your phone) doesn't open among the remote tabs: the
sidebar shows **This Mac · 1 new thread here**, and clicking it switches back and shows it.

Before connecting, SkyeLab asks this machine's own Tailscale who is at the address it dialled. It connects only to
one of your own devices (same Tailscale user, not tagged), and it remembers that machine's Tailscale node for the
name. If the name later answers from another node, its tabs close and a warning offers **Trust it**: trust it only if
you replaced or renamed that machine. If your Tailscale says the machine isn't yours or can't answer, the status says
so and nothing is sent to it. If the machine claims to be a different node than your Tailscale sees, the status says
so and nothing is listed or sent after the handshake. After a reconnect nothing is asked of the machine until it is
checked again, and a refusal clears its projects, threads and tabs from the window.

The mobile app remembers the machine for the name too, with the same **Trust it** warning ("This isn't the machine
you connected to before"). It can't ask a local Tailscale, so it goes by the node id the machine reports about itself:
that catches a reused name or a replaced machine, but not a machine that lies about its id.
