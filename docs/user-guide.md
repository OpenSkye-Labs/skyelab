# User guide

Shortcuts are shown for macOS. On Linux and Windows, use <kbd>Ctrl</kbd> for <kbd>⌘</kbd>.

## Command palette

Press <kbd>⌘K</kbd> or <kbd>⇧⌘P</kbd> to find anything: an action (with its shortcut), a project to switch to, a
thread to open, or a terminal, Changes or browser tab to go to. With nothing typed it lists the everyday **Actions** (new thread, go
to file, search, add project, pull requests, issues, usage, settings, theme) and your **Recent threads** (the twelve
used last); type to find any other action. Typing searches everything, grouped under **Actions**, **Threads**,
**Projects**, **Scripts** and **Tabs**, best match first, with the matched letters in bold. Threads of every project are
also found by their messages: type a few words from a prompt or an answer and threads with one message holding every
word, in any order, list after the rest, with their project's name and the matched line. Put words in double quotes
(`"hungry lion"`) to find them only written together. Words under three letters
are matched in the same message as the longer ones; a query of only short words (`go to`) looks for them written
together. Type a few letters in order (`os` finds
**Open Settings**), move with the arrow keys (they wrap around) or the mouse, press <kbd>Enter</kbd> to run it and
<kbd>Esc</kbd> to close. Rows with a chevron open a page: **New thread in…** (pick a project), **Change theme**, and on
another machine **Forwarded ports**. <kbd>Esc</kbd>, <kbd>Backspace</kbd> in an empty search, or the Back row above the
search go back from a page. The action runs
where you were, so actions that only make sense in a thread or in Settings are listed only while you're there.
In a terminal on Linux and Windows, <kbd>Ctrl+K</kbd> stays the shell's; use <kbd>Ctrl+Shift+P</kbd> there.
While the window shows another machine, the palette lists that machine's projects and the selected project's
threads, and opens them there. A toggle such as **Toggle Sharing on Tailnet** or **Toggle Archived Threads** shows a ticked box while it is on.

## Keyboard

**F6** and **Shift+F6** move the keyboard between the window's areas: the sidebar, the conversation, the message
input, the right panel and the terminal (with no thread open, the page takes the conversation's place). Hidden areas
are skipped and it wraps around. The area you land on gets a ring until your next click. **Esc** outside a text field
goes back to the message input; in a terminal, Esc stays the shell's. F6 doesn't reach into a browser tab's page.

**⌘F** (Ctrl+F) finds in whatever has the keyboard: a thread or a side chat, the Changes diff, a file preview or a
browser tab. **Enter** / **Shift+Enter** step through the matches and **Esc** closes the bar.

**Settings → Keybindings** lists every action with the keys that run it now (search by name or key), and
**Open keybindings file** opens `keybindings.toml` in SkyeLab's config folder (`~/Library/Application Support/skyelab`
on macOS, `~/.config/skyelab` on Linux), creating it with a commented example. Each rule names a key, an action and,
optionally, where it applies:

```toml
[[binding]]
key = "cmd-shift-t"      # modifiers joined with -: cmd, ctrl, alt, shift; secondary = cmd on macOS, ctrl elsewhere
action = "NewTerminal"   # an action's name from that list; "none" frees the key
context = "Workspace"    # optional: Workspace (the default, the whole window), Thread, Terminal, Settings, ...
```

The built-in keys stay; your rules go on top and win where they share a key. A rule only applies where its context
is: one in `Terminal` works only with a terminal focused, and a text field's own keys still win inside it. Saved
changes apply within a second, no restart. A rule SkyeLab can't use (an unknown action, a bad key, or a context
that is misspelled or one no built-in key uses) is skipped and shown once in a notice and on the Settings page, with its line number; the rest of the file still loads.
The palette and the Welcome page show the keys in effect.

## Files

**Go to File** (<kbd>⌘P</kbd>), **Search in Files** (<kbd>⇧⌘F</kbd>) and the **Preview** tab (which can also edit and save a text file): see
[Files](user-guide-files.md).

## Layout

