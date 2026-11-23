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

## Files

**Go to File** (<kbd>⌘P</kbd>), **Search in Files** (<kbd>⇧⌘F</kbd>) and the read-only **Preview** tab: see
[Files](user-guide-files.md).

## Layout

The sidebar lists the threads of all your projects in one list: **Pinned** first, then **Working** (agents running
now, or waiting for your answer, even on a page you're not looking at), then the rest by when they last changed: **Today**, **Yesterday**, **Previous 7 days** and **Older**. Each row
shows the title, then the agent's last reply in two muted lines, then a line with the project, the account the
project's folder is bound to (**No account** in the warning color if it has none), the pull request opened from the
thread (**#24**) and the branch (the thread's worktree branch, else the branch the project folder has checked out).
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
thread: **Changes**, browsers and **Devices**.

<kbd>⌘[</kbd> and <kbd>⌘]</kbd> go back and forward between pages (<kbd>Alt+←</kbd> and <kbd>Alt+→</kbd> on Linux
and Windows, outside text fields), and so do a mouse's back and forward buttons.

The page's top row shows where you are (project / thread) and its actions, and at its right end three buttons show
and hide the sidebar (<kbd>⌘B</kbd>), the terminal drawer (<kbd>⌘J</kbd>) and the right panel (<kbd>⌥⌘R</kbd>). Drag
a panel's edge to resize it.

SkyeLab remembers the window's size and position, the right panel's width and whether it was open, and the page it
showed, in `settings.toml`. The first launch opens the window over most of the screen; a saved position on a display that is
no longer connected is ignored.

## Accounts and bindings

First-run setup on the Welcome page (or **Set Up Accounts** in the palette), binding folders to accounts, and a GitHub
login per account: see [user-guide-accounts.md](user-guide-accounts.md).

## Threads

Starting threads, scratch threads, the composer, permissions, queueing and steering:
[user-guide-using-threads.md](user-guide-using-threads.md).

## Worktrees

Click **Local checkout** under the composer (it turns into **New worktree**) before you send the first prompt. The thread then works in its own
git worktree at `<project>/.skyelab/worktrees/<slug>` on a new branch `skyelab/<slug>`, started from the branch the
Details tab's branch row shows as **From <branch>** (the current branch unless you pick another). The `.skyelab` folder is
added to the repo's local `info/exclude`, so it never shows up in `git status`.

The Details tab then has:

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
