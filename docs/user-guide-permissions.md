# Permissions, questions and plans

## Permissions

In the composer, the chip next to the paperclip (**Ask ⌄**) picks what the thread's agent may do without asking. Its
menu lists the four modes with what each allows; **Full access** shows in the warning color:

- **Ask** (the default): it asks before anything its own rules don't already allow.
- **Plan**: it reads and plans only, with no edits or commands (for Codex, a read-only sandbox).
- **Accept edits**: file edits in the folder go ahead; anything else asks. (For Codex this is the same as Ask: its
  sandbox already lets it edit in the folder without asking.)
- **Full access**: nothing asks. SkyeLab asks you to confirm first. A Codex agent also leaves its sandbox, so it can
  write outside the folder and use the network.

The mode is saved with the thread and used every time its agent starts or resumes. Changing it while the agent runs
applies at once for Claude and from the next turn for Codex. Claude takes Full access only when it starts: switching a
running Claude thread to Full access saves the choice and says so; stop it (**Stop Agent**) and send a message to
continue in Full access.

When the agent asks, a **Waiting for approval** card docks on top of the composer, so it never scrolls out of view
(the timeline keeps a one-line **Waiting for your answer below** row; once answered, the row says what was decided). The card shows with the tool and what it wants to do (the
command, or the file; for a plain shell command, the agent's own description of it under the command, and for anything
else the full input it sent), and the agent waits until you answer: **Allow once**, **Allow for this thread** (Claude's own suggested rule
for this call, or this exact command when it has none; while this agent runs; after a resume it asks again) or **Deny**,
or **Interrupt** to stop the turn instead. Afterwards the row says what was decided.
An ask the agent withdraws (an interrupt, its turn ended, or the agent stopped) shows **Withdrawn**. Sending a message instead declines the ask and redirects the agent (the composer says so). A request in a thread whose agent has
stopped can't be answered; send a message to continue. Another machine viewing the thread, and the mobile app, show
the same card and can answer it.

When a turn fails, or the agent exits with an error, what it wrote to stderr (the last 64 KiB) shows as an
**Agent output** row; click it to open it.

## Questions and plans

When the agent asks you something (Claude's `AskUserQuestion`, Codex's user-input request), the question docks on top of
the composer, one at a time, as T3 shows it: its label, **1/2** when there are several (click the label row to fold
the panel), the question, then its options numbered 1 to 9 (one choice, or a box for each when you can pick several).
When a question docks it takes the keyboard, unless you are typing in the composer: press an option's number
(1–9) or click it to pick it, and a one-choice question then moves on to the next; <kbd>↑</kbd>/<kbd>↓</kbd> move a
one-choice pick and <kbd>↩</kbd> is Next or Submit. Typing a letter starts your own answer in the composer, <kbd>Esc</kbd> goes back
to the composer, and <kbd>⌘⇧A</kbd> (<kbd>Ctrl+Shift+A</kbd> elsewhere) returns to the question. Whatever you type in the composer is that question's own answer and replaces the picks (leave it blank to use
the selected option). **Next** (<kbd>⌘↩</kbd>) goes to the next question and **Submit** on the last one sends them all;
× skips the question. **Attach files** under a question adds files to that answer (up to 100 in all; × drops one): the
agent gets their paths with your answer and reads them with its own tools. A file alone answers a question. Attaching
is offered only on the machine that runs the thread. In Plan mode, when the agent is ready it shows a
**Proposed plan** card: **Approve** lets it start (the thread switches to Ask, so edits still ask first) and **Keep
planning** tells it to keep refining. From another machine's view the plan's buttons work but don't switch the
thread's mode, and the question card can only be read.
