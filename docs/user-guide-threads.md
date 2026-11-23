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
- Turn it off in **Settings → Threads → Generate thread titles**.
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

## Settings → Threads

Defaults for new threads. A thread's own choice always wins.

- **Generate thread titles**: on by default.
- **Generate commit messages and PRs**: on by default. Shows the Changes tab's sparkle buttons
  ([how they work](user-guide-accounts.md)).
- **Default permissions**: Ask, Plan or Accept edits. Full access is never a default; pick it in a thread.
- **New threads start in a worktree**: turns on **New worktree** under a new thread's composer.
- **Claude model** and effort, **Codex model** (an id, empty for the account's default) and effort: used by a new
  thread in a project that hasn't picked a model yet (the last model picked in a project stays its default).

They are saved in `settings.toml` under `[threads]`. Threads an agent delegates start in Ask.

## Fork a thread

Under a finished reply, the branch button (**Fork from here**) makes a new thread in the same project, under the same
account, with the conversation up to that reply. Its header says **Forked from** and links to the original. The
original is not changed.

An agent session can't be copied, so the fork's first message starts a new session whose prompt begins with the
copied conversation (your messages and the agent's answers, no tool calls; the newest 60,000 characters). Your message
is shown and stored as you typed it. The fork starts in the project's folder, not in the original's worktree, and
copies no checkpoints. If the folder is no longer bound to the thread's account, nothing is forked.
