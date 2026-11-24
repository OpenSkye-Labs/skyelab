# The thread timeline

What a thread shows while its agent works, and the tools for reading it back. Back to the [user guide](user-guide.md).

## Rows

- Text and thinking stream in word by word as the agent writes. Thinking is a **Thought** row; click it to read it.
  Complete blocks are saved. Text a turn cut off (Stop, Interrupt, a crash) is saved as far as it got and marked
  **(interrupted)**, so a reopened thread still shows it.
- The agent's todo list (Claude's `TodoWrite`, Codex's plan) is one checklist per turn that updates in place.
- Edits (`Edit`, `Write`, `MultiEdit`) show their diff inline. Click the file name to open it in the Changes tab.
- File paths in the agent's text open in the **Preview** tab at their line: markdown links (`[a.rs](src/a.rs:12)`),
  `path:line` text (`src/a.rs:12`) and code spans naming a file (`` `src/a.rs` ``). Only paths inside the thread's
  folder become links; a path outside it stays plain text. Web links open in your browser, and nothing else opens.
- Shell commands show as `$ command` with how they ended (`exit 1`, `failed`); click one for its output.
- Messages and code blocks have copy buttons; under a turn's final reply the copy button sits next to the time the
  turn ended.
- A ```` ```mermaid ```` block in a reply draws as a diagram once the block is complete (while it streams you see its
  source). The code icon in its corner shows the source, the diagram icon brings the drawing back, and copy copies the
  source. A diagram SkyeLab can't draw (bad syntax, over 16 KB, or too big to read) shows its source with the reason.
  Diagrams never load images or links.
- A finished turn folds its work (tool calls, thinking, the text in between) under one line above the reply:
  **Worked for 4.2s · 12.3k in · 175 out · $0.0421 ›** (that turn's own tokens and cost). Click it to open or fold the work. A running turn shows
  everything; one that failed, or was stopped before it finished, isn't folded and keeps its line after the turn.
- When the account hits its usage limit, the timeline shows one banner, **Usage limit reached. Limited until it resets
  in …**, and the
  sidebar shows **Limited**, until a turn succeeds again.
- If a Claude thread's earlier session can't be found when you continue it (its history was deleted, or it ran under
  another config dir), the thread says so and runs your message in a new session. A Codex thread shows the failure.
- A subagent (Claude's **Agent** or **Task** tool) is one card: **Subagent · general-purpose**, what it was asked,
  and **Running · 4s**, **Done · 1m 02s** or **Failed**. While it runs, the line under it says what it is doing now.
  Click the card for the tool calls the subagent made (a subagent's own subagents roll up into the same card) and
  its final report. Its calls never show as the thread's own rows.

## Files each turn changed

Under a finished turn's reply, a card says **3 changed files +12 −4**. Click it for the file list; click a file, or
**Open turn N diff**, to open the Changes tab on that turn, scrolled to the file. The list comes from the turn's
checkpoint (the same diff as the Changes tab's **Turn N** chip), so it needs a folder in a git repo. If a turn ended
without a result (a crash, a session that had to start over), the turn numbers no longer line up and the cards stay
hidden rather than list another turn's files; the Changes tab's turn chips still work.

## When a turn fails

If the last turn failed (an error result, or the agent exited and wrote why to stderr), a red banner over the composer
says what went wrong:

- **Retry** sends the same prompt again (attachments aren't resent).
- **View output** opens the agent's stderr row in the timeline and scrolls to it.
- **×** closes the banner until another turn fails.

When the agent says its account isn't signed in ("Not logged in", a 401), the banner says **Account Work isn't signed
in**. **Log in** opens a terminal tab that signs that same account in (`claude auth login` or `codex login`, with the
account's own config folder); when it finishes, press **Retry**. **Open Settings** goes to Settings → Accounts.
SkyeLab never retries under another account: the folder's binding decides.
Stopping a turn yourself (Stop, Interrupt) shows no banner.

## Minimap

A thread with two or more of your messages gets a thin rail at the left edge of the timeline: one short line per
message, top to bottom. The line of the turn you are reading is brighter. Hover the rail to see that message and the
turn's last reply; click a line to scroll its message to the top.

## Find in thread

**⌘F** (Ctrl+F elsewhere) in a thread opens a find bar at the timeline's top right. It counts every match of what you
type, ignoring case, in prompts, replies, thinking, tool calls and their output, and subagent cards (**2/7**). **Enter**
goes to the next match and **Shift+Enter** to the previous one, wrapping around; the timeline scrolls to it, and a
folded turn or a closed tool row opens when the match is inside it. Rows with a match are tinted, the current one
more strongly. **Esc** or **×** closes the bar and returns to the prompt.