The sidebar lists the threads of all your projects in one list: **Pinned** first, then **Working** (agents running
now, even on a page you're not looking at), then the rest by when they last changed: **Today**, **Yesterday**, **Previous 7 days** and **Older**. A thread
waiting for your answer is your turn, not the agent's: it lists by time, its row saying what it waits for. Each row
shows the title, then a line with the project (left out while the list is narrowed to one project), the account the
project's folder is bound to (**No account** in the warning color if it has none), the pull request opened from the
thread (**#24**, its icon and color showing open, draft, merged or closed, plus its checks while open) and the branch (the thread's worktree branch, else the branch the project folder has checked out).
Beside the title is the thread's state (**Working 1m 12s**, counting up while the turn runs,
**Approval** while the agent waits for a permission, **Input** for its question, **Plan ready** for a plan to approve, **Failed**, **Interrupted**, **Done** for a finished turn you
haven't seen) or how long ago it last changed. In the thread itself the line above the composer says **Working for 1m 12s**. Until a newly launched agent
reports its session, the line names the step it is on instead: **Starting Claude… 2s** (or Codex, Cursor or Antigravity), **Resuming Claude
session… 2s** when it continues a stored session, and **Starting MCP servers (3)… 4s** while the agent still connects
MCP servers (both at once on a resume). While the agent runs one of the account's own hooks (Codex's `hooks.json`, Claude's settings hooks such
as SessionStart) it says **Running hook sessionStart (…)… 12s** instead, and after 30 seconds a hint says where the hook
comes from and that Interrupt stops the turn. Hold the pointer still on a row for a moment to see its card (a click
holds cards back until the pointer moves again): the project's name and folder, the worktree and branch, the pull request (click to open it), the account with its provider, the bound folder
that picks that account, and when the thread was created and last updated.

Above the list are the search field (it finds threads by title or message text the same way as the palette, and a
row found by a message shows the matched line under its title), saved views (the bookmark button), the project filter (the folder button) and
**New thread** (**+**). The project
filter narrows the list to the projects you tick (**All projects** clears it); it also holds **Manage projects** and
**Add project**. **New thread** starts in the one project the list is narrowed to, or in your only project; otherwise
it asks which project in the palette (**New thread in**), whose last row is **Add project…**. With no project at all
it opens Add project. Neither ever opens the system folder picker.

**Add project** opens a palette page with path completion, recent folders, New project and Clone from URL: see
[user-guide-projects.md](user-guide-projects.md). **Saved views** keep the ticked projects and the search text under a name: **Save current
view…** asks for one (a name you already used replaces that view), picking a view brings both back, and **Delete
view** removes one. The view the list shows now is checked. Views are kept in `settings.toml` (`[[sidebar_views]]`),
projects by folder, so a project you remove is skipped. The **Working** header shows how many threads work now; click it to fold them away.

**Settle and snooze** move a thread off the list until you need it again (a check and an alarm clock on the hovered
row): see [user-guide-managing-threads.md](user-guide-managing-threads.md#settle-and-snooze).

A slim rail on the far left of the window switches what the window shows: **Home** (your threads; it reopens the
sidebar and the last thread you had open), **Bots** (coming soon), **Pull requests**, **Issues** and **Scheduled
tasks** at the top, and **Usage** and **Settings** at the bottom. Scheduled tasks, Usage and Settings open the Settings
window; clicking the highlighted one again closes it. Usage shows a yellow dot when an account is at 75%
of a usage window and a red one at 90% or when it is limited; its tooltip says which. The sidebar's bottom row keeps **Refresh**, which reloads
the thread list.

The center shows one page at a time: a thread, a new thread, Settings, or nothing open yet. Clicking a thread in the
sidebar (or picking it in the palette) switches the page; there are no center tabs. The window follows the thread's
project: the status bar, terminals and Open in editor use its folder and account. A thread you switch away from keeps
running, and its row keeps its state. A thread opens at its latest message, or where you left it scrolled up, also
after a restart. An agent that has sat idle for 10 minutes is ended to free its memory (not
while it has a background command, a subagent or a question open); your next message resumes its session, as after
a restart. A thread that starts on its own (delegated, or from another device) doesn't take
the page: its row says **Done** until you open it (see **Unread** below). While the agent waits on threads it delegated, or on subagents in its turn, a line over the composer says
**Waiting on N subagents** with their names: **Details** opens or closes the thread details card (Lineage lists them),
and **Stop** stops them (a subagent stops with the turn that runs it). Tools open in the right panel beside the
thread: **Changes**, browsers and **Devices**. The right panel's first tab (**+**) is **Open a surface**: Browser
(<kbd>B</kbd>), Terminal (<kbd>T</kbd>), Files (<kbd>F</kbd>), Diff (<kbd>D</kbd>), Pull request (<kbd>P</kbd>),
Linked items (<kbd>L</kbd>), Device (<kbd>M</kbd>) and Side chat (<kbd>S</kbd>). Press the letter while the panel has
focus (<kbd>Esc</kbd> hides the panel), or click the row; a greyed-out surface says why on hover. Terminal opens in the drawer.
The expand button at the panel's top right fills the window with it; press it again or <kbd>Esc</kbd> to put it back. Every right-panel and terminal tab has **Zoom In** / **Zoom Out** in its **…** menu. **Reopen Closed View** (<kbd>⇧⌘T</kbd>) brings
back the tab you closed last in this window: a browser tab on its page, the file preview on its file, or a Changes
tab while its thread is open. Terminals, side chats and agents' browser tabs don't come back.

**Thread details** (<kbd>⇧⌘B</kbd>, <kbd>Ctrl+Shift+B</kbd> on Linux and Windows) opens a card that floats over the
chat at the top right: this machine and the folder, the branch with its git actions and Changes, project scripts,
side chats, the agents this thread delegated (Lineage), every account's usage, the pull request and the editor. It
stays open while you work, beside the conversation; press the button again or <kbd>Esc</kbd> to close it. When the
right panel leaves it no room beside the conversation, it covers the chat and a click outside it closes it.

<kbd>⌘[</kbd> and <kbd>⌘]</kbd> go back and forward between pages (<kbd>Alt+←</kbd> and <kbd>Alt+→</kbd> on Linux
and Windows, outside text fields), and so do a mouse's back and forward buttons.

The page's top row shows where you are (project / thread) and its actions, and at its right end four buttons show
and hide the thread details card (<kbd>⇧⌘B</kbd>), the sidebar (<kbd>⌘B</kbd>), the terminal drawer (<kbd>⌘J</kbd>) and the right panel (<kbd>⌥⌘R</kbd>). These buttons work wherever the focus is, a terminal included. The terminal drawer's buttons: [user-guide-terminal.md](user-guide-terminal.md).
Drag
a panel's edge to resize it. Drag the sidebar's edge almost to the window's left edge and it fades; let go there and
it hides (anywhere else the drag only resizes it, down to its minimum width). A hidden sidebar comes back with
<kbd>⌘B</kbd> at the width it had, or from the thin handle that shows when you point at the middle of the window's
left edge: click it, or drag it to the width you want.

SkyeLab remembers the window's size and position, the right panel's width and whether it was open, and the page it
showed, in `settings.toml`. The first launch opens the window over most of the screen; a saved position on a display that is
no longer connected is ignored.

Several windows, and what quitting does while agents work: [user-guide-windows.md](user-guide-windows.md).

## Settings and themes

<kbd>⌘,</kbd> (or **Settings** in the left rail, the status bar or the palette) opens Settings as a window-sized
dialog over the app; **Esc**, a click outside it or the **×** closes it, and it reopens on the section you left.
Each window has its own. Type in **Search** at the top of its list (or press <kbd>/</kbd>) to find a setting by name.
Sections with preferences have **Restore defaults** in the header.

**Settings → Appearance** picks a theme (each has a light and a dark side): **SkyeLab** (the default), **Classic** (SkyeLab's
original zinc and true black), **Skye**, **Graphite**, **Dawn**, **Grove**, and ports of **Catppuccin**, **Rosé Pine**,
**Tokyo Night**, **Kanagawa**, **Everforest**, **Gruvbox**, **Nord** and **Flexoki**, the **Mode** (System, Light or Dark), an **Accent color**, **Density** (Compact tightens rows) and
the **Text size** and **Code text size**. Changes apply at once in every window and are saved under `[appearance]` in
`settings.toml`. The palette has the same choices under **Change theme**.

Motion is brief and mostly instant: the palette opens and closes at once, dialogs fade in, and a sidebar or panel
you hide slides shut. With **Reduce motion** on in macOS (System Settings → Accessibility → Display), SkyeLab
shows every change at once. It reads that setting when it starts.

**Settings → Usage** compares every account: its 5-hour and weekly limits, and what its turns cost and used today,
in 7 and in 30 days, per account and per project, with a daily chart of the chosen range (Today charts the last 7 days). Each limit bar says how old its reading is.
Readings come from the agents during turns, and also, while Usage or the thread details card is open, from asking
each account's own agent CLI every 5 minutes (`claude` or `codex`, with that account's config folder only; Cursor
doesn't expose rate-limit polling; SkyeLab never reads a login token). An account with no limits to read (signed out, an API key) is asked once an hour. When
asking fails, the account says **Couldn't refresh limits** with the reason under its bars and keeps its last
readings; SkyeLab asks again later, waiting longer after each failure (up to an hour). **Limited** still comes only
from a turn the agent was refused in, or from Codex saying a limit was reached.

**Settings → General → Advanced → Developer mode** is for people working on SkyeLab itself: a **Debug** chip in the
status bar, an overlay with frame times and live counts, and a debug log of network calls and child processes (no
prompts or secrets). [debugging.md](debugging.md) explains it.

## Updates

Checking for, downloading and installing updates: [user-guide-updates.md](user-guide-updates.md).

## Accounts and bindings

First-run onboarding (or **Open Onboarding** in the palette), binding folders to accounts, and a GitHub
login per account: see [user-guide-accounts.md](user-guide-accounts.md).

## Threads

Starting threads, scratch threads, the composer, permissions, queueing and steering:
[user-guide-using-threads.md](user-guide-using-threads.md). Browser tabs, yours and agents':
[user-guide-browser.md](user-guide-browser.md).

## Scheduled tasks

Saved prompts that start a thread on a schedule (intervals, daily, weekdays, cron), missed runs, and the account
rule for unattended runs: [user-guide-scheduled.md](user-guide-scheduled.md).

## Pull requests and issues

The Pull requests and Issues pages, merging and reviewing a PR, and commenting on, closing or reopening an issue:
[user-guide-github.md](user-guide-github.md).

## Worktrees

Click **Local checkout** under the composer (it turns into **New worktree**) before you send the first prompt. The thread then works in its own
git worktree at `<project>/.skyelab/worktrees/<slug>` on a new branch `skyelab/<slug>`, started from the branch the
thread details card's branch row shows as **From <branch>** (the current branch unless you pick another). The `.skyelab` folder is
added to the repo's local `info/exclude`, so it never shows up in `git status`.

The thread details card then has:

- **Changes** opens a diff of everything the agent changed in the worktree, committed or not; **Commit all** counts
  only the files not committed yet, and says **Nothing to commit** when there are none (**Loading…** until the first read lands). It follows the folder live:
  an edit made in your editor or a terminal, a commit or a branch switch shows up within a second, as do the
  composer's change count and branch (see [live updates](user-guide-git.md#live-updates)). **Refresh** reloads it
  by hand. New files count in the card's **Changes vs** line too.
  A file with more than 5,000 diff lines, or the files after the first 50,000 lines in all, show one
  "Diff too large" row with the line count instead: open those in a terminal or your editor.
- **Open terminal here** opens a terminal in the worktree, on the thread's account.
- **Discard worktree** removes the worktree. Stop the agent first (**Stop Agent** in the command palette). If it has uncommitted changes, SkyeLab lists the
  files you would lose and asks you to confirm. The branch is kept.

## Branches, scripts and turns

The branch picker, project scripts, and per-turn checkpoints with rewind: [user-guide-git.md](user-guide-git.md).

## Other machines

Sharing this machine over Tailscale, device approval and the environment switcher: [user-guide-remote.md](user-guide-remote.md).

## Device hub

Open it with <kbd>⇧⌘D</kbd> (<kbd>Ctrl+Alt+D</kbd> on Linux and Windows), or **Devices** in the status bar, pressed while the tab shows. The key or button again, or <kbd>Esc</kbd> in the tab, hides it. It
lists your iOS Simulators (macOS with Xcode) and Android emulators and devices (needs the Android SDK: SkyeLab looks in
`ANDROID_HOME`, `ANDROID_SDK_ROOT`, Android Studio's default folder, then `PATH`). **Boot** starts a device. A booted one has **Shutdown**,
**Screenshot**, **Open URL…** and **Install app…**. **Refresh** reloads the list.

Agents can use a booted simulator or emulator too (the `device_` tools; never a physical phone). An agent opens one for its thread, then takes screenshots
and, on Android, taps, types and presses Home or Back. One thread holds a device at a time; another thread is refused.
The device's row reads **Used by <thread>** and shows the screen after each thing the agent does. **Take back** stops
the agent at once: no agent can use that device again until you click **Let agents use**. Closing or archiving the
thread frees its devices. Agents can't boot a device: boot it here first. iOS Simulators take no taps or typing
from agents (`simctl` has no input commands), only screenshots.
