# Permissions, questions and plans

## Permissions

In the composer, the chip next to the paperclip (**Full access ⌄** on a new thread) picks what the thread's agent may
do without asking. Its menu lists the four modes with what each allows; **Full access** shows in the warning color:

- **Ask**: it asks before anything its own rules don't already allow.
- **Plan**: it reads and plans only, with no edits or commands (for Codex, a read-only sandbox).
- **Accept edits**: file edits in the folder go ahead; anything else asks. (For Codex this is the same as Ask: its
  sandbox already lets it edit in the folder without asking.)
- **Full access** (the default for a new thread, as in T3 Code): nothing asks. A Codex agent also leaves its sandbox,
  so it can write outside the folder and use the network. Switching a thread to it from another mode asks you to
  confirm first.

**To make Ask the default**, open **Settings → Chat → Default permissions** and pick **Ask** (for all projects, or
for one in **Applying settings for**). A default you picked, and the mode each existing thread saved, never change.
Some threads keep their own rule: a side chat is always Plan; a scheduled task uses the mode saved on it (Full access
needs the task's own confirm); a thread an agent starts with `start_thread` starts in Ask (Plan when that project's
default is Plan), never Full access. A fork, a hand-off and a thread started from your phone or another machine start
in your new-thread default.

The mode is saved with the thread and used every time its agent starts or resumes. Changing it while the agent runs
applies at once for Claude and from the next turn for Codex. Claude takes Full access only when it starts: switching a
running Claude thread to Full access saves the choice and says so; stop it (**Stop Agent**) and send a message to
continue in Full access.

When the agent asks, a **Waiting for approval** card docks on top of the composer, so it never scrolls out of view
(the timeline keeps a one-line **Waiting for your answer below** row; once answered, the row says what was decided). The card shows with the tool and what it wants to do (the
command, or the file; for a plain shell command, the agent's own description of it under the command; for a file
write its line count and first 20 lines, and for an edit its diff (a line between a MultiEdit's edits); when that is
only a part, the card says **Showing 20 of 2,000 lines** and **View all** opens the whole change, every line, in a
read-only diff; for anything
else, or text with hidden or control characters, the full input it sent), and the agent waits until you answer: **Allow once**, **Allow for this thread** (Claude's own suggested rule
for this call, or this exact command when it has none; while this agent runs; after a resume it asks again;
for a Codex command, SkyeLab itself allows that exact command again in this thread), for a Codex command also
**Allow "cargo test …" for this thread** (any one plain command starting with those words, the first two to start, by
the same rules as a project prefix below: never one that chains, pipes, substitutes or redirects, never a shell or
interpreter; while this agent runs, and not in Plan mode; its row then says **Allowed for this thread** and the rule),
**Allow for this project** (below) or **Deny**, or **Interrupt** to stop the turn instead. Afterwards the row says
what was decided.
An ask the agent withdraws (an interrupt, its turn ended, or the agent stopped) shows **Withdrawn**. Sending a message instead declines the ask and redirects the agent (the composer says so). A request in a thread whose agent has
stopped can't be answered; send a message to continue. Another machine viewing the thread, and the mobile app, show
the same card and can answer it.

### Allow for this project

**Allow for this project** is offered for shell commands (Claude's Bash, Codex's shell), file edits and writes
(Edit, Write, MultiEdit, NotebookEdit) and MCP tools. It opens a dialog that shows the rule before anything is saved:

- A command: **This exact command**, or **Commands starting with** and the words to match (the first two words to
  start; edit them). A prefix rule matches one plain command with any arguments after those words, never one that
  chains (`;`, `&&`, `||`), pipes, substitutes (`$( )`, backticks), redirects or spans lines; such a command can only
  be allowed exactly. A prefix can't start with a shell or another program that runs whatever it is given (`bash`,
  `env`, `sudo`, `python`, `node`, `find`, …) or end in a flag; allow those exactly.
- A file: a path relative to the thread's folder, the file itself to start. Widen it with `*` (within one folder) or
  `**` (across folders), like `src/**`. It can't reach outside the project (no `..`, no absolute paths, no symlinks
  out), and never covers `.git` or agent and editor settings (`.claude`, `.mcp.json`, `.vscode`, …).
- An MCP tool: every call to that one tool.

**Save rule and allow** saves it and allows the waiting call (the rule must cover it). From then on, a matching ask
in this project, under the account the folder is bound to, is allowed without asking, and its row says **Allowed by
project rule** and which one. A call that asks to leave the sandbox is never allowed by a rule. Rules live in SkyeLab, per project and per account: never in the repo, never in the
account's Claude or Codex settings, and SkyeLab still ignores a repo's own `.claude/settings*.json`. They don't apply
in Plan mode or in a side chat. In Full access nothing asks, so rules don't come into it.

To see, edit or remove them, open **Settings → Chat** and pick the project in **Applying settings for**: its
**Permission rules** list the bound account's rules, each with **Edit** (the pattern, checked like a new rule) and
**Remove**.

When a turn fails, or the agent exits with an error, what it wrote to stderr (the last 64 KiB) shows as an
**Agent output** row; click it to open it.

## Questions and plans

When the agent asks you something (Claude's `AskUserQuestion`, Codex's user-input request), the question docks on top of
the composer, one at a time, as T3 shows it: its label, **1/2** when there are several (click the label row to fold
the panel), the question, then its options numbered 1 to 9, one line each with its description beside it (one choice,
or a box for each when you can pick several). The panel stays compact: past a quarter of the window its options
scroll, so the thread above it, a plan's **Approve** included, keeps its room.
When a question docks it takes the keyboard, unless you are typing in the composer: press an option's number
(1–9) or click it to pick it, and a one-choice question then moves on to the next; <kbd>↑</kbd>/<kbd>↓</kbd> move a
one-choice pick and <kbd>↩</kbd> is Next question or Submit. Typing a letter starts your own answer in the composer, <kbd>Esc</kbd> goes back
to the composer, and <kbd>⌘⇧A</kbd> (<kbd>Ctrl+Shift+A</kbd> elsewhere) returns to the question. Whatever you type in the composer is that question's own answer and replaces the picks (leave it blank to use
the selected option). **Next question** (<kbd>⌘↩</kbd>) goes to the next question and **Submit answers** (**Submit
answer** for a single question) on the last one sends them all; for half a second after a pick moved the question on,
the button ignores clicks, so a click aimed at Next doesn't submit the next question unanswered;
× skips the question. The paperclip in the panel's header (**Attach files**) adds files to that answer (up to 100 in all; × drops one): the
agent gets their paths with your answer and reads them with its own tools. A file alone answers a question. Attaching
is offered only on the machine that runs the thread. In Plan mode, when the agent is ready it shows a
**Proposed plan** card: **Approve** lets it start (the thread switches to Ask, so edits still ask first) and **Keep
planning** tells it to keep refining. From another machine's view the plan's buttons work but don't switch the
thread's mode, and the question card can only be read.
