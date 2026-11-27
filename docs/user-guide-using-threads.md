# User guide: threads

[Back to the user guide](user-guide.md)

1. Click **Add** under Projects and pick a folder.
2. Select the project and click **New** under Threads.
3. Type a prompt and press <kbd>↩</kbd> (or click **Send**). <kbd>⇧↩</kbd> starts a new line, and <kbd>⌘↩</kbd>
   (<kbd>Ctrl+↩</kbd>) always sends. To make Enter add lines instead, change **Settings → Chat → Send shortcut**:
   **⌘ + Enter for multiline prompts** (Enter sends a one-line prompt; once it has a line break, only ⌘↩ sends) or
   **⌘ + Enter always**. While a `@`, `/` or `$` menu is open, Enter picks the highlighted item.

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
- **`@`** lists the folder's files: the ones git tracks and new ones it would add (ignored files don't show). Type to narrow it,
  then <kbd>↩</kbd> or <kbd>Tab</kbd> inserts `@path`; <kbd>Esc</kbd> closes it. Two or more letters that start a
  model's name (`@son`, `@gpt-cod`) also list up to eight of the thread's provider's models above the files: picking
  one inserts a reference such as `model:claude/sonnet` into the prompt, for the agent to read (to name a model for
  a thread it delegates, say). It doesn't change the thread's own model; the model chip does that.
- **`/`** at the start lists commands: SkyeLab's own (`/side`, below; `/model` opens the model picker; `/plan` and
  `/default` switch the thread to Plan or Ask; these three run in the composer and send nothing), the CLI's own (`/compact`, `/context`, `/cost`, `/init`, `/review`, …), your
  account's `commands/` folder (in its config dir) and the project's `.claude/commands/`. SkyeLab starts Claude
  with only your user settings, so a project command may not run. Codex has no slash commands.
- **`$`** lists your account's skills (`skills/<name>/SKILL.md` in its config dir). Picking one inserts `/name` for
  Claude and `$name` for Codex. Commands and skills come only from the account the folder is bound to.
- **Mentions show as chips** in the prompt once a space follows them, picked or typed: `@path` as the file's name
  in the accent color, `model:…` as the model's name with its provider's logo, `$name` and a leading `/name` with
  their own icon. A chip acts as one character: the arrow keys step over it, <kbd>⌫</kbd> or <kbd>⌦</kbd> removes it
  whole, and copying it copies its text. What is sent, saved as a draft, stashed or queued is the same plain text.
- **`#`** (titled **Threads and their pull requests**) lists this project's other threads and the pull requests
  linked to them. Picking a thread adds a chip with
  its title and last reply (up to 2,000 characters); picking a pull request adds a chip with its link and the title
  of the thread it belongs to. Each chip is sent after your message as a code block, like a terminal excerpt; click
  it to read the text, × to drop it. Hover a thread or pull request chip for the same card its sidebar row shows. Only threads that ran on this composer's account, in a folder still bound to
  it, are listed, so another account's work never ends up in this account's prompt.
- **Quote** a reply: select text in one of the agent's replies (drag over it, or double-click a word) and a small
  **Quote** button shows where you let go. It adds the selection as a quote chip; the message sends it as a Markdown
  quote (`> …`) before what you typed, so you can ask about exactly that part. Scrolling hides the button.
- **Stash** a prompt for later: <kbd>⌘S</kbd> (<kbd>Ctrl+S</kbd> off macOS) sets aside what the composer holds (text,
  files and chips) and empties it. A **Stash** tab with the count sits on the composer's top-right edge; click it (or
  press <kbd>⌘S</kbd> in an empty composer) to list the stashed prompts, newest first. Click one, or pick it with
  <kbd>↑</kbd>/<kbd>↓</kbd> and <kbd>Enter</kbd>, to take it out of the stash and add it after what you have typed;
  × deletes one. There is one stash for all projects and threads; it keeps the last 20 and survives restarts.
  A restore brings everything back (text, files, quote, terminal and diff-comment chips), whatever account it was
  stashed under, so a quote of one account's agent can reach another account's agent this way. Only a thread,
  pull request, issue or side-chat chip is checked again: it comes back only on the account it was picked under,
  while its folder is still bound to it, else a note says it was left out. A
  restore that would pass 8 chips or 10 files is refused and the entry stays in the stash.
