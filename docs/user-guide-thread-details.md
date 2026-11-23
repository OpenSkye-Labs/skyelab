# Thread details

The square-menu button at the top right, beside the panel toggles, opens the thread's details card (also
**Toggle Thread Details** in the palette). Everything in it acts on the thread's own folder under its bound account:

- **This machine's name**, with a button that opens the thread's folder in Finder (or your file manager).
- **Open in <editor>** runs the editor described under **Open in editor** below. The arrow lists the editors on your
  PATH (VS Code, Cursor, Zed, Windsurf, Sublime Text); picking one opens the folder in it and saves it as `editor`
  in `settings.toml`.
- **Add project script** (or **Run project script** once the project has some) opens the project's scripts.
- **The branch** opens the branch picker (a worktree thread shows its own branch, which doesn't switch).
- **Commit & push** opens Changes, where Commit, Push and Open PR are. The pull request opened from the thread is
  listed below it.
- **Changes vs <base>** shows the lines added and removed since the worktree's base commit, or since `HEAD`
  (untracked files aren't counted), and opens Changes.
- A worktree thread also has **Open terminal here** and **Discard worktree** (see **Worktrees**).
- **Lineage · N running** lists the threads this thread's agent delegated that are working now, with how long they
  have run; **Previous agents** lists the rest with how they ended. Click one to open it.
- **Usage** bars (Session, Weekly) show how much of the account's limits are used, as Claude last reported them in
  this thread (its `rate_limit_event`). Claude doesn't always include the numbers, and Codex doesn't report them,
  so the bars show only after a report.

Click outside the card or press the button again to close it.

Dialogs (the palette, Add project, Rename) dim the window behind them. GPUI can't blur the window's own content, so
the backdrop is the window's background color at 75%.

