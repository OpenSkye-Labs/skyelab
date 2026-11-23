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

Binding folders to accounts, and a GitHub login per account: see [user-guide-accounts.md](user-guide-accounts.md).

## Threads

1. Click **Add** under Projects and pick a folder.
2. Select the project and click **New** under Threads.
3. Type a prompt and press <kbd>⌘↩</kbd> (or click **Send**).

### The composer

- **Attach** (the paperclip), drag files onto the composer, or paste an image or copied files. Images (PNG, JPEG,
  GIF, WebP), PDFs and text files work, up to 5 MB each and 10 per message; each shows as a chip you can remove.
  Claude gets them as image and document blocks. Codex gets images and text files, not PDFs.
- **`@`** lists the files git tracks in the folder (ignored and untracked files don't show). Type to narrow it,
  then <kbd>↩</kbd> or <kbd>Tab</kbd> inserts `@path`; <kbd>Esc</kbd> closes it.
- **`/`** at the start lists commands: the CLI's own (`/compact`, `/context`, `/cost`, `/init`, `/review`, …), your
  account's `commands/` folder (in its config dir) and the project's `.claude/commands/`. SkyeLab starts Claude
  with only your user settings, so a project command may not run. Codex has no slash commands.
- **`$`** lists your account's skills (`skills/<name>/SKILL.md` in its config dir). Picking one inserts `/name` for
  Claude and `$name` for Codex. Commands and skills come only from the account the folder is bound to.
- **The model chip** (right of the composer: **Opus · High**, or **Default model** until you pick) sets the model and effort for this
  thread: Opus, Sonnet, Haiku or Fable with effort Low to Max for Claude, or the models Codex lists for your account.
  A pick is kept on the thread and becomes the project's default for new threads. A running Claude agent switches
  model from the next message; a new effort applies the next time it starts.
- **While the agent works**, Send queues the message instead: it shows above the composer with **Sends when the
  agent finishes**, and goes as soon as the turn ends, in order. **Steer** on a queued message, or
  <kbd>⇧⌘↩</kbd> while typing, sends it into the running turn now (it stays part of that turn, inside its fold). A `/command` always waits for the turn to end.
  Messages from another device or another agent are sent at once, as before.
- **When the composer is narrow** (the right panel open), the permission chip shows only its icon and the model chip
  only the model.
- **The strip under the composer** shows this machine's name, the account, **Local checkout** or **Worktree** (click
  it before the first send to run in a new worktree), how many files changed (click it to open Changes, refreshed
  after each turn) and the current branch.

The thread header is a breadcrumb (the project's name as the sidebar shows it / parent thread / title; hover it for
the folder's path) with the provider (**Claude** or **Codex**) and
the account it runs on. While a turn runs, **Working for 1m 12s** above the composer counts how long, and while the
composer is empty, Send turns into **Interrupt**, which stops the current turn (it shows **Stopping…** until the turn
ends; after 10 seconds without an answer it works again and **Force stop** appears). <kbd>⌘.</kbd> interrupts too.
**Stop Agent** (command palette, or <kbd>⌥⌘.</kbd>) ends the agent; there is no Stop button. Threads are saved, and opening an old one and sending a follow-up resumes the
same session (Claude's `--resume`, or Codex's `thread/resume`). If the thread store can't save an event (a full disk,
another SkyeLab on the same data), the thread still shows it and says how many events weren't saved. SkyeLab retries every few seconds and with each new
event, and saves them in order once the store works again. Quitting before then loses them.
Switching to another page doesn't stop a thread's agent; **Stop** or quitting SkyeLab does. A thread that was still running when SkyeLab last closed
without stopping it (a crash, say) shows as **interrupted**; send a follow-up to resume it.

### Thread details

The right panel's **Details** tab (the right-panel toggle opens it) holds the thread's folder, branch, git actions,
scripts, delegated agents, pull request and editor, and every account's usage. Details and the dialog backdrop:
[user-guide-thread-details.md](user-guide-thread-details.md).

### Permissions, questions and plans

The composer's permission chip (**Ask ⌄**) picks what the agent may do without asking (**Full access** confirms
first). Approvals, the agent's questions and plans show as cards in the thread. Each mode, and how answering works:
[user-guide-permissions.md](user-guide-permissions.md).

### What the timeline shows

- Text and thinking stream in word by word as the agent writes. Thinking is a **Thought** row; click it to read it.
  Complete blocks are saved. Text a turn cut off (Stop, Interrupt, a crash) is saved as far as it got and marked
  **(interrupted)**, so a reopened thread still shows it.
- The agent's todo list (Claude's `TodoWrite`, Codex's plan) is one checklist per turn that updates in place.
- Edits (`Edit`, `Write`, `MultiEdit`) show their diff inline. Click the file name to open it in the Changes tab.
  File links in the agent's text (`[a.rs](src/a.rs)`) do the same; web links open in your browser, and nothing else opens.