- **Dictate** (macOS): click the microphone left of Send, or press <kbd>⌘⇧M</kbd>. Words appear in the prompt at the
  cursor as you speak (replacing what you had selected), and the prompt is read-only meanwhile. <kbd>⌘⇧M</kbd> again
  or the check mark finishes; <kbd>Esc</kbd> or × puts your draft back as it was; Send finishes and sends what is in the
  prompt. Switching to another tab, or anything else that takes the focus out of the thread, ends dictation and keeps
  the words heard so far (switching to another app doesn't). macOS's Speech framework does the recognition on your Mac whenever it supports your language there, so
  the audio never leaves it. When it doesn't, SkyeLab asks first, with the microphone still off: **Dictation in
  en-US sends audio to Apple**. **Allow for en-US** remembers that language and starts; **Cancel** starts nothing and
  saves nothing. It asks again for each other such language; Restore defaults in Settings → Chat forgets the answers.
  While such a language is dictated, the pill says **Sent to Apple**. The first time, macOS asks for the microphone and for speech recognition;
  if you said no, the composer tells you where to allow it (System Settings → Privacy & Security → Microphone, or
  Speech Recognition). It uses your Mac's language. Linux and Windows have no dictation, so the button isn't there.
- **The model chip** (right of the composer: **Opus 5.5 High**) always names the model that will run. Until you pick
  one it shows the account's own default, with the tooltip **Account default model**: the model the thread's last run
  on the default reported, else the `model` in the account's Claude `settings.json` or Codex `config.toml` (SkyeLab reads only that
  field and never writes there), else Codex's listed default, else **Claude default**. It sets the model and effort for this
  thread: Opus, Sonnet, Haiku or Fable with effort Low to Max for Claude, or the models Codex lists for your account.
  The chip shows the provider's logo, and opens a picker: on the left, the folder's account (it's there to tell you
  which account runs; the picker never switches account or provider), in the middle a search box and the account's
  models with a check on the current one, and on the right the effort levels for that model. Type to narrow the list,
  <kbd>↑</kbd> <kbd>↓</kbd> and <kbd>↩</kbd> to pick, <kbd>⌘1</kbd>–<kbd>⌘9</kbd> (Ctrl elsewhere) to pick the
  Nth model, <kbd>Esc</kbd> to close. A pick is kept on the thread and becomes the project's default for new threads. A running Claude agent switches
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
the account it runs on. While a turn runs, **Working for 1m 12s** above the composer counts how long (while a new agent starts,
that line names its step, such as **Starting Claude…** or **Starting MCP servers (3)…**), and while the
composer is empty, Send turns into **Interrupt**, which stops the current turn (it shows **Stopping…** until the turn
ends; after 10 seconds without an answer it works again and **Force stop** appears). <kbd>⌘.</kbd> interrupts too.
**Stop Agent** (command palette, or <kbd>⌥⌘.</kbd>) ends the agent; there is no Stop button. **Restart Agent Session**
(command palette) ends it too, so your next message resumes the same session in a new agent process that loads the
account's MCP servers, skills and plugins as they are now; while a turn runs it asks first. Like Stop, it ends the
agent for everything else too: a parent agent can't message the thread any more. Threads are saved, and opening an old one and sending a follow-up resumes the
same session (Claude's `--resume`, or Codex's `thread/resume`). If the thread store can't save an event (a full disk,
another SkyeLab on the same data), the thread still shows it and says how many events weren't saved. SkyeLab retries every few seconds and with each new
event, and saves them in order once the store works again. Quitting before then loses them.
Switching to another page doesn't stop a thread's agent; **Stop** or quitting SkyeLab does. A thread whose turn was cut
off that way shows as **Interrupted** in the sidebar and its turn ends **Stopped after …**, also after a restart (a turn
still running when SkyeLab crashed shows as **Interrupted** too); send a follow-up to resume it, which clears the mark.

### Thread details

The thread details card (<kbd>⇧⌘B</kbd>, floating over the chat) holds the thread's folder, branch, git actions,
scripts, delegated agents, pull request and editor, and every account's usage. Details and the dialog backdrop:
[user-guide-thread-details.md](user-guide-thread-details.md).

### Permissions, questions and plans

The composer's permission chip (**Full access ⌄** on a new thread, the default) picks what the agent may do without
asking (switching to **Full access** from another mode confirms first; Settings → Chat → Default permissions sets
what new threads start in). Approvals and the agent's questions dock on top of the composer; plans show as cards in the thread. Each mode, and how answering works:
[user-guide-permissions.md](user-guide-permissions.md).

### What the timeline shows

Streaming text and thinking, tool calls and edits, subagent cards, visual replies (`html_render` charts, diagrams and
mockups), the files each turn changed, a banner when a turn fails, the left-edge timeline minimap (for jumping across
turns in threads with two or more messages), and find in thread (⌘F / Ctrl+F):
[user-guide-timeline.md](user-guide-timeline.md). To give inline visual replies more horizontal room, change **Card
width** in **Settings → Chat → Visual replies** (**Standard** 728 px, **Wide** 960 px, or **Full** up to 1280 px or the
available viewport width, capped at 240–1600 px); **Restore defaults** in Settings → Chat resets it to **Standard**.

### Finding and managing threads

Search, titles, drafts and prompt history, the thread and project menus, imported sessions, unread, keys and Open in editor: [user-guide-managing-threads.md](user-guide-managing-threads.md).

### Browser tabs

Browser tabs (yours and the ones agents open) are in [User guide: browser tabs](user-guide-browser.md).

### Delegated threads

An agent can hand work to another thread with SkyeLab's MCP tools (`start_thread`, `send_message`,
`wait_thread`, and others). The new thread appears in the sidebar marked unread; the page you're on stays. Its header's breadcrumb links back to the
parent, and the parent's thread details card lists it under **Lineage**. An agent can only start threads in projects bound to its own
account; a start into a project on another account is refused and no thread is created. A delegated thread can
delegate once more (two levels), and a thread can have four children running at a time. That limit also holds
when a stopped child is sent a new message, by its parent, from another device or by you: it stays stopped and its
header says why. Once you stop a thread, it can't start or message other threads, even with a call
it had already sent. When `wait_thread` sees a child's turn end, it also returns that turn as data: whether it failed, its
result text, how long it took, what it cost and which files it edited, so the parent doesn't parse the child's reply.
Codex threads get the same tools, with the same account rule. On Windows they work the same way (not yet tested on a real Windows machine).
