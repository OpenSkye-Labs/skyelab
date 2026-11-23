# User guide

Shortcuts are shown for macOS. On Linux and Windows, use <kbd>Ctrl</kbd> for <kbd>⌘</kbd>.

## Command palette

Press <kbd>⌘K</kbd> or <kbd>⇧⌘P</kbd> to find anything: an action (with its shortcut), a project to switch to, a
thread of the current project to open, or an open tab to go to. Type a few letters in order (`os` finds
**Open Settings**), move with the arrow keys, press <kbd>↩</kbd> to run it and <kbd>Esc</kbd> to close. The action runs
where you were, so actions that only make sense in a thread or in Settings are listed only while you're there.
In a terminal on Linux and Windows, <kbd>Ctrl+K</kbd> stays the shell's; use <kbd>Ctrl+Shift+P</kbd> there.
While the window shows another machine, the palette lists that machine's projects and the selected project's
threads, and opens them there. A toggle such as **Toggle Sharing on Tailnet** shows whether it is **On** or **Off**.

## Layout

The sidebar lists your projects, each with the account its folder is bound to; the selected project's threads sit
under it with their state (**Working**, **Interrupted**, **Failed**) or how long ago they last changed. Threads open
in the center. Tools open in the right panel beside the thread: **Changes**, browsers and **Devices**. Settings and
Remote open in the center.

The three buttons at the right end of the center's tab bar show and hide the sidebar (<kbd>⌘B</kbd>), the terminal
drawer (<kbd>⌘J</kbd>) and the right panel (<kbd>⌥⌘R</kbd>). Drag a panel's edge to resize it.

SkyeLab remembers the window's size and position and the right panel's width and whether it was open, in
`settings.toml`. The first launch opens the window over most of the screen; a saved position on a display that is
no longer connected is ignored.

## Accounts and bindings

SkyeLab runs every agent on the account its folder is bound to: a Claude Code account or an OpenAI Codex account.
A folder with no binding never runs an agent.

### Add an account

1. Sign in to the account in its own config folder:

   ```sh
   CLAUDE_CONFIG_DIR=~/.claude-work claude auth login
   ```

2. Open Settings (<kbd>⌘,</kbd>). Under **Add account**, pick **Claude** or **Codex**, enter an id (`work`), a label
   (`Work (Team)`) and the absolute config folder (`/Users/you/.claude-work`), then click **Add**.

For a Codex account the config folder is its `CODEX_HOME` (for example `/Users/you/.codex-work`). After adding it,
press **Log in to Codex** on its row: that opens a terminal running `codex login` with `CODEX_HOME` set to that
folder (SkyeLab creates the folder first, private to you). A config file written before Codex support has no
`provider` key; those accounts are Claude. An account's provider is fixed once added (its threads hold that provider's
sessions); to switch, add a new account and re-bind the folder.

