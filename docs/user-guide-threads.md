# Thread titles, drafts, history, defaults and forks

## Titles

A new thread is named after the first line of your prompt. Once its first turn ends, the thread's own agent writes a
short title (six words at most) from your first prompt and its first reply, and the sidebar shows it.

- The call runs under **the account the thread is bound to**, like every agent process: the folder is resolved again,
  the env is cleared and only that account's `CLAUDE_CONFIG_DIR` (or `CODEX_HOME`) is set. Claude runs
  `claude -p` on Haiku with no tools, no MCP servers and no saved session; Codex runs an ephemeral read-only
  `codex exec` at low effort. Nothing from other threads is sent.
- A title you set yourself (**Rename**) is never replaced by a generated one.
- **Right-click a thread → Regenerate title** names it again, replacing any title. If the folder is no longer bound to
  the thread's account, nothing runs and a notice says why.
- Turn it off in **Settings → Chat → Generate thread titles**.
- Threads on another machine are named there, not here.

## Drafts

What you type in a composer and the files you attach stay with that thread until you send them: switch to another
thread, close the tab or quit, and they are back when you return. A **New thread** tab keeps its draft per project.
Sending (or emptying the composer) removes the draft. Threads on another machine keep no draft here.

## Prompt history

In an empty composer, <kbd>↑</kbd> shows this thread's last prompt; <kbd>↑</kbd> again goes further back. <kbd>↓</kbd>
walks forward and ends at an empty composer. In a prompt with several lines, the arrows move the caret first, so you
can still edit it. Text you typed yourself is never replaced; the keys move the caret as usual. Files attached to an
earlier prompt aren't brought back.

## Defaults for new threads

A thread's own choice always wins. They live in three Settings sections:

- **Chat → Send shortcut**: when Enter sends a prompt. **Enter** (the default), **⌘ + Enter for multiline prompts** or
  **⌘ + Enter always**. <kbd>⇧↩</kbd> always adds a line and <kbd>⌘↩</kbd> always sends.
- **Chat → Generate thread titles**: on by default.
- **Chat → Visual replies**: the https hosts an agent's page may load scripts, styles and fonts from
  ([visual replies](user-guide-timeline.md)).
- **Diffs & pull requests → Generate commit messages and PRs**: on by default. Shows the Changes tab's sparkle buttons
  ([how they work](user-guide-accounts.md)).
- **Chat → Default permissions**: Full access (the default), Ask, Plan or Accept edits. Pick Ask to have new threads
  ask before anything their rules don't allow.
- **Worktrees → New threads start in a worktree**: turns on **New worktree** under a new thread's composer.
- **Diffs & pull requests → Fetch remote**: Off, or every 1, 5 (the default), 15 or 30 minutes. While a thread is shown, its folder fetches
  its branch's upstream in the background under the thread's account's own GitHub login (the same rules as Push:
  https remotes only), so Details can say how far behind it is. Saved as `fetch_minutes`.
