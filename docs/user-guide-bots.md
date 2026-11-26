# Bots and scheduled tasks

Part of the [user guide](user-guide.md).

A bot is a saved prompt that starts a new thread in a project on a schedule, when GitHub has news in the project's
repo (a new pull request, a new issue, or a pull request whose checks fail), or both. A bot with only a schedule is
what earlier versions called a scheduled task. Open **Bots** in the left rail (Settings → Scheduled tasks links there),
or pick **Schedule this prompt…** in the command palette while a composer has focus: that opens the editor with the
composer's text and project. With no bots yet, two starters open the editor prefilled: **PR review** and **Issue
triage**, both in Plan mode.

## The editor

- **Name** (optional: the prompt's first line) and **Project**.
- **Prompt**: what each run sends.
- **Schedule**: none (GitHub triggers only), every N minutes, every N hours, daily at a time, weekdays (Monday to Friday) at a time, or a
  five-field cron expression (`minute hour day month weekday`, e.g. `0 9 * * 1-5`). Times are this machine's local
  time; a time that daylight saving skips runs once at the jump, and one it repeats runs once. When the machine's time
  zone changes, daily, weekday and cron tasks move to the new zone's time. Intervals keep their beat: a task every
  30 minutes that first ran at 9:00 runs at 9:30, 10:00 and so on, however late a check started it.
- **GitHub triggers**: **New PR**, **New issue** and **Checks failed**, whose items fire (**Anyone's**, or **Only
  mine**: the account's own GitHub login), and **At most N an hour** (1 to 30, 6 by default). They need the project to
  have a github.com `origin` and its account's GitHub CLI to be signed in; otherwise they are off and say why, with a
  **Sign in** link. See [GitHub triggers](#github-triggers).
- **Runs in**: the project's checkout, or a new worktree per run. A bot with a GitHub trigger always gets a new
  worktree (Local checkout is off), branched from the default branch.
- **Permissions**: Plan (the default for a new bot), Ask, Accept edits or Full access. A bot with a GitHub trigger
  runs only in Plan or Ask (only Plan on a Codex, Cursor or Antigravity account, whose Ask still edits files without
  asking): the editor offers only those, and a run is refused if the bot somehow has another. Full access needs its own confirm on the task, because a
  scheduled run has nobody watching; without it the task can't be saved. A run in Ask mode stops at the first
  permission question until you answer it in the thread; an **Approval needed** notice says so, and the bot's card
  says **Waiting for approval**. While it waits it holds its slot.
- **Model**: a model id, or empty for the agent's default.

A task is saved under the account its project folder is bound to at that moment. An unbound folder can't get a task.

## When it runs

- Only while SkyeLab is running. There is no background service. On macOS SkyeLab keeps running with every window
  closed: a task that comes due then opens a window and runs in it.
- A run missed while SkyeLab was closed (or the machine slept for more than 10 minutes) runs once at the next start,
  however many slots it missed. Turn off **Run missed tasks at start** to skip those instead.
- One run of a task at a time: if its last run is still starting or working, that slot is skipped. When a new run
  starts, the task's earlier runs that finished close their agent (their threads stay and can be resumed), unless one
  is on screen.
- A cron expression that never comes (`0 9 31 2 *`) can't be saved.
- At most 2 scheduled runs work at once, app-wide (`max_concurrent` under `[scheduled]` in `settings.toml`). A task
  that comes due while they are busy waits and starts when one finishes, however long that takes.
- With several windows open there is still one scheduler; its threads open in the first window.

## GitHub triggers

- Every 5 minutes, per account and project, SkyeLab runs one `gh pr list` and/or `gh issue list` through that account's
  own GitHub CLI (its config dir, a clean environment). There are no webhooks: nothing listens on the internet.
- Before every poll the folder's binding is checked, as before a run: if it no longer resolves to the bot's account,
  the bot is paused and gh isn't run.
- The first poll after a bot is saved or turned back on only records what is already open: nothing that existed before
  fires. After that, each new PR, new issue, or failing head commit of an open PR fires once. Draft PRs fire once
  they are marked ready. PRs from a branch a bot run made never fire a bot; a PR from a fork does, whatever its
  branch is called.
- Each hit waits in its bot's queue and starts like a scheduled run: one live run per bot, at most 2 bot runs app-wide.
  Over the bot's hourly cap, or with 5 hits already waiting, a hit is recorded as **rate limited** and dropped. A bot
  rate limited three hours in a row is paused; turned back on, it starts counting again.
- Only the newest 50 open PRs and issues are listed. In a repo with more open than that, an older one that moves into
  the list (because a newer one closed) fires as new.
- A GitHub-started run gets the bot's prompt plus the item: its link, title, labels and description, inside a fenced
  block under a line saying it is the author's text, to be used as context and never as instructions. `@mentions` and a
  leading `/command` (fullwidth `＠` and `／` too) are broken so they can't act as such, and zero-width characters
  are dropped. Put `{{item}}` in the prompt to place the block there;
  otherwise it goes at the end. Comments are not read.
- Those runs start without SkyeLab's own tools (starting threads in other projects, the browser, devices), and so does
  any later turn in those threads. The run's worktree comes from the project's default branch (the one `origin/HEAD`
  names, else `main` or `master`), never from the folder's checkout or the PR's head, so a PR never runs its own code;
  the agent can read the diff with `gh pr diff`.
- A Claude run started by GitHub loads none of the account's settings, so its allow rules never let a tool run
  unasked; the account's `permissions.deny` and `permissions.ask` rules still apply. Its asks offer only **Allow
  once**: no **Allow for this thread**, command prefix or project rule, and saved project rules don't answer there.
- Bots only report in their thread. They don't post reviews or comments; you do.

## Accounts

Each run resolves the folder's account again, from `accounts.toml` as it is on disk. If the folder is no longer bound,
is bound to a different account than the task was saved under, or the file can't be read, the run is skipped, the
task is **paused**, and a notice says why. SkyeLab never falls back to another account. Fix the binding (or edit and
save the task to move it to the new account), then turn the task back on: saving an edit never turns a paused task
back on.

## The list

Each bot shows what starts it (its schedule, its GitHub triggers) and its permissions, its next run (**Watching
GitHub** for one with no schedule, **Paused** when SkyeLab paused it), how its last run went (started, skipped, paused,
failed), a link to the thread it last started, why its last GitHub poll listed nothing, and controls: the run history
toggle, the switch, **Run now**, **Edit** and **Delete** (click twice). The history lists the newest 50 runs (**Show
more** for the rest, up to 200): status (queued, started, skipped, paused, failed, rate limited), what started it,
the item (`#42`), when, why, and its thread. Threads a bot started carry a **Bot** badge in the sidebar. The **Bots**
rail item shows how many bots need you: paused ones, and open runs waiting on an approval.
