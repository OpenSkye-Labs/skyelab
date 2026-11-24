# Thread details

The thread details card floats over the chat at the top right, for the thread on screen. The first button at the
page's top right opens and closes it (<kbd>⇧⌘B</kbd>, <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>B</kbd> elsewhere), and so
does <kbd>Esc</kbd>. It stays open while you work beside the conversation; when the right panel leaves it no room
there, it covers the chat and a click outside closes it. Everything in it acts on the thread's own folder under its
bound account.

- **Environment:** this machine's name, with a button that opens the thread's folder in Finder (or your file
  manager). On a git repo, then:
  - **The branch** opens the branch picker (a worktree thread shows its own branch, which doesn't switch). When the
    branch tracks an upstream, the row ends with how far apart they are (**2 behind / 1 ahead**), counted after the
    background fetch (Settings → Diffs & pull requests → **Fetch remote**). If that fetch failed, a line under the row says why.
  - **Pull N commits** shows while the upstream is ahead. It runs **Pull** (below).
  - **Commit & push** opens a menu that says which GitHub login pushes (the thread's account's own `gh` login, read
    against `accounts.toml` as saved now; if the folder has been bound to another account since, it shows the refusal
    Push would give instead), then
    **Commit**, **Commit & push**, **Push**, **Pull** and **Open PR** (**View PR** once the thread has one). Pull fetches,
    then fast-forwards the branch to its upstream; it refuses when the folder has uncommitted changes to tracked files,
    when the branch has no upstream yet, and when the branch has commits the upstream doesn't (merge or rebase by hand). Each runs in the
    Changes tab with the same checks as its buttons there: the folder must still be bound to the thread's account,
    and Push and Open PR need that account's GitHub login. Commit and Open PR use the message and title typed in
    Changes; with none typed, the field is focused and nothing runs.
  - **Changes vs <base>** shows the lines added and removed since the worktree's base commit, or since `HEAD`
    (untracked files aren't counted), and opens Changes.
  - A worktree thread also has **Open terminal here** and **Discard worktree** (see **Worktrees**).
- **Actions:** **Add project script** (or **Run project script** once the project has some) opens the project's
  scripts.
- **Lineage · N running:** the threads this thread's agent delegated that are working now, with how long they have
  run. **Previous agents** lists the rest with how they ended. Click one to open it.
- **Context:** how full the thread's context window is after the agent's newest model call: a bar, the percentage
  and the tokens against the window (**170k / 200k**). The window is the one the agent reports (Claude Code at the end
  of each turn, Codex with every reading), else the model's known window (200k for Claude, 1M for a `[1m]` model);
  with neither, only the tokens show. From 80% the row turns amber (red from 90%) and says to send `/compact`, which
  makes the agent summarize the conversation and free the window. The row appears once the agent has reported a
  reading. The composer footer shows the same reading as **42% context** on its right, with **· /compact** from 80%.
- **Usage:** every account in `accounts.toml`: its name and provider, and a small bar per window it reported
  (**5h**, **Weekly**) with the percentage used and how old that window's own reading is (**3m**). A window whose reset
  time has passed is dimmed and reads **reset**: its old percentage no longer holds. When the newest reading says the
  account is refused, the line reads **Limited · resets in 2h 14m** in the warning color until the reset; a refusal
  that names no reset time reads **Limited** for 6 hours after its reading, then stops showing. The readings come from
  what the agents already report in that account's threads (Claude's `rate_limit_event` lines, Codex's rate-limit
  updates); SkyeLab never asks an account for them. An account with nothing reported yet shows **—**.
- **Pull request:** the pull request opened from the thread. Without one, **Link pull request…** takes a pasted
  `https://github.com/<owner>/<repo>/pull/<number>` link (anything else is refused) and keeps it on the thread; it
  then shows here and as the `#<number>` badge on the thread's sidebar row. The unlink button at the end of the row
  forgets the link (the pull request itself is untouched) without asking; the notice that follows has **Undo**.
  Once read, the row shows the PR's state (**Open**, **Draft**, **Merged**, **Closed**, colored like its sidebar badge),
  then its checks (passing, running, or **N checks failing** with each failing check listed; click one to open its
  page) and its review (**Approved**, **Changes requested**, **Awaiting review**). SkyeLab reads it with the thread's
  account's own `gh`, only while the thread's folder is still bound to that account, one PR at a time: an open PR
  every 5 minutes, and the shown thread's again on window focus or when you open it if its last read is a minute old. A merged PR is never read again, across restarts too. A closed one is read again only when you open its thread or focus its window, at most once an hour, so a reopened PR turns Open
  (linking another PR starts over). An account with no GitHub login shows **Sign in to GitHub** (it opens
  Settings → Source control) and `gh` is never run for it. The sidebar badge takes the state's icon and color, with a small
  check mark, dot or cross for the checks while the PR is open.
- **Editor:** **Open in <editor>** runs the editor described under **Open in editor**. The arrow lists the editors on
  your PATH (VS Code, Cursor, Zed, Windsurf, Sublime Text); picking one opens the folder in it and saves it as
  `editor` in `settings.toml`.

With no thread on screen, the tab shows only Usage.

## Dialog backdrop

Dialogs (the palette, Add project, Rename) dim the window behind them. GPUI can't blur the window's own content, so
the backdrop is the window's background color at 75%.

## Usage page

**Settings → Usage** (also **Usage** in the sidebar footer, or **Open Usage** in the palette) puts every account in
accounts.toml side by side. Pick **Today**, **7 days** or **30 days**, and **Cost** or **Tokens**:

- the total, then one line per account with its turns, its share of the total, its newest 5h and weekly readings and
  **Limited · resets in …** while it is limited;
- **Daily cost** (or tokens) for the last 30 days, one stacked bar per day in each account's color;
- **Projects**: each project's cost, tokens and turns per account, highest first.

The numbers are what this machine's agents reported at the end of each turn (Claude reports a cost, Codex only
tokens); SkyeLab never asks an account. Imported session history isn't counted, and a deleted thread's turns still
are. Claude reports a running total for its process (and, since Claude Code 2.1.277, for a resumed session), so
each turn counts what it added to that total; turns stored by older SkyeLab builds were corrected when the store
upgraded. Claude's cost is its own estimate from list prices, not a bill.
