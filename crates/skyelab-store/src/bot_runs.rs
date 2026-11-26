//! Bots' run history (every run a task made, skipped or was refused, by what) and the GitHub items each bot's poller
//! has already seen, so an item fires once.

use crate::{Store, expect_one};
use anyhow::Result;
use rusqlite::{OptionalExtension as _, Row, params};
use std::collections::HashSet;

/// Version 28: bots. A task's GitHub triggers, whose items fire (`anyone`/`me`), its hourly cap, whether its poller
/// has recorded what already existed, its runs, and the items seen.
pub(crate) const MIGRATION_V28: &str = "
ALTER TABLE scheduled_tasks ADD COLUMN triggers TEXT NOT NULL DEFAULT '';
ALTER TABLE scheduled_tasks ADD COLUMN author_filter TEXT NOT NULL DEFAULT 'anyone';
ALTER TABLE scheduled_tasks ADD COLUMN max_runs_per_hour INTEGER NOT NULL DEFAULT 6;
ALTER TABLE scheduled_tasks ADD COLUMN gh_primed INTEGER NOT NULL DEFAULT 0;
CREATE TABLE bot_runs (
    id INTEGER PRIMARY KEY,
    task_id INTEGER NOT NULL,
    at INTEGER NOT NULL,
    trigger TEXT NOT NULL,
    item_url TEXT,
    item_prompt TEXT,
    status TEXT NOT NULL,
    error TEXT,
    thread_id INTEGER
);
CREATE INDEX bot_runs_by_task ON bot_runs(task_id, at);
CREATE INDEX bot_runs_by_thread ON bot_runs(thread_id) WHERE thread_id IS NOT NULL;
CREATE TABLE bot_seen (
    task_id INTEGER NOT NULL,
    key TEXT NOT NULL,
    at INTEGER NOT NULL,
    PRIMARY KEY (task_id, key)
);
";

/// Runs kept per task; older ones are pruned as new ones come.
const KEEP_RUNS: i64 = 200;
/// Seen keys older than this are pruned: an item that old won't be listed as new again.
const KEEP_SEEN_MS: i64 = 30 * 24 * 3_600_000;
/// Triggers whose item text is untrusted.
const GITHUB: &str = "('pr_opened', 'issue_opened', 'checks_failed')";

/// One run of a task. `trigger` is `schedule`, `run_now` or a GitHub trigger's name; `status` is `queued`,
/// `started`, `skipped`, `paused`, `failed` or `rate_limited`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotRun {
    pub id: i64,
    pub task_id: i64,
    pub at: i64,
    pub trigger: String,
    pub item_url: Option<String>,
    /// The framed prompt a queued GitHub hit starts with; cleared once it leaves the queue.
    pub item_prompt: Option<String>,
    pub status: String,
    pub error: Option<String>,
    pub thread_id: Option<i64>,
}

/// A run to append.
#[derive(Debug, Clone, Copy, Default)]
pub struct NewBotRun<'a> {
    pub task: i64,
    pub at: i64,
    pub trigger: &'a str,
    pub item_url: Option<&'a str>,
    pub item_prompt: Option<&'a str>,
    pub status: &'a str,
    pub error: Option<&'a str>,
}

const COLS: &str = "id, task_id, at, trigger, item_url, item_prompt, status, error, thread_id";

fn run_from_row(r: &Row) -> rusqlite::Result<BotRun> {
    Ok(BotRun {
        id: r.get(0)?,
        task_id: r.get(1)?,
        at: r.get(2)?,
        trigger: r.get(3)?,
        item_url: r.get(4)?,
        item_prompt: r.get(5)?,
        status: r.get(6)?,
        error: r.get(7)?,
        thread_id: r.get(8)?,
    })
}

