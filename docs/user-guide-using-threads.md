# User guide: threads

[Back to the user guide](user-guide.md)

1. Click **Add** under Projects and pick a folder.
2. Select the project and click **New** under Threads.
3. Type a prompt and press <kbd>⌘↩</kbd> (or click **Send**).

**Scratch threads** are for quick questions outside any project. Run **New Scratch Thread** from the palette and pick
an account. SkyeLab makes that account's scratch folder (`scratch/<account>` in its data folder) on first use, binds
it to the account you picked and lists it as the project **Scratch · <account>**. Like any folder, it never falls back
to a default account: no pick, no thread. Remove the project or its binding like any other.

### The composer

- **Attach** (the paperclip), drag files onto the composer, or paste an image or copied files. Images (PNG, JPEG,
  GIF, WebP), PDFs and text files work, up to 5 MB each and 10 per message; each shows as a chip you can remove.
  Claude gets them as image and document blocks. Codex gets images and text files, not PDFs.
- **Long pastes become files.** Pasting text of 8 KB or more, or 200 lines or more, attaches it as `pasted-text.txt`
  (`pasted-text-2.txt`, … after that) instead of filling the prompt. **Paste as text** under the composer puts the
  last one back into the prompt. A prompt over 120,000 characters isn't sent: the composer says by how much and keeps
  your text.
- **Images you sent** show as thumbnails on your message while the thread stays open in this window (SkyeLab doesn't
  store image files). Click one to view it large: **+** and **−** zoom, **Fit** fits it to the window again, and
  <kbd>Esc</kbd> or a click outside closes it.
- **`@`** lists the files git tracks in the folder (ignored and untracked files don't show). Type to narrow it,
  then <kbd>↩</kbd> or <kbd>Tab</kbd> inserts `@path`; <kbd>Esc</kbd> closes it.
- **`/`** at the start lists commands: the CLI's own (`/compact`, `/context`, `/cost`, `/init`, `/review`, …), your
  account's `commands/` folder (in its config dir) and the project's `.claude/commands/`. SkyeLab starts Claude
  with only your user settings, so a project command may not run. Codex has no slash commands.
- **`$`** lists your account's skills (`skills/<name>/SKILL.md` in its config dir). Picking one inserts `/name` for
  Claude and `$name` for Codex. Commands and skills come only from the account the folder is bound to.
- **`#`** lists this project's other threads and the pull requests linked to them. Picking a thread adds a chip with
  its title and last reply (up to 2,000 characters); picking a pull request adds a chip with its link and the title
  of the thread it belongs to. Each chip is sent after your message as a code block, like a terminal excerpt; click
  it to read the text, × to drop it. Only threads that ran on this composer's account, in a folder still bound to
  it, are listed, so another account's work never ends up in this account's prompt.
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

The thread details card (<kbd>⇧⌘B</kbd>, floating over the chat) holds the thread's folder, branch, git actions,
scripts, delegated agents, pull request and editor, and every account's usage. Details and the dialog backdrop:
[user-guide-thread-details.md](user-guide-thread-details.md).

### Permissions, questions and plans

The composer's permission chip (**Ask ⌄**) picks what the agent may do without asking (**Full access** confirms
first). Approvals, the agent's questions and plans show as cards in the thread. Each mode, and how answering works:
[user-guide-permissions.md](user-guide-permissions.md).

### What the timeline shows

Streaming text and thinking, tool calls and edits, subagent cards, the files each turn changed, a banner when a turn
fails, and find in thread (⌘F / Ctrl+F): [user-guide-timeline.md](user-guide-timeline.md).

### Finding and managing threads

- **Search.** The field at the top of the sidebar filters the listed threads by title and by what was
  said in them (your prompts and the agent's answers, not tool output).
- Generated titles, drafts, prompt history (<kbd>↑</kbd>), Settings → Chat and **Fork from here**:
  [user-guide-threads.md](user-guide-threads.md).
- **Right-click a thread** to **Rename**, **Pin** (pinned threads list first, with a star), **Archive** or **Delete**
  it. Archived threads move under **Archived** at the end of the list; click it to show them. Archive and delete
  show a notice with **Undo**. A deleted thread is gone for good about eight seconds later (or at the next start if
  you quit first). A thread that is working can't be archived or deleted (stop it first), and one with its own
  worktree can't be deleted until you discard the worktree.
- **Manage a project** from the project filter: **Manage projects**, then the project, then **Rename**,
  **Open in editor**, **Account bindings…** (opens Settings), **Import sessions…** or **Remove from SkyeLab**. **Switch to project** in the
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

The bottom panel (<kbd>⌘J</kbd>) holds terminals. <kbd>⌥⌘T</kbd> opens a new one; its tab shows the account it runs
as. In a terminal, <kbd>⌘D</kbd> (<kbd>Ctrl+Shift+D</kbd>) splits it: a second shell beside it in the same tab,
on the folder's account, and <kbd>⌥⌘→</kbd>/<kbd>⌥⌘←</kbd> (<kbd>Ctrl+Alt+→/←</kbd>) move between the panes. A
terminal belongs to the thread that was showing when you opened it: another thread hides it, and it comes back,
still running, with its thread. Terminals opened with no thread showing stay with every thread. Terminals and their
scrollback end when SkyeLab quits. <kbd>⌥⌘B</kbd> opens a browser tab for your dev server in the right panel.

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

### Agent browser tabs

An agent can open a browser tab of its own with SkyeLab's `browser_open` tool, then `browser_navigate`, read
`browser_url`, run `browser_eval` (a JavaScript expression; the JSON result comes back, up to 64 KiB, within 10
seconds) and take a `browser_screenshot` (a PNG of the visible page, at most 1600 px wide and 1 MiB; macOS and Linux, where it is untested so far; not yet on Windows). SkyeLab doesn't keep image bytes in its thread history: the tool row shows **Image (205 KB)**
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
parent, and the parent's thread details card lists it under **Lineage**. An agent can only start threads in projects bound to its own
account; a start into a project on another account is refused and no thread is created. A delegated thread can
delegate once more (two levels), and a thread can have four children running at a time. That limit also holds
when a stopped child is sent a new message, by its parent, from another device or by you: it stays stopped and its
header says why. Once you stop a thread, it can't start or message other threads, even with a call
it had already sent. Codex threads get the same tools, with the same account rule. On Windows they work the same way (not yet tested on a real Windows machine).
