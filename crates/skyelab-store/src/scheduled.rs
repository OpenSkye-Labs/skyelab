//! Scheduled tasks: a saved prompt that starts a new thread in a project on a schedule. The schedule rules live in
//! `skyelab_core::schedule`; this keeps the tasks, their last run, and which threads they started.

use crate::{Store, expect_one, now_ms};
use anyhow::{Context as _, Result};
use rusqlite::{OptionalExtension as _, Row, params};

/// Version 21: scheduled tasks, and the task a thread was started by.
pub(crate) const MIGRATION_V21: &str = "
CREATE TABLE scheduled_tasks (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL,
    title TEXT NOT NULL,
    prompt TEXT NOT NULL,
    schedule TEXT NOT NULL,
    worktree INTEGER NOT NULL DEFAULT 0,
    permission_mode TEXT NOT NULL DEFAULT 'default',
    bypass_confirmed INTEGER NOT NULL DEFAULT 0,
    model TEXT,
    effort TEXT,
    account_id TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    next_run_at INTEGER,
    last_run_at INTEGER,
    last_status TEXT,
    last_error TEXT,
    created_at INTEGER NOT NULL
);
ALTER TABLE threads ADD COLUMN scheduled_task_id INTEGER;
CREATE INDEX threads_scheduled_task ON threads(scheduled_task_id) WHERE scheduled_task_id IS NOT NULL;
";

/// One task. `id` 0 is a task not saved yet.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScheduledTask {
    pub id: i64,
    pub project_id: i64,
    pub title: String,
    pub prompt: String,
    /// `skyelab_core::schedule::Schedule`'s spec text.
    pub schedule: String,
    /// Each run gets its own worktree; off runs in the project's checkout.
    pub worktree: bool,
    /// The agent layer's permission mode value.
    pub permission_mode: String,
    /// The user confirmed Full access for this task's unattended runs.
    pub bypass_confirmed: bool,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// The account the folder resolved to when the task was saved; a run under any other is refused.
    pub account_id: String,
    pub enabled: bool,
    pub next_run_at: Option<i64>,
    pub last_run_at: Option<i64>,
    /// `started`, `skipped`, `paused` or `failed`.
    pub last_status: Option<String>,
    pub last_error: Option<String>,
    pub created_at: i64,
}

const COLS: &str = "id, project_id, title, prompt, schedule, worktree, permission_mode, bypass_confirmed, model, effort, \
                    account_id, enabled, next_run_at, last_run_at, last_status, last_error, created_at";

fn task_from_row(r: &Row) -> rusqlite::Result<ScheduledTask> {
    Ok(ScheduledTask {
        id: r.get(0)?,
        project_id: r.get(1)?,
        title: r.get(2)?,
        prompt: r.get(3)?,
        schedule: r.get(4)?,
        worktree: r.get(5)?,
        permission_mode: r.get(6)?,
        bypass_confirmed: r.get(7)?,
        model: r.get(8)?,
        effort: r.get(9)?,
        account_id: r.get(10)?,
        enabled: r.get(11)?,
        next_run_at: r.get(12)?,
        last_run_at: r.get(13)?,
        last_status: r.get(14)?,
        last_error: r.get(15)?,
        created_at: r.get(16)?,
    })
}

impl Store {
    /// Inserts a new task (`id` 0) or updates what the editor sets; the last run's record stays. Returns its id.
    pub fn save_scheduled_task(&self, t: &ScheduledTask) -> Result<i64> {
        let p = params![
            t.project_id,
            t.title,
            t.prompt,
            t.schedule,
            t.worktree,
            t.permission_mode,
            t.bypass_confirmed,
            t.model,
            t.effort,
            t.account_id,
            t.enabled,
            t.next_run_at,
        ];
        if t.id == 0 {
            self.conn
                .execute(
                    "INSERT INTO scheduled_tasks (project_id, title, prompt, schedule, worktree, permission_mode,
                        bypass_confirmed, model, effort, account_id, enabled, next_run_at, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                    rusqlite::params_from_iter(p.iter().copied().chain([&now_ms() as _])),
                )
                .context("save a scheduled task")?;
            return Ok(self.conn.last_insert_rowid());
        }
        let n = self.conn.execute(
            "UPDATE scheduled_tasks SET project_id = ?1, title = ?2, prompt = ?3, schedule = ?4, worktree = ?5,
                permission_mode = ?6, bypass_confirmed = ?7, model = ?8, effort = ?9, account_id = ?10,
                enabled = ?11, next_run_at = ?12
             WHERE id = ?13",
            rusqlite::params_from_iter(p.iter().copied().chain([&t.id as _])),
        )?;
        expect_one(n, "scheduled task", t.id)?;
        Ok(t.id)
    }

    /// Every task, oldest first.
    pub fn scheduled_tasks(&self) -> Result<Vec<ScheduledTask>> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT {COLS} FROM scheduled_tasks ORDER BY id"))?;
        let rows = stmt.query_map([], task_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn scheduled_task(&self, id: i64) -> Result<Option<ScheduledTask>> {
        let sql = format!("SELECT {COLS} FROM scheduled_tasks WHERE id = ?1");
        Ok(self.conn.query_row(&sql, [id], task_from_row).optional()?)
    }

    /// The threads it started keep their badge.
    pub fn delete_scheduled_task(&self, id: i64) -> Result<()> {
        let n = self
            .conn
            .execute("DELETE FROM scheduled_tasks WHERE id = ?1", [id])?;
        expect_one(n, "scheduled task", id)
    }

    pub fn set_scheduled_task_enabled(&self, id: i64, enabled: bool) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE scheduled_tasks SET enabled = ?2 WHERE id = ?1",
            params![id, enabled],
        )?;
        expect_one(n, "scheduled task", id)
    }

    pub fn set_scheduled_next_run(&self, id: i64, next: Option<i64>) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE scheduled_tasks SET next_run_at = ?2 WHERE id = ?1",
            params![id, next],
        )?;
        expect_one(n, "scheduled task", id)
    }

    /// What its last run did, and when.
    pub fn record_scheduled_run(
        &self,
        id: i64,
        at: i64,
        status: &str,
        error: Option<&str>,
    ) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE scheduled_tasks SET last_run_at = ?2, last_status = ?3, last_error = ?4 WHERE id = ?1",
            params![id, at, status, error],
        )?;
        expect_one(n, "scheduled task", id)
    }

    /// Marks thread `id` as started by task `task`.
    pub fn set_thread_scheduled_task(&self, id: i64, task: i64) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE threads SET scheduled_task_id = ?2 WHERE id = ?1",
            params![id, task],
        )?;
        expect_one(n, "thread", id)
    }

    /// The newest thread task `task` started that still exists.
    pub fn last_scheduled_thread(&self, task: i64) -> Result<Option<i64>> {
        Ok(self.conn.query_row(
            "SELECT MAX(id) FROM threads WHERE scheduled_task_id = ?1 AND deleted_at IS NULL",
            [task],
            |r| r.get(0),
        )?)
    }

    /// Each task's newest thread that still exists, with its status: (task id, thread id, status).
    pub fn last_scheduled_runs(&self) -> Result<Vec<(i64, i64, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT scheduled_task_id, id, status FROM threads WHERE id IN (
                SELECT MAX(id) FROM threads WHERE scheduled_task_id IS NOT NULL AND deleted_at IS NULL
                GROUP BY scheduled_task_id)",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}
