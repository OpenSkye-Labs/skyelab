# User guide

Shortcuts are shown for macOS. On Linux and Windows, use <kbd>Ctrl</kbd> for <kbd>⌘</kbd>.

## Command palette

Press <kbd>⌘K</kbd> or <kbd>⇧⌘P</kbd> to find anything: an action (with its shortcut), a project to switch to, a
thread of the current project to open, or an open tab to go to. Type a few letters in order (`os` finds
**Open Settings**), move with the arrow keys, press <kbd>↩</kbd> to run it and <kbd>Esc</kbd> to close. The action runs
where you were, so actions that only make sense in a thread or in Settings are listed only while you're there.
In a terminal on Linux and Windows, <kbd>Ctrl+K</kbd> stays the shell's; use <kbd>Ctrl+Shift+P</kbd> there.

## Accounts and bindings

SkyeLab runs every agent on the Claude account its folder is bound to. A folder with no binding never runs an agent.

### Add an account

1. Sign in to the account in its own config folder:

   ```sh
   CLAUDE_CONFIG_DIR=~/.claude-work claude auth login
   ```

2. Open Settings (<kbd>⌘,</kbd>). Under **Add account**, enter an id (`work`), a label (`Work (Team)`) and the
   absolute config folder (`/Users/you/.claude-work`), then click **Add**.

Accounts and bindings are saved in `accounts.toml` in the platform config folder:
`~/Library/Application Support/skyelab/` on macOS, `~/.config/skyelab/` on Linux, `%APPDATA%\skyelab\` on Windows.

### Bind a folder

In Settings, under **Folder bindings**, pick the account and click **Bind folder…**. Everything under that folder
now runs on that account. A deeper binding wins over a shallower one, so you can bind `~/work` to your work account
and `~/work/oss-fork` to your personal one. Use **Test folder** to check which account a path resolves to.

The status bar shows the account for the current folder, or **No account bound**. If you send a prompt in an unbound
folder, the thread asks you to bind it first and offers a **Bind this folder to an account** button.

### Stale bindings and Re-confirm

When you bind a folder, SkyeLab records the real folder the path pointed to at that moment. If the path later
points somewhere else, the binding turns **stale**: it shows a yellow warning and no agent starts under it. This
happens when:

- the path became a symlink, or a symlink was pointed at another folder (**Not the folder you confirmed any more**),
- the binding was written by an older SkyeLab that didn't record the folder (**Not confirmed yet**),
- the path can't be resolved at all, for example a broken link (**Can't be resolved now**).

The warning shows the folder the path points to now and the one you confirmed. If the new folder is right, click
**Re-confirm**. If not, fix the folder on disk or **Remove** the binding.

## GitHub per account

Give an account its own GitHub login: in Settings, set the account's GitHub config dir (an absolute path such as
`/Users/me/.config/gh-work`) and press **Log in to GitHub**. That opens a terminal running `gh auth login` with
`GH_CONFIG_DIR` set to that dir. SkyeLab never sees the token. Settings then shows which GitHub user is logged in.

Agents, project terminals and the Changes tab's **Commit**, **Push** and **Open PR** use the folder's account's
GitHub login. If the account has none, Push and Open PR refuse instead of using your global `gh` login. Push only
works for https remotes without credentials in the URL; ssh remotes are refused, because an ssh key isn't tied to
an account.

## Threads

1. Click **Add** under Projects and pick a folder.
2. Select the project and click **New** under Threads.
3. Type a prompt and press <kbd>⌘↩</kbd> (or click **Send**).

The thread header shows the account it runs on. **Interrupt** stops the current turn. **Stop** ends the agent.
Threads are saved, and opening an old one and sending a follow-up resumes the same Claude session.

The bottom panel (<kbd>⌘J</kbd>) holds terminals. <kbd>⌥⌘T</kbd> opens a new one. <kbd>⌥⌘B</kbd> opens a browser
tab for your dev server.

### Delegated threads

An agent can hand work to another thread with SkyeLab's MCP tools (`start_thread`, `send_message`,
`wait_thread`, and others). The new thread opens as a tab. Its header says **Delegated by** and links back to the
parent, and the parent lists its **Delegated threads**. An agent can only start threads in projects bound to its own
account; a start into a project on another account is refused and no thread is created. A delegated thread can
delegate once more (two levels), and a thread can have four children running at a time. Not available on Windows yet.

## Worktrees

Tick **Run in a new worktree** in the composer before you send the first prompt. The thread then works in its own
git worktree at `<project>/.skyelab/worktrees/<slug>` on a new branch `skyelab/<slug>`. The `.skyelab` folder is
added to the repo's local `info/exclude`, so it never shows up in `git status`.

The thread header then has:

- **Changes** opens a diff of everything the agent changed in the worktree, committed or not. **Refresh** reloads it.
- **Open terminal here** opens a terminal in the worktree, on the thread's account.
- **Discard worktree** removes the worktree. Stop the agent first. If it has uncommitted changes, SkyeLab lists the
  files you would lose and asks you to confirm. The branch is kept.

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

Only your own devices can connect: same Tailscale user, not tagged, and not this machine. Connected devices are
listed in the Remote tab, each with a **Disconnect** button. A connected device can list projects and threads,
start a thread in an existing project, send follow-ups and interrupt. It can't add projects, change accounts or
bindings, or open terminals. Turning sharing off closes every connection.

## The environment switcher

The top of the sidebar (**SkyeLab on This Mac ▾**) picks which machine the window shows. Open it to see your online
devices and recent ones, or type a MagicDNS name such as `desktop.example.ts.net` and click **Connect**. Only full
MagicDNS names work, not IP addresses or short names. The other machine must have sharing turned on.

On a remote machine, the sidebar lists its projects and threads. You can open threads, start new ones, send
follow-ups and stop them. The account shown is the one that machine resolved. Terminals and worktree controls are
local only. Switching back to **This Mac** closes all remote tabs.

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

## Device hub

Open it with <kbd>⇧⌘D</kbd> (<kbd>Ctrl+Alt+D</kbd> on Linux and Windows), or **Devices** in the status bar. It
lists your iOS Simulators (macOS with Xcode) and Android emulators and devices (needs the Android SDK: SkyeLab looks in
`ANDROID_HOME`, `ANDROID_SDK_ROOT`, Android Studio's default folder, then `PATH`). **Boot** starts a device. A booted one has **Shutdown**,
**Screenshot**, **Open URL…** and **Install app…**. **Refresh** reloads the list.