- Shell commands show as `$ command` with how they ended (`exit 1`, `failed`); click one for its output.
- Messages and code blocks have copy buttons; under a turn's final reply the copy button sits next to the time the
  turn ended.
- A finished turn folds its work (tool calls, thinking, the text in between) under one line above the reply:
  **Worked for 4.2s · 12.3k in · 175 out · $0.0421 ›**. Click it to open or fold the work. A running turn shows
  everything; one that failed, or was stopped before it finished, isn't folded and keeps its line after the turn.
- When the account hits its usage limit, the thread says **Limited until it resets in …** above the composer and the
  sidebar shows **Limited**, until a turn succeeds again.
- If a Claude thread's earlier session can't be found when you continue it (its history was deleted, or it ran under
  another config dir), the thread says so and runs your message in a new session. A Codex thread shows the failure.

### Finding and managing threads

- **Search.** The field at the top of the sidebar filters the listed threads by title and by what was
  said in them (your prompts and the agent's answers, not tool output).
- **Right-click a thread** to **Rename**, **Pin** (pinned threads list first, with a star), **Archive** or **Delete**
  it. Archived threads move under **Archived** at the end of the list; click it to show them. Archive and delete
  show a notice with **Undo**. A deleted thread is gone for good about eight seconds later (or at the next start if
  you quit first). A thread that is working can't be archived or deleted (stop it first), and one with its own
  worktree can't be deleted until you discard the worktree.
- **Manage a project** from the project filter: **Manage projects**, then the project, then **Rename**,
  **Open in editor**, **Account bindings…** (opens Settings) or **Remove from SkyeLab**. **Switch to project** in the
  palette narrows the list to that project. Removing never touches the folder or its
  files, and keeps the threads' history: add the same folder again and the project comes back with its threads. If
  agents are running in it, SkyeLab asks first, and removing stops them.
- **Unread.** When a turn finishes in a thread you aren't looking at, or a thread starts in the background
  (delegated, or from another device), the thread's title turns bold and its row says **Done** until you open it. You
  also get a notice: a toast in the window if SkyeLab is in front, or a system notification if it
  is in the background (system notifications need the bundled app; macOS asks for permission the first time).
  Archived threads stay quiet.
- **Keys.** <kbd>⌘N</kbd> new thread, <kbd>⇧⌘]</kbd> / <kbd>⇧⌘[</kbd> next / previous thread, <kbd>⌘1</kbd>–<kbd>⌘9</kbd>
  jump to a thread in sidebar order, <kbd>⌘O</kbd> open the folder in your editor. Each switches the page, so
  <kbd>⌘[</kbd> / <kbd>⌘]</kbd> (back / forward) walk back through them. All are in the palette. On Linux
  and Windows these <kbd>Ctrl</kbd> keys stay the shell's while a terminal is focused.
- **Open in editor** (<kbd>⌘O</kbd>) opens the shown thread's folder (its worktree, if it has one), else the project's.
  It runs `editor` from `settings.toml` (for example `editor = "zed --new"`; split on spaces, no shell), else
  `$VISUAL`, else `$EDITOR`, else `code` or `zed` from your PATH. The editor gets a terminal's environment, never an
  account's config dir.

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
`browser_url`, run `browser_eval` (a JavaScript expression; the JSON result comes back, up to 64 KiB, within 10
seconds) and take a `browser_screenshot` (a PNG of the visible page, at most 1600 px wide and 1 MiB; macOS only). SkyeLab doesn't keep image bytes in its thread history: the tool row shows **Image (205 KB)**
while the thread is open, and **Image (205 KB) not kept** after a restart. The same goes for any image or PDF a tool
returns. Agents may only load http and https pages, including `localhost` dev servers, and never run scripts in a
local file page. Each thread has one tab, titled **Agent browser**, with a bar saying the agent controls it:

- **Take over** makes it your tab. The agent can't reach it any more; its next `browser_open` opens a new tab.
- **Close** closes the tab.

An agent can't see or drive the tabs you open with <kbd>⌥⌘B</kbd>, or another thread's tab. Its tab has private,
in-memory cookies and storage, so it is not signed in to anything you are, and nothing it sets reaches your tabs.
Take over reloads the page, which stops any script the agent left running. While the agent still has the tab,
don't sign in to anything in it: the agent can read that page. macOS and Linux only.

### Delegated threads

An agent can hand work to another thread with SkyeLab's MCP tools (`start_thread`, `send_message`,
`wait_thread`, and others). The new thread appears in the sidebar marked unread; the page you're on stays. Its header's breadcrumb links back to the
parent, and the parent's Details tab lists it under **Lineage**. An agent can only start threads in projects bound to its own
account; a start into a project on another account is refused and no thread is created. A delegated thread can
delegate once more (two levels), and a thread can have four children running at a time. That limit also holds
when a stopped child is sent a new message, by its parent, from another device or by you: it stays stopped and its
header says why. Once you stop a thread, it can't start or message other threads, even with a call
it had already sent. Codex threads get the same tools, with the same account rule. Not available on Windows yet.

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