- **Chat → Claude model** and effort, **Codex model** (an id, empty for the account's default) and effort: used by a new
  thread in a project that hasn't picked a model yet (the last model picked in a project stays its default).

They are saved in `settings.toml` under `[threads]`. Threads an agent delegates start in Ask.

### Per-project defaults

Work and personal projects often want different defaults. At the top of **Chat** and **Worktrees**, **Applying
settings for** picks **All projects** (the app-wide defaults above) or one project. With a project picked, the same
rows set that project's own default for permissions, Claude model and effort, Codex model and effort, and "start in a
worktree". Each row offers **Inherit: …**, which follows All projects (the Codex model field inherits when empty).
**Restore defaults** sends the project's rows on that page back to Inherit. **Project settings…** in a project's
sidebar menu opens Chat with that project already picked.

A new thread's model comes from, in order: the project's own default, the last model picked in that project, then
All projects. The project's account is not a setting here: it always comes
from the folder's binding (**Accounts**). Per-project values are saved in `settings.toml` under
`[threads.projects."<project folder>"]`, so moving the folder loses them, as it loses the binding.

## Fork a thread

Under a finished reply, the branch button (**Fork from here**) makes a new thread in the same project, under the same
account, with the conversation up to that reply. Its header says **Forked from** and links to the original. The
original is not changed.

An agent session can't be copied, so the fork's first message starts a new session whose prompt begins with the copied
conversation (your messages and the agent's answers, no tool calls; about the newest 60 KB, built by the same transcript
builder as the handoff below), fenced as quoted data so
no text in it can pose as a new message. Your message is shown and stored as you typed it. The copied turns don't
count again in the Usage page, and the fork's first turn counts its whole cost, since its session is new. The fork
starts where the original runs: in its worktree, sharing that worktree and its branch (discarding the worktree from
either thread removes it for both, and is refused while the other thread's agent runs there), or in the project's
folder once that worktree is gone. It copies no checkpoints.
If that folder is no longer bound to the thread's account, nothing is forked. A second click while a fork of the
same thread is being made (from any window) does nothing, so a double click makes one fork.

## Continue in the other provider

A thread can move from Claude to Codex, or from Codex to Claude: **Continue in Codex** (or **Continue in Claude**) is in
the thread details card, in the command palette, and on the "This thread can't continue here" notice. It makes a new
thread in the same project whose header says **Continued from** and links to the original; the details card's
**Lineage** links to it too. The original is not changed.

A folder is bound to one account, so the new thread runs under the account the folder is bound to, and only when that
account is of the other provider. If the folder is still bound to the original's account (or to nothing), SkyeLab asks
you to bind it to a Codex (or Claude) account in Settings → Accounts first; it never picks one for you. Once you do,
the original's threads in that folder can't send any more, and their notice offers the handoff.

A session can't move between providers, so the new thread's first message starts with a portable transcript of the
original: your messages and the agent's answers kept whole, each command as the command, whether it failed and its
output, each file edit as the paths it changed, and plans. Reasoning, other tool calls and attached files are left
out. Up to 16 KB is carried, picking your latest message, the agent's latest answer and your first message first, then
the newest others; each item goes whole or not at all, and the transcript marks where items were left out. It is
fenced as quoted context, never as instructions. Your message is shown and stored as you typed it. Like a fork, it
starts where the original runs (its worktree, shared, or the project's folder once that worktree is gone), and a
double click makes one thread. While the window shows another machine, **Continue in …** isn't offered for this
machine's threads (the details row, the palette entry and the notice's button are hidden): the new thread couldn't
open there. Switch back to this machine to use it.

## Send to several models

In a new thread, the model chip's menu has **Send to several models…**. Tick 2 to 4 models of the account the folder
is bound to (Claude's Opus, Sonnet and Haiku, or the Codex account's models; never another account), and the chip
reads "3 models". **In worktrees** (on by default in a git repo) gives each model its own worktree, so their edits
don't collide; **One model** goes back. Send starts one thread per model, the new thread running the first, and opens
the **Compare** page: a column per model with its status, cost, time, changed files and latest reply. Once every model
has finished, the header shows the total cost (from each turn's stored cost), and **Keep this one** asks whether to
archive the other threads or leave them; either way the kept thread opens. Each model runs at its default effort,
from the same base branch. At most 4 compared threads can be working at once; a send that would pass that is refused
with your prompt kept.

In the sidebar the threads share one row with a **×3** badge: click the row for the Compare page, the badge to list
the threads under it.

## Side chats

Type `/side <question>` (or `/btw <question>`; both are listed in the composer's `/` menu) in the composer, or run
**New Side Chat** from the command palette or the thread details card's **Side chats** section, to ask something on the side
without interrupting the thread's agent. A tab named after your question opens in the right panel with a small chat.
Opened without a question, the side chat's composer takes the focus, so you can type it there.
It runs in the thread's folder under the thread's account, as its own agent session, and its first message starts with
the newest part of the thread's conversation (about the newest 12 KB, no tool calls), fenced as quoted data so no text
in it can pose as a new message. Its title is your question.

A side chat is a read-only helper. Its agent always starts in **Plan** mode (Codex: its read-only sandbox), so it
reads and answers but doesn't edit files or run commands: if its agent asks to, only **Deny** goes through. It gets
none of SkyeLab's tools (no delegating, no browser). Its permissions chip shows Plan and can't be changed, and a plan
it proposes can't be approved there: ask the main thread to make changes. A file link in its answer opens the Preview
tab, as in the main thread.

**Bring back** puts the side chat's last answer in the thread's composer as a chip, sent with your next message.
Side chats are not listed in the sidebar: the thread details card lists them under **Side chats**, and a click opens
one again. Closing its tab keeps it (a running answer keeps going); closing, archiving or deleting the thread stops
its side chats, and deleting the thread deletes them. A side chat is open in one window at a time. If the folder is
no longer bound to the thread's account, no side chat starts and your question stays in the composer. A side chat
can't open side chats of its own.
