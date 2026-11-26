# User guide: finding and managing threads

[Back to threads](user-guide-using-threads.md)

- **Search.** The field at the top of the sidebar filters the listed threads by title and by what was
  said in them (your prompts and the agent's answers, not tool output). Every word must match, in any order;
  `"words in quotes"` must be written together. A row found by a message shows the matched line.
- Generated titles, drafts, prompt history (<kbd>↑</kbd>), Settings → Chat and **Fork from here**:
  [user-guide-threads.md](user-guide-threads.md).
- **Right-click a thread** to **Rename**, **Pin** (pinned threads list first, with a star), **Archive** or **Delete**
  it. Archived threads move under **Archived** at the end of the list; click it to show them. When every thread is
  archived the list says so, with **Show archived**. Archive and delete
  show a notice with **Undo**. A deleted thread is gone for good about eight seconds later (or at the next start if
  you quit first). A thread that is working can't be archived or deleted (stop it first), and one with its own
  worktree can't be deleted until you discard the worktree.
- **Manage a project** from the project filter: **Manage projects**, then the project, then **Rename**,
  **Open in editor**, **Project settings…** (the project's own new-thread defaults), **Account bindings…** (opens Settings), **Import sessions…** or **Remove from SkyeLab**. **Switch to project** in the
  palette narrows the list to that project. Removing never touches the folder or its
  files, and keeps the threads' history: add the same folder again and the project comes back with its threads. If
  agents are running in it, SkyeLab asks first, and removing stops them.
- **Import sessions.** Bring Claude Code or Codex sessions you ran outside SkyeLab into a project: **Import
  sessions…** in the project's menu, or **Import Sessions** in the palette for the selected project. SkyeLab reads
  only the account the folder is bound to (its `CLAUDE_CONFIG_DIR/projects`, or `CODEX_HOME/sessions`), lists the
  sessions run in the folder or a folder under it with their title, age and message count, and imports the ones you
  tick as threads. Sessions in a subfolder bound to another account are left out. A Claude session's timeline shows
  your prompts, the agent's replies, thinking and tool calls; a Codex session shows prompts and replies only (the
  agent still has its full history). Each message keeps the time it had in the session. Sending in an imported thread
  continues the same session; if the agent no longer has it, the thread says so and runs your message in a new
  session (Claude and Codex alike). Sessions already in a thread, in any project (nested projects can both list
  one), are marked **Imported** and can't be picked again, and a thread won't resume a session another thread is
  running right now. The account's files are only read, never changed; each
  session is read up to 32 MB, and lines that don't parse are skipped (the notice says how many).
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

The bottom panel (<kbd>⌘J</kbd>) holds terminals. <kbd>⌥⌘T</kbd> opens a new one. Its tab shows what runs in front
of the shell (`npm`, `vim`), else the folder's name, then the account it runs as. Double-click the title to rename the
tab (<kbd>Enter</kbd> keeps it, <kbd>Esc</kbd> cancels, an empty name goes back to the automatic one). In a terminal,
<kbd>⌘D</kbd> (<kbd>Ctrl+Shift+D</kbd>) splits the focused pane side by side and <kbd>⇧⌘D</kbd>
(<kbd>Ctrl+Alt+Shift+D</kbd>) stacks a new one under it, each a new shell on that folder's account; splits nest.
Drag the line between two panes to resize them. <kbd>⌥⌘→</kbd>/<kbd>⌥⌘←</kbd> (<kbd>Ctrl+Alt+→/←</kbd>) move between
the panes. <kbd>⌘W</kbd> (<kbd>Ctrl+Shift+W</kbd>) closes the focused pane, and the tab with its last one; the × on
the tab closes the whole tab. When a shell you close is still running a command, SkyeLab asks first; turn that off
with **Terminal close confirmation** in Settings → General. A terminal belongs to the thread that was showing when you
opened it: another thread hides it, and it comes back, still running, with its thread. Terminals opened with no
thread showing stay with every thread.

When SkyeLab starts again, each window reopens its terminal tabs with their splits, sizes and names, each pane in the
folder its shell was last in. They are new shells: running programs and scrollback don't survive a quit, and no
scrollback is written to disk. A thread's tabs open the first time that thread shows. Terminal text uses the code text
size from Settings → Appearance, and its colors come from the theme. <kbd>⌥⌘B</kbd> opens a browser tab for your dev
server in the right panel.

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
Relative paths are read from the folder the terminal started in. To ask the agent about some output, select it and
press <kbd>⌘L</kbd> (<kbd>Ctrl+Shift+L</kbd>) or right-click and pick **Add to prompt**: it shows above the prompt
of the thread on screen as a chip (click it to read the text, × to drop it) and is sent after your message as a
code block, up to 20,000 characters.

## Settle and snooze

Hover a thread row for two buttons where its time was: the check **settles** it (it's done:
it moves to the **Settled** shelf at the bottom, folded, newest settle first, 25 at a time with **Show N more**), and
the alarm clock **snoozes** it: **In 1 hour**, **Tonight** (18:00, while that's ahead), **Tomorrow** (09:00), **Next
week** (Monday 09:00) or **Custom…**, where you type `45m`, `3h`, `2d`, `18:30` or `2026-12-01 09:00`. A snoozed
thread waits in the folded **Snoozed** shelf with how long until it wakes; then it is back in the list marked
**Woke** until you open it. Settling, snoozing, archiving or deleting a thread puts **Undo** in the sidebar's bottom row
for a few seconds (it stays while the pointer is on it; with the sidebar hidden it is a notice instead). Right after a
row's button acts, the rows below slide up: a second press within half a second is ignored, so a double click never
settles the next thread too. Opening a shelf near the bottom scrolls it into view. A settled row's hover button un-settles it, and a snoozed row's
wakes it now. The row's right-click menu has the same entries, and the palette has **Settle Thread** and **Snooze
Thread** for the open thread. A thread that starts working shows under Working whatever its shelf, and a settled or
snoozed thread leaves Pinned while it's shelved. Archived threads can't be settled or snoozed.
