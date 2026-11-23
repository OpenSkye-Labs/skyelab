# User guide: branches, scripts and turns

Back to the [user guide](user-guide.md).

## Branches

The branch row in the right panel's Details tab lists the folder's local branches, most recent first, with a search. On a thread
that runs in the project folder, picking one switches the folder's checkout; type a name that doesn't exist and
pick **Create branch** to make it from the current commit and switch to it (your changes stay as they are). Stop the
agent first. With uncommitted changes, SkyeLab asks before switching:

- **Stash and switch** stashes every change, untracked files included, as `skyelab: changes left on the way to <branch>`.
  Get them back with `git stash pop` on the old branch.
- **Take changes along** switches with the changes, as plain `git switch` does. If a change would be overwritten,
  git refuses and nothing moves.

Ignored files are never overwritten: if the other branch tracks a file you have as an ignored one, the switch is
refused. If the changes move on while the question is shown, SkyeLab asks again.

## Project scripts

The Details tab's **Add project script** / **Run project script** row holds a project's scripts: a name and a command, such as `npm run dev`. Click one to run
it in a new terminal tab in the thread's folder (or its worktree), on the folder's account, with the terminal's
usual environment plus `SKYELAB_PROJECT_ROOT` and, in a worktree, `SKYELAB_WORKTREE_PATH`. A folder with no
account bound runs nothing. Tick **Run in every new worktree** for setup scripts such as `pnpm install`: they run as
soon as a worktree thread you start begins (not for threads an agent delegates). The command palette lists every script as **Run script: <name>** (it runs in the
project folder). Scripts are kept in SkyeLab's own database, never in the repo, so an agent can't add one.

## Turns and rewind

SkyeLab records a checkpoint of the thread's folder right before each agent turn and when it ends: every file git
would see, untracked ones included and ignored ones not. Checkpoints live under `refs/skyelab/checkpoints/` in the
repo; your branches, index and stashes are never touched. Once a turn has ended, the Changes tab shows a row of
scopes: **Working tree** (the diff it always showed), **Since thread start**, and **Turn 1**, **Turn 2** and so on.

On a turn, **Rewind to before turn N** puts the files back as they were when that turn started. It lists every file
it would restore and delete before it does anything, and changes nothing if the files moved since. Stop the agent
first. Notes:

- It rewinds files only, not the conversation, and leaves your staging (the index) as it is.
- A thread's checkpoints go when the thread is deleted for good (after the undo window); archiving keeps them.
- On a thread in the project folder (no worktree) it also undoes your own edits made since that turn started.
- Ignored files are left alone. If an ignored file or folder stands where the checkpoint has a file, the confirm
  names that file, and it is written (replacing what's in the way) only if you tick **Overwrite ignored files**.
- It never writes through a symlinked folder, so nothing outside the repo changes; if it can't do every part, it
  does none. It skips submodules, and only touches files inside the thread's folder.
