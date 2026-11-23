# Thread details

The right panel's **Details** tab is a card stack for the thread on screen. The right-panel toggle at the top
right (<kbd>⌥⌘R</kbd>, <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>R</kbd> elsewhere) opens it, and it stays as the panel's first
tab beside Changes, browsers and Devices. Everything in it acts on the thread's own folder under its bound account.

- **Environment:** this machine's name, with a button that opens the thread's folder in Finder (or your file
  manager). On a git repo, then:
  - **The branch** opens the branch picker (a worktree thread shows its own branch, which doesn't switch).
  - **Commit & push** opens a menu that says which GitHub login pushes (the thread's account's own `gh` login, read
    against `accounts.toml` as saved now; if the folder has been bound to another account since, it shows the refusal
    Push would give instead), then
    **Commit**, **Commit & push**, **Push** and **Open PR** (**View PR** once the thread has one). Each runs in the
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
- **Editor:** **Open in <editor>** runs the editor described under **Open in editor**. The arrow lists the editors on
  your PATH (VS Code, Cursor, Zed, Windsurf, Sublime Text); picking one opens the folder in it and saves it as
  `editor` in `settings.toml`.

With no thread on screen, the tab shows only Usage.

## Dialog backdrop

Dialogs (the palette, Add project, Rename) dim the window behind them. GPUI can't blur the window's own content, so
the backdrop is the window's background color at 75%.
