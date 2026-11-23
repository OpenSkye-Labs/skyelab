# User guide

Shortcuts are shown for macOS. On Linux and Windows, use <kbd>Ctrl</kbd> for <kbd>⌘</kbd>.

## Command palette

Press <kbd>⌘K</kbd> or <kbd>⇧⌘P</kbd> to find anything: an action (with its shortcut), a project to switch to, a
thread of the current project to open, or a terminal, Changes or browser tab to go to. Threads of every project are
also found by their messages: type a few words from a prompt or an answer (three letters or more) and matching
threads list after the rest, with their project's name. Type a few letters in order (`os` finds
**Open Settings**), move with the arrow keys, press <kbd>↩</kbd> to run it and <kbd>Esc</kbd> to close. The action runs
where you were, so actions that only make sense in a thread or in Settings are listed only while you're there.
In a terminal on Linux and Windows, <kbd>Ctrl+K</kbd> stays the shell's; use <kbd>Ctrl+Shift+P</kbd> there.
While the window shows another machine, the palette lists that machine's projects and the selected project's
threads, and opens them there. A toggle such as **Toggle Sharing on Tailnet** shows whether it is **On** or **Off**.

## Keyboard

**F6** and **Shift+F6** move the keyboard between the window's areas: the sidebar, the conversation, the message
input, the right panel and the terminal (with no thread open, the page takes the conversation's place). Hidden areas
are skipped and it wraps around. The area you land on gets a ring until your next click. **Esc** outside a text field
goes back to the message input; in a terminal, Esc stays the shell's. F6 doesn't reach into a browser tab's page.

**Settings → Keyboard shortcuts** lists every action with the keys that run it now (search by name or key), and
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

**Go to File** (<kbd>⌘P</kbd>), **Search in Files** (<kbd>⇧⌘F</kbd>) and the read-only **Preview** tab: see
[Files](user-guide-files.md).

## Layout

The sidebar lists the threads of all your projects in one list: **Pinned** first, then **Working** (agents running
now, or waiting for your answer, even on a page you're not looking at), then the rest by when they last changed: **Today**, **Yesterday**, **Previous 7 days** and **Older**. Each row
shows the title, then the agent's last reply in two muted lines, then a line with the project, the account the
project's folder is bound to (**No account** in the warning color if it has none), the pull request opened from the
thread (**#24**, its icon and color showing open, draft, merged or closed, plus its checks while open) and the branch (the thread's worktree branch, else the branch the project folder has checked out).
Beside the title is the thread's state (**Working 1m 12s**, counting up while the turn runs,
**Approval** while the agent waits for your answer, **Failed**, **Interrupted**, **Done** for a finished turn you
haven't seen) or how long ago it last changed. Hold the pointer still on a row for a moment to see its card (a click
holds cards back until the pointer moves again): the project's name and folder, the worktree and branch, the pull request (click to open it), the account with its provider, the bound folder
that picks that account, and when the thread was created and last updated.

Above the list are the search field, the project filter (the folder button) and **New thread** (**+**). The project
filter narrows the list to the projects you tick (**All projects** clears it); it also holds **Manage projects** and
**Add project**. **New thread** starts in the one project the list is narrowed to, or in your only project; otherwise
it asks which project. **Settings** is at the bottom of the sidebar.

The center shows one page at a time: a thread, a new thread, Settings, or nothing open yet. Clicking a thread in the
sidebar (or picking it in the palette) switches the page; there are no center tabs. The window follows the thread's
project: the status bar, terminals and Open in editor use its folder and account. A thread you switch away from keeps
running, and its row keeps its state. A thread that starts on its own (delegated, or from another device) doesn't take
the page: its row says **Done** until you open it (see **Unread** below). Tools open in the right panel beside the
thread: **Changes**, browsers and **Devices**. The right panel's first tab (**+**) is **Open a surface**: Browser
(<kbd>B</kbd>), Terminal (<kbd>T</kbd>), Files (<kbd>F</kbd>), Diff (<kbd>D</kbd>), Pull request (<kbd>P</kbd>),
Linked items (<kbd>L</kbd>), Device (<kbd>M</kbd>) and Side chat (<kbd>S</kbd>). Press the letter while the panel has
focus, or click the row; a surface the shown thread can't have (no pull request linked, no thread open) is greyed out.
The expand button at the panel's top right fills the window with it.

**Thread details** (<kbd>⇧⌘B</kbd>, <kbd>Ctrl+Shift+B</kbd> on Linux and Windows) opens a card that floats over the
chat at the top right: this machine and the folder, the branch with its git actions and Changes, project scripts,
side chats, the agents this thread delegated (Lineage), every account's usage, the pull request and the editor. It
stays open while you work, beside the conversation; press the button again or <kbd>Esc</kbd> to close it. When the
right panel leaves it no room beside the conversation, it covers the chat and a click outside it closes it.

<kbd>⌘[</kbd> and <kbd>⌘]</kbd> go back and forward between pages (<kbd>Alt+←</kbd> and <kbd>Alt+→</kbd> on Linux
and Windows, outside text fields), and so do a mouse's back and forward buttons.

The page's top row shows where you are (project / thread) and its actions, and at its right end four buttons show
and hide the thread details card (<kbd>⇧⌘B</kbd>), the sidebar (<kbd>⌘B</kbd>), the terminal drawer (<kbd>⌘J</kbd>) and the right panel (<kbd>⌥⌘R</kbd>). Drag
a panel's edge to resize it.

SkyeLab remembers the window's size and position, the right panel's width and whether it was open, and the page it
showed, in `settings.toml`. The first launch opens the window over most of the screen; a saved position on a display that is
no longer connected is ignored.

**New Window** (<kbd>⇧⌘N</kbd>, <kbd>Ctrl+Shift+N</kbd> on Linux and Windows, the palette, or the SkyeLab menu on
macOS) opens another window on the same projects, threads and accounts. Each window has its own page, project filter,
docks and terminals. A thread open in two windows is the same thread with one agent: both windows show it as it runs,
and its message input follows the window that opened it last, with its text and files (undo starts over there); the other window shows the draft, and clicking it brings the input there. A notification for it comes once. Saving accounts in
Settings applies to every window, and a window catches up with projects and threads changed elsewhere when you switch
to it. Each window scrolls a thread on its own. Every open window saves its frame and page, and the next launch opens all
of them again; a window you close leaves that list, except the last one, so a launch always has a window to reopen (quitting keeps every window).
Closing the last window quits on Linux and Windows. On macOS SkyeLab keeps running, agents included, until you quit
(<kbd>⌘Q</kbd>); click its Dock icon to open a window again.

## Updates

Once a day at most, when SkyeLab starts (and every hour after that it checks whether a day has passed), it asks
GitHub for the latest release of SkyeLab. When that release is newer than yours, a small notice at the bottom of the
sidebar offers **Download**, which opens the release page in your browser, and **Skip this version**, which hides it
until a newer one comes out. **Check for Updates** (the palette, or the SkyeLab menu on macOS) and **Check now** in
**Settings → About** check right away, skipped version included. SkyeLab never installs anything itself, and the
request carries nothing about you. **Settings → About → Check for updates** turns the daily check off.

## Accounts and bindings

First-run setup on the Welcome page (or **Set Up Accounts** in the palette), binding folders to accounts, and a GitHub
login per account: see [user-guide-accounts.md](user-guide-accounts.md).

## Threads

Starting threads, scratch threads, the composer, permissions, queueing and steering:
[user-guide-using-threads.md](user-guide-using-threads.md).

## Scheduled tasks

Saved prompts that start a thread on a schedule (intervals, daily, weekdays, cron), missed runs, and the account
rule for unattended runs: [user-guide-scheduled.md](user-guide-scheduled.md).

## Pull requests and issues

**Open Pull Requests** and **Open Issues** in the command palette show one page each. Every project whose folder is
bound to an account is listed under its name and account, read with that account's own `gh` login and never
another's; the repo is the project's `origin` (a github.com https or ssh URL). An account with no GitHub login shows
**Sign in to GitHub** and nothing is asked. Up to 50 open items per project are read, once per filter (**Mine**,
**Review requested**, **All** for PRs; **Assigned to me**, **All** for issues) until you press refresh.

- Click a PR to start a thread in its project with a review prompt; the PR is linked to the thread when you send it.
  The arrow icon opens the PR in your browser.
- Click an issue to start a thread with the issue (title, link, labels, the start of its text) as a context chip.
  Click a label to show only that label's issues; click it again (or the **Label** button) to show all.

## Worktrees

Click **Local checkout** under the composer (it turns into **New worktree**) before you send the first prompt. The thread then works in its own
git worktree at `<project>/.skyelab/worktrees/<slug>` on a new branch `skyelab/<slug>`, started from the branch the
thread details card's branch row shows as **From <branch>** (the current branch unless you pick another). The `.skyelab` folder is
added to the repo's local `info/exclude`, so it never shows up in `git status`.

The thread details card then has:

- **Changes** opens a diff of everything the agent changed in the worktree, committed or not. **Refresh** reloads it.
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

Open it with <kbd>⇧⌘D</kbd> (<kbd>Ctrl+Alt+D</kbd> on Linux and Windows), or **Devices** in the status bar. It
lists your iOS Simulators (macOS with Xcode) and Android emulators and devices (needs the Android SDK: SkyeLab looks in
`ANDROID_HOME`, `ANDROID_SDK_ROOT`, Android Studio's default folder, then `PATH`). **Boot** starts a device. A booted one has **Shutdown**,
**Screenshot**, **Open URL…** and **Install app…**. **Refresh** reloads the list.
