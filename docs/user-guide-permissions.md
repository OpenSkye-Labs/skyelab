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

When the agent asks, the thread shows a **Waiting for approval** card with the tool and what it wants to do (the
command, or the file), and the agent waits until you answer: **Allow once**, **Allow for this thread** (Claude's own suggested rule
for this call, or this exact command when it has none; while this agent runs; after a resume it asks again) or **Deny**,
or **Interrupt** to stop the turn instead. Afterwards the row says what was decided.
An ask the agent withdraws (an interrupt, its turn ended, or the agent stopped) shows **Withdrawn**. A request in a thread whose agent has
stopped can't be answered; send a message to continue. Another machine viewing the thread, and the mobile app, show
the same card and can answer it.

When a turn fails, or the agent exits with an error, what it wrote to stderr (the last 64 KiB) shows as an
**Agent output** row; click it to open it.

## Questions and plans

When the agent asks you something (Claude's `AskUserQuestion`, Codex's user-input request), the thread shows a
question card: each question with its options (one choice, or **Pick any** for several) and **Other** for a typed
answer. **Submit answers** sends them all; **Skip** declines. In Plan mode, when the agent is ready it shows a
**Proposed plan** card: **Approve** lets it start (the thread switches to Ask, so edits still ask first) and **Keep
planning** tells it to keep refining. From another machine's view the plan's buttons work but don't switch the
thread's mode, and the question card can only be read.
