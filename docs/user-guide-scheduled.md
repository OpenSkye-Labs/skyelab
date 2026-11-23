# Scheduled tasks

Part of the [user guide](user-guide.md).

A scheduled task is a saved prompt that starts a new thread in a project on a schedule. Open **Settings → Scheduled
tasks**, or pick **Schedule this prompt…** in the command palette while a composer has focus: that opens the editor
with the composer's text and project.

## The editor

- **Name** (optional: the prompt's first line) and **Project**.
- **Prompt**: what each run sends.
- **Schedule**: every N minutes, every N hours, daily at a time, weekdays (Monday to Friday) at a time, or a
  five-field cron expression (`minute hour day month weekday`, e.g. `0 9 * * 1-5`). Times are this machine's local
  time; a time that daylight saving skips runs once at the jump, and one it repeats runs once.
- **Runs in**: the project's checkout, or a new worktree per run.
- **Permissions**: Ask, Plan, Accept edits or Full access. Full access needs its own confirm on the task, because a
  scheduled run has nobody watching; without it the task can't be saved. A run in Ask mode stops at the first
  permission question until you answer it in the thread; an **Approval needed** notice says so. While it waits it
  holds its slot.
- **Model**: a model id, or empty for the agent's default.

A task is saved under the account its project folder is bound to at that moment. An unbound folder can't get a task.

## When it runs

- Only while SkyeLab is running. There is no background service.
- A run missed while SkyeLab was closed (or the machine slept for more than 10 minutes) runs once at the next start,
  however many slots it missed. Turn off **Run missed tasks at start** to skip those instead.
- One run of a task at a time: if its last run is still starting or working, that slot is skipped. When a new run
  starts, the task's earlier runs that finished close their agent (their threads stay and can be resumed), unless one
  is on screen.
- A cron expression that never comes (`0 9 31 2 *`) can't be saved.
- At most 2 scheduled runs work at once, app-wide (`max_concurrent` under `[scheduled]` in `settings.toml`). A task
  that comes due while they are busy waits and starts when one finishes, however long that takes.
- With several windows open there is still one scheduler; its threads open in the first window.

## Accounts

Each run resolves the folder's account again, from `accounts.toml` as it is on disk. If the folder is no longer bound,
is bound to a different account than the task was saved under, or the file can't be read, the run is skipped, the
task is **paused**, and a notice says why. SkyeLab never falls back to another account. Fix the binding (or edit and
save the task to move it to the new account), then turn the task back on: saving an edit never turns a paused task
back on.

## The list

Each task shows its schedule, its next run (or **Paused**), how its last run went (started, skipped, paused, failed),
a link to the thread it last started, and controls: the switch (on or paused), **Run now**, **Edit** and **Delete**
(click twice). Threads a task started carry a **Scheduled** badge in the sidebar.
