# User guide: branches, scripts, the diff and turns

Back to the [user guide](user-guide.md).

## Branches

The branch row in the thread details card (⇧⌘B) lists the folder's local branches, most recent first, with a search. On a thread
that runs in the project folder, picking one switches the folder's checkout; type a name that doesn't exist and
pick **Create branch** to make it from the current commit and switch to it (your changes stay as they are). The
search has the focus as soon as the list opens. A branch checked out in another worktree (a worktree thread's) is
marked **in a worktree**: git can't switch to it here, so SkyeLab says so and changes nothing. Stop the
agent first. With uncommitted changes, SkyeLab asks before switching:

- **Stash and switch** stashes every change, untracked files included, as `skyelab: changes left on the way to <branch>`.
  Get them back with `git stash pop` on the old branch.
- **Take changes along** switches with the changes, as plain `git switch` does. If a change would be overwritten,
  git refuses and nothing moves.

Ignored files are never overwritten: if the other branch tracks a file you have as an ignored one, the switch is
refused. If the changes move on while the question is shown, SkyeLab asks again.

## Project scripts

The thread details card's **Add project script** / **Run project script** row holds a project's scripts: a name and a command, such as `npm run dev`. Click one to run
it in a new terminal tab in the thread's folder (or its worktree), on the folder's account, with the terminal's
usual environment plus `SKYELAB_PROJECT_ROOT` and, in a worktree, `SKYELAB_WORKTREE_PATH`. A folder with no
account bound runs nothing. Tick **Run in every new worktree** for setup scripts such as `pnpm install`: they run as
soon as a worktree thread you start begins (not for threads an agent delegates). The command palette lists every script as **Run script: <name>** (it runs in the
project folder). Scripts are kept in SkyeLab's own database, never in the repo, so an agent can't add one.

## Reading the diff

The Changes tab lists the changed files as a folder tree on the left: click a folder to fold it, a file to jump to it
in the diff. A folder that only holds one folder is shown as one row (`crates/app/src`). The buttons in its header
switch between a **unified** diff and a **split** one (old on the left, new on the right), turn **line wrapping** on
or off, and **hide whitespace changes** (`git diff -w`: a file whose only change is whitespace shows as such). SkyeLab
remembers all three in `settings.toml` under `[diff]`.

Click in the diff and press <kbd>⌘F</kbd> (<kbd>Ctrl+F</kbd>) to find in it: the bar counts every match in the file
names and lines, <kbd>↩</kbd> and <kbd>⇧↩</kbd> step through them, and <kbd>Esc</kbd> closes it.

## Commenting on lines

Click a line's number (its gutter), or drag down the gutter across several lines, to comment on them: a field opens
under the lines. <kbd>⌘↩</kbd> (<kbd>Ctrl+↩</kbd>) or **Comment** keeps it, **Cancel** or <kbd>Esc</kbd> drops it. In
the split view, the left gutter comments on the old lines and the right one on the new lines. A comment stays in one
file; a drag past its end stops there.

Comments show under their lines. The strip above the diff counts them; **Send N comments** puts them all in the
thread's composer as one chip (each comment's file and lines, the lines themselves and your note), sent with your
next message as a fenced block, and removes them from the diff. **Clear** drops them.

Each thread keeps its own comments, per diff (the working tree, or one turn). When the diff reloads (a turn ended, a
commit), each comment finds its lines again, even if they moved; a comment whose lines are gone is dropped, and the
strip says how many. A comment's message shows at most 30 of its lines. Comments are kept in memory only: closing the
thread's tab or quitting SkyeLab loses unsent ones.

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