Codex threads run `codex app-server` in the `workspace-write` sandbox by default: Codex can edit files in the folder
but not outside it, and has no network for its commands. When it wants to run a command it doesn't trust or step
outside the sandbox, the thread asks you (see [Permissions](#permissions)). The repo's own `.codex/` config, hooks and rules never load; your account's own
`CODEX_HOME/config.toml` (MCP servers, model) does.

Accounts and bindings are saved in `accounts.toml` in the platform config folder:
`~/Library/Application Support/skyelab/` on macOS, `~/.config/skyelab/` on Linux, `%APPDATA%\skyelab\` on Windows.

### Bind a folder

In Settings, under **Folder bindings**, pick the account and click **Bind folder…**. Everything under that folder
now runs on that account. A deeper binding wins over a shallower one, so you can bind `~/work` to your work account
and `~/work/oss-fork` to your personal one. Use **Test folder** to check which account a path resolves to.

The status bar shows the account for the current folder, or **No account** in the warning color. If you send a prompt in an unbound
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
works for https remotes without credentials in the URL, and checks every push URL of the remote; ssh remotes are
refused, because an ssh key isn't tied to an account.

An agent's own `git push` (in its shell) also asks only the account's `gh` for https credentials, never your
keychain or a helper from `~/.gitconfig`. Agents don't get your ssh-agent, and git's ssh transport fails for them.
To let one account's agents push over ssh anyway, add `ssh_agent = true` to that `[[account]]` in `accounts.toml`.

**Commit** commits the ticked files, or with none ticked exactly the working tree the Changes tab shows (the
button says how many files). If anything changed since the tab loaded, Commit refuses; refresh, look again, and
commit. Repo hooks run on Commit and Push, with the same clean environment; if a commit hook adds files you
didn't review, the commit is undone and they stay staged. Push never pushes submodules.

## Threads

1. Click **Add** under Projects and pick a folder.
2. Select the project and click **New** under Threads.
3. Type a prompt and press <kbd>⌘↩</kbd> (or click **Send**).

The thread header shows the provider (**Claude** or **Codex**) and the account it runs on. **Interrupt** stops the
current turn. **Stop** ends the agent. Threads are saved, and opening an old one and sending a follow-up resumes the
same session (Claude's `--resume`, or Codex's `thread/resume`). If the thread store can't save an event (a full disk,
another SkyeLab on the same data), the thread still shows it and says how many events weren't saved. SkyeLab retries every few seconds and with each new
event, and saves them in order once the store works again. Closing the tab or quitting before then loses them.
Closing a thread's tab or quitting SkyeLab stops its agent. A thread that was still running when SkyeLab last closed
without stopping it (a crash, say) shows as **interrupted**; send a follow-up to resume it.

### Permissions

Under the composer, **Permissions** picks what the thread's agent may do without asking:

- **Ask** (the default): it asks before anything its own rules don't already allow.
- **Plan**: it reads and plans only, with no edits or commands (for Codex, a read-only sandbox).
- **Accept edits**: file edits in the folder go ahead; anything else asks. (For Codex this is the same as Ask: its
  sandbox already lets it edit in the folder without asking.)
- **Full access**: nothing asks. SkyeLab asks you to confirm first. A Codex agent also leaves its sandbox, so it can
  write outside the folder and use the network.

The mode is saved with the thread and used every time its agent starts or resumes. Changing it while the agent runs
applies at once for Claude and from the next turn for Codex. Claude takes Full access only when it starts: switching a
running Claude thread to Full access saves the choice and says so; Stop it and send a message to continue in Full
access.

When the agent asks, the thread shows a **Waiting for approval** card with the tool and what it wants to do (the
command, or the file), and the agent waits until you answer: **Allow once**, **Allow for this thread** (Claude's own suggested rule
for this call, or this exact command when it has none; while this agent runs; after a resume it asks again) or **Deny**. Afterwards the row says what was decided.
An ask the agent withdraws (an interrupt, its turn ended, or the agent stopped) shows **Withdrawn**. A request in a thread whose agent has
stopped can't be answered; send a message to continue. Another machine viewing the thread, and the mobile app, show
the same card and can answer it.

When a turn fails, or the agent exits with an error, what it wrote to stderr (the last 64 KiB) shows as an
**Agent output** row; click it to open it. Claude's `AskUserQuestion` also arrives as a request: allowing it runs the
tool, but SkyeLab has no way to answer its question yet.

The bottom panel (<kbd>⌘J</kbd>) holds terminals. <kbd>⌥⌘T</kbd> opens a new one; its tab shows the account it runs
as. <kbd>⌥⌘B</kbd> opens a browser tab for your dev server in the right panel.

A terminal runs as the folder's account: `claude` or `codex` typed there uses that account's config dir, and
`gh` and `git push` over https use its GitHub login when it has one (without one, `gh` keeps your default login
and git over https gets no credentials). The other provider's home, and in an unbound folder every home and the
GitHub dir, point at an empty private folder under `<data dir>/skyelab/unbound` (one per account), so the CLI asks
you to log in instead of using your default account. ssh keys work there as usual. API keys and tokens from the environment SkyeLab was started with
(`ANTHROPIC_API_KEY`, `CLAUDE_CODE_OAUTH_TOKEN`, `GH_TOKEN`, `AWS_*`, ...) don't reach it; your shell's own rc
files still run, so a key they export is set again.

In a terminal, the wheel or trackpad scrolls back through the last 10,000 lines, and so do
<kbd>Shift+PageUp</kbd>/<kbd>Shift+PageDown</kbd>; new output doesn't move you while you're scrolled up, and typing
jumps back to the bottom. Drag to select, double-click for a word, triple-click for a line, then <kbd>⌘C</kbd> to copy
(<kbd>Ctrl+Shift+C</kbd> on Linux and Windows); <kbd>⌘V</kbd> (<kbd>Ctrl+Shift+V</kbd>) pastes. When a program
such as vim or htop uses the mouse, clicks and the wheel go to it; hold <kbd>Shift</kbd> to select instead. Hold
<kbd>⌘</kbd> (<kbd>Ctrl</kbd>) over a link to underline it and click to open it: `http` and `https` addresses open in
your browser (no other kind ever does), and a file path jumps to that file in an open Changes tab that lists it.
Relative paths are read from the folder the terminal started in.

### Agent browser tabs

An agent can open a browser tab of its own with SkyeLab's `browser_open` tool, then `browser_navigate`, read
`browser_url` and run `browser_eval` (a JavaScript expression; the JSON result comes back, up to 64 KiB, within 10
seconds). Agents may only load http and https pages, including `localhost` dev servers, and never run scripts in a
local file page. Each thread has one tab, titled **Agent browser**, with a bar saying the agent controls it:

- **Take over** makes it your tab. The agent can't reach it any more; its next `browser_open` opens a new tab.
- **Close** closes the tab.

An agent can't see or drive the tabs you open with <kbd>⌥⌘B</kbd>, or another thread's tab. Its tab has private,
in-memory cookies and storage, so it is not signed in to anything you are, and nothing it sets reaches your tabs.
Take over reloads the page, which stops any script the agent left running. While the agent still has the tab,
don't sign in to anything in it: the agent can read that page. macOS and Linux only. There is no screenshot tool yet.

### Delegated threads

An agent can hand work to another thread with SkyeLab's MCP tools (`start_thread`, `send_message`,
`wait_thread`, and others). The new thread opens as a tab. Its header says **Delegated by** and links back to the
parent, and the parent lists its **Delegated threads**. An agent can only start threads in projects bound to its own
account; a start into a project on another account is refused and no thread is created. A delegated thread can
delegate once more (two levels), and a thread can have four children running at a time. That limit also holds
when a stopped child is sent a new message, by its parent, from another device or by you: it stays stopped and its
tab says why. Once you stop a thread (or close its tab), it can't start or message other threads, even with a call
it had already sent. Codex threads get the same tools, with the same account rule. Not available on Windows yet.

## Worktrees

Tick **Run in a new worktree** in the composer before you send the first prompt. The thread then works in its own
git worktree at `<project>/.skyelab/worktrees/<slug>` on a new branch `skyelab/<slug>`. The `.skyelab` folder is
added to the repo's local `info/exclude`, so it never shows up in `git status`.

The thread header then has:

- **Changes** opens a diff of everything the agent changed in the worktree, committed or not. **Refresh** reloads it.
  A file with more than 5,000 diff lines, or the files after the first 50,000 lines in all, show one
  "Diff too large" row with the line count instead: open those in a terminal or your editor.
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

## Device hub

Open it with <kbd>⇧⌘D</kbd> (<kbd>Ctrl+Alt+D</kbd> on Linux and Windows), or **Devices** in the status bar. It
lists your iOS Simulators (macOS with Xcode) and Android emulators and devices (needs the Android SDK: SkyeLab looks in
`ANDROID_HOME`, `ANDROID_SDK_ROOT`, Android Studio's default folder, then `PATH`). **Boot** starts a device. A booted one has **Shutdown**,
**Screenshot**, **Open URL…** and **Install app…**. **Refresh** reloads the list.
