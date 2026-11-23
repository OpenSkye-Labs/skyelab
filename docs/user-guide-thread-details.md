# Thread details

The right panel's **Details** tab is a card stack for the thread on screen. The right-panel toggle at the top
right (<kbd>⌥⌘R</kbd>, <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>R</kbd> elsewhere) opens it, and it stays as the panel's first
tab beside Changes, browsers and Devices. Everything in it acts on the thread's own folder under its bound account.

- **Environment:** this machine's name, with a button that opens the thread's folder in Finder (or your file
  manager). On a git repo, then:
  - **The branch** opens the branch picker (a worktree thread shows its own branch, which doesn't switch).
  - **Commit & push** opens Changes, where Commit, Push and Open PR are.
  - **Changes vs <base>** shows the lines added and removed since the worktree's base commit, or since `HEAD`
    (untracked files aren't counted), and opens Changes.
  - A worktree thread also has **Open terminal here** and **Discard worktree** (see **Worktrees**).
- **Actions:** **Add project script** (or **Run project script** once the project has some) opens the project's
  scripts.
- **Lineage · N running:** the threads this thread's agent delegated that are working now, with how long they have
  run. **Previous agents** lists the rest with how they ended. Click one to open it.
- **Usage:** every account in `accounts.toml`, with the newest reading SkyeLab has of its limits (Session, Weekly)
  and how old it is ("as of 12m ago"). The readings come from the `rate_limit_event` lines Claude writes in that
  account's threads; SkyeLab never asks an account for them. Claude doesn't always include the numbers, and Codex
  readings aren't read yet, so an account with nothing reported shows **—**.
- **Pull request:** the pull request opened from the thread, when there is one.
- **Editor:** **Open in <editor>** runs the editor described under **Open in editor**. The arrow lists the editors on
  your PATH (VS Code, Cursor, Zed, Windsurf, Sublime Text); picking one opens the folder in it and saves it as
  `editor` in `settings.toml`.

With no thread on screen, the tab shows only Usage.

## Dialog backdrop

Dialogs (the palette, Add project, Rename) dim the window behind them. GPUI can't blur the window's own content, so
the backdrop is the window's background color at 75%.