impl Store {
    /// Appends a run and prunes the task's history to the newest [`KEEP_RUNS`]. Returns its id.
    pub fn add_bot_run(&self, run: &NewBotRun) -> Result<i64> {
        let &NewBotRun {
            task,
            at,
            trigger,
            item_url,
            item_prompt,
            status,
            error,
        } = run;
        self.conn.execute(
            "INSERT INTO bot_runs (task_id, at, trigger, item_url, item_prompt, status, error)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![task, at, trigger, item_url, item_prompt, status, error],
        )?;
        let id = self.conn.last_insert_rowid();
        self.conn.execute(
            "DELETE FROM bot_runs WHERE task_id = ?1 AND id NOT IN
                (SELECT id FROM bot_runs WHERE task_id = ?1 ORDER BY at DESC, id DESC LIMIT ?2)",
            params![task, KEEP_RUNS],
        )?;
        Ok(id)
    }

    /// A queued run left the queue: it started, failed or was refused. Its framed prompt is dropped.
    pub fn update_bot_run(&self, id: i64, status: &str, error: Option<&str>) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE bot_runs SET status = ?2, error = ?3, item_prompt = NULL WHERE id = ?1",
            params![id, status, error],
        )?;
        expect_one(n, "bot run", id)
    }

    pub fn set_bot_run_thread(&self, id: i64, thread: i64) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE bot_runs SET thread_id = ?2 WHERE id = ?1",
            params![id, thread],
        )?;
        expect_one(n, "bot run", id)
    }

    /// Task `task`'s runs, newest first, at most `limit`.
    pub fn bot_runs(&self, task: i64, limit: i64) -> Result<Vec<BotRun>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {COLS} FROM bot_runs WHERE task_id = ?1 ORDER BY at DESC, id DESC LIMIT ?2"
        ))?;
        let rows = stmt.query_map(params![task, limit], run_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Task `task`'s oldest GitHub hit still waiting for a free slot.
    pub fn queued_bot_run(&self, task: i64) -> Result<Option<BotRun>> {
        let sql = format!(
            "SELECT {COLS} FROM bot_runs WHERE task_id = ?1 AND status = 'queued' ORDER BY at, id LIMIT 1"
        );
        Ok(self.conn.query_row(&sql, [task], run_from_row).optional()?)
    }

    /// The thread was started by a GitHub trigger: its history holds untrusted text, so it never gets SkyeLab's
    /// tools, a resumed turn included.
    pub fn thread_untrusted(&self, thread: i64) -> Result<bool> {
        Ok(self.conn.query_row(
            &format!("SELECT EXISTS (SELECT 1 FROM bot_runs WHERE thread_id = ?1 AND trigger IN {GITHUB})"),
            [thread],
            |r| r.get(0),
        )?)
    }

    /// The branches bot runs made in project `project`: PRs from them never trigger a bot.
    pub fn bot_branches(&self, project: i64) -> Result<HashSet<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT worktree_branch FROM threads
             WHERE project_id = ?1 AND scheduled_task_id IS NOT NULL AND worktree_branch IS NOT NULL",
        )?;
        let rows = stmt.query_map([project], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The item keys task `task`'s poller has seen.
    pub fn bot_seen(&self, task: i64) -> Result<HashSet<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT key FROM bot_seen WHERE task_id = ?1")?;
        let rows = stmt.query_map([task], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Records `keys` as seen at `at`, and prunes keys older than 30 days. The poller marks every item still listed
    /// each time, so only items gone from the list for a month are pruned.
    pub fn mark_bot_seen(&self, task: i64, keys: &[String], at: i64) -> Result<()> {
        for key in keys {
            self.conn.execute(
                "INSERT OR REPLACE INTO bot_seen (task_id, key, at) VALUES (?1, ?2, ?3)",
                params![task, key, at],
            )?;
        }
        self.conn.execute(
            "DELETE FROM bot_seen WHERE task_id = ?1 AND at < ?2",
            params![task, at - KEEP_SEEN_MS],
        )?;
        Ok(())
    }

    /// The poller recorded what existed: from now on new items fire.
    pub fn set_gh_primed(&self, task: i64) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE scheduled_tasks SET gh_primed = 1 WHERE id = ?1",
            [task],
        )?;
        expect_one(n, "scheduled task", task)
    }
}
