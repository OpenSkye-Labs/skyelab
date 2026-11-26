//! SQLite persistence for projects, threads and thread events.
//!
//! Synchronous on purpose: the app calls it from a background task. Timestamps are Unix milliseconds.
//! Ids are `AUTOINCREMENT`, so a stale id of a deleted row never points at a new one.
//!
//! Delete rules: a project that still has threads can't be deleted (`ON DELETE RESTRICT`),
//! because each thread records which account ran it and that audit must not vanish as a
//! side effect. Deleting a thread deletes its events (`ON DELETE CASCADE`).

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params};

mod events;
mod extras;
pub use extras::{Draft, DraftRow, ProjectScript};
mod images;
mod import;
mod manage;
mod search;
pub use search::{SearchHit, query_terms};
mod model;
pub mod project_rules;
pub mod scheduled;
mod thread_row;
pub use model::ModelChoice;
pub(crate) use thread_row::{THREAD_COLS, thread_from_row};
mod migrations;
mod rows;
pub use rows::RowChange;
mod settle;
pub mod usage;
use migrations::MIGRATIONS;
pub use usage::UsageReading;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub path: PathBuf,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thread {
    pub id: i64,
    pub project_id: i64,
    pub title: String,
    pub cwd: PathBuf,
    /// The account the thread was created under, kept as an audit record.
    pub account_id: String,
    pub agent_session_id: Option<String>,
    /// Opaque to the store; the agent layer defines the values.
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
    /// The git worktree the thread runs in, when it got its own. Cleared once the worktree is discarded.
    pub worktree_path: Option<PathBuf>,
    /// The worktree's branch. Kept after a discard, like the branch itself.
    pub worktree_branch: Option<String>,
    /// The commit the worktree was created from: its changes are shown against this.
    pub worktree_base: Option<String>,
    /// The pull request opened from this thread.
    pub pr_url: Option<String>,
    /// The thread whose agent delegated this one, if any.
    pub parent_thread_id: Option<i64>,
    /// Which tool calls its agent may make without asking; the agent layer defines the values.
    pub permission_mode: String,
    /// When the user pinned it (pinned threads list first).
    pub pinned_at: Option<i64>,
    /// When the user archived it (listed apart, never notifies).
    pub archived_at: Option<i64>,
    /// A turn finished while the user wasn't looking at it.
    pub unread: bool,
    /// The thread this one was forked from, if any.
    pub forked_from: Option<i64>,
    /// The linked pull request's state when it was last read (`open`, `draft`, `merged`, `closed`).
    pub pr_state: Option<String>,
    /// The thread this one is a side chat of: hidden from the thread lists, listed under it.
    pub side_of: Option<i64>,
    /// The multi-model send this thread is one model of: its siblings share the number.
    pub compare_group: Option<i64>,
    /// The scheduled task that started it: the sidebar's "Scheduled" badge.
    pub scheduled_task_id: Option<i64>,
    /// When the user settled it (done: listed in the Settled shelf).
    pub settled_at: Option<i64>,
    /// Hidden until then; once passed it lists again as woke, until opened.
    pub snoozed_until: Option<i64>,
    /// Usage limited: continues by itself then (T3's Resume at reset), once. Cleared when it fires or the user sends.
    pub limit_resume_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub thread_id: i64,
    /// 1, 2, 3… per thread.
    pub seq: i64,
    pub kind: String,
    /// Opaque JSON; the agent layer defines the event types.
    pub payload_json: String,
    pub created_at: i64,
}

/// A thread to create. Named fields, so title, cwd and account can't be swapped silently.
#[derive(Debug, Clone, Copy)]
pub struct NewThread<'a> {
    pub project_id: i64,
    pub title: &'a str,
    pub cwd: &'a Path,
    /// The account the thread runs under, recorded as the audit.
    pub account_id: &'a str,
    pub status: &'a str,
}

type AppendHook = Box<dyn Fn(&Event) + Send>;

pub struct Store {
    conn: Connection,
    on_append: Option<AppendHook>,
    row_hooks: rows::RowHooks,
    /// The threads [`Store::open`] purged, until [`Store::take_purged`] hands them over (their git checkpoints go too).
    purged: Vec<Thread>,
}

const PROJECT_COLS: &str = "id, name, path, created_at";

impl Store {
    /// Opens (creating if needed) a database file in WAL mode and migrates it. A thread the last run left
    /// `starting` or `running` has no agent any more, so it becomes `interrupted`: it holds no child slot and
    /// reads as done.
    pub fn open(path: &Path) -> Result<Self> {
        let conn =
            Connection::open(path).with_context(|| format!("open store at {}", path.display()))?;
        let mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
        if mode != "wal" {
            bail!("store at {} refused WAL mode (got {mode})", path.display());
        }
        let store =
            Self::init(conn).with_context(|| format!("init store at {}", path.display()))?;
        // shortcut: assumes one app per store; a second instance on the same data dir would interrupt the first
        // one's live threads. Upgrade with an owner pid per row or an instance lock if that becomes real.
        store
            .conn
            .execute(
                "UPDATE threads SET status = 'interrupted' WHERE status IN ('starting', 'running')",
                [],
            )
            .context("marking the last run's threads interrupted")?;
        // A delete whose undo window the last run didn't see out is final now.
        let mut store = store;
        let sql = format!("SELECT {THREAD_COLS} FROM threads WHERE deleted_at IS NOT NULL");
        store.purged = store
            .conn
            .prepare(&sql)?
            .query_map([], thread_from_row)?
            .collect::<rusqlite::Result<_>>()
            .context("listing deleted threads")?;
        store
            .conn
            .execute("DELETE FROM threads WHERE deleted_at IS NOT NULL", [])
            .context("purging deleted threads")?;
        Ok(store)
    }

    /// The threads the last [`Store::open`] purged (once: later calls get none).
    pub fn take_purged(&mut self) -> Vec<Thread> {
        std::mem::take(&mut self.purged)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", true)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let latest = MIGRATIONS.len() as i64;
        let version: i64 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if !(0..=latest).contains(&version) {
            bail!(
                "database schema is version {version}, newer than this build supports ({latest})"
            );
        }
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
            tx.execute_batch(sql)
                .with_context(|| format!("migrate schema to version {}", i + 1))?;
        }
        let slimmed = if version < 12 {
            images::omit_stored_images(&tx).context("migrate schema to version 12")?
        } else {
            0
        };
        tx.pragma_update(None, "user_version", latest)?;
        tx.commit()?;
        if slimmed > 0 {
            // Give the dropped image bytes back to the disk, once. Best effort: the rewrite is already committed, and
            // a full disk or a busy file must not stop the app from opening (the free pages are reused anyway).
            let _ = conn.execute_batch("VACUUM; PRAGMA wal_checkpoint(TRUNCATE);");
        }
        let row_hooks = rows::RowHooks::default();
        rows::install(&mut conn, row_hooks.clone())?;
        Ok(Self {
            conn,
            on_append: None,
            row_hooks,
            purged: Vec::new(),
        })
    }

    /// `path` is stored normalized (`/repo/` and `/repo` are the same project).
    pub fn create_project(&self, name: &str, path: &Path) -> Result<Project> {
        let path: PathBuf = path.components().collect();
        let created_at = now_ms();
        self.conn
            .execute(
                "INSERT INTO projects (name, path, created_at) VALUES (?1, ?2, ?3)",
                params![name, path_str(&path)?, created_at],
            )
            .with_context(|| format!("create project at {}", path.display()))?;
        Ok(Project {
            id: self.conn.last_insert_rowid(),
            name: name.to_owned(),
            path,
            created_at,
        })
    }

    pub fn project(&self, id: i64) -> Result<Option<Project>> {
        let sql = format!("SELECT {PROJECT_COLS} FROM projects WHERE id = ?1");
        Ok(self
            .conn
            .query_row(&sql, [id], project_from_row)
            .optional()?)
    }

    /// Ordered by name. Removed projects are left out.
    pub fn list_projects(&self) -> Result<Vec<Project>> {
        let sql = format!(
            "SELECT {PROJECT_COLS} FROM projects WHERE removed_at IS NULL ORDER BY name, id"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], project_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn rename_project(&self, id: i64, name: &str) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE projects SET name = ?2 WHERE id = ?1",
            params![id, name],
        )?;
        expect_one(n, "project", id)
    }

    /// Fails while the project still has threads (see the crate docs).
    pub fn delete_project(&self, id: i64) -> Result<()> {
        let n = self
            .conn
            .execute("DELETE FROM projects WHERE id = ?1", [id])
            .with_context(|| format!("delete project {id} (refused while it has threads)"))?;
        expect_one(n, "project", id)
    }

    /// Oldest first.
    pub fn project_scripts(&self, project_id: i64) -> Result<Vec<ProjectScript>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, name, command, setup FROM project_scripts WHERE project_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map([project_id], |r| {
            Ok(ProjectScript {
                id: r.get(0)?,
                project_id: r.get(1)?,
                name: r.get(2)?,
                command: r.get(3)?,
                setup: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn add_project_script(
        &self,
        project_id: i64,
        name: &str,
        command: &str,
        setup: bool,
    ) -> Result<ProjectScript> {
        self.conn
            .execute(
                "INSERT INTO project_scripts (project_id, name, command, setup, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![project_id, name, command, setup, now_ms()],
            )
            .with_context(|| format!("add a script to project {project_id}"))?;
        Ok(ProjectScript {
            id: self.conn.last_insert_rowid(),
            project_id,
            name: name.to_owned(),
            command: command.to_owned(),
            setup,
        })
    }

    pub fn delete_project_script(&self, id: i64) -> Result<()> {
        let n = self
            .conn
            .execute("DELETE FROM project_scripts WHERE id = ?1", [id])?;
        expect_one(n, "project script", id)
    }

    pub fn create_thread(&self, new: &NewThread) -> Result<Thread> {
        let now = now_ms();
        self.conn
            .execute(
                "INSERT INTO threads (project_id, title, cwd, account_id, status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                params![
                    new.project_id,
                    new.title,
                    path_str(new.cwd)?,
                    new.account_id,
                    new.status,
                    now
                ],
            )
            .with_context(|| format!("create thread in project {}", new.project_id))?;
        Ok(Thread {
            id: self.conn.last_insert_rowid(),
            project_id: new.project_id,
            title: new.title.to_owned(),
            cwd: new.cwd.to_owned(),
            account_id: new.account_id.to_owned(),
            agent_session_id: None,
            status: new.status.to_owned(),
            created_at: now,
            updated_at: now,
            worktree_path: None,
            worktree_branch: None,
            worktree_base: None,
            pr_url: None,
            parent_thread_id: None,
            permission_mode: "default".to_owned(),
            pinned_at: None,
            archived_at: None,
            unread: false,
            forked_from: None,
            pr_state: None,
            side_of: None,
            compare_group: None,
            scheduled_task_id: None,
            settled_at: None,
            snoozed_until: None,
            limit_resume_at: None,
        })
    }

    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    #[tracing::instrument(name = "store.thread", skip_all)]
    pub fn thread(&self, id: i64) -> Result<Option<Thread>> {
        let sql = format!("SELECT {THREAD_COLS} FROM threads WHERE id = ?1");
        Ok(self
            .conn
            .query_row(&sql, [id], thread_from_row)
            .optional()?)
    }

    /// Most recently updated first, archived ones included. Deleted ones (still in their undo window) and side chats
    /// are left out.
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    #[tracing::instrument(name = "store.list_threads", skip_all)]
    pub fn list_threads(&self, project_id: i64) -> Result<Vec<Thread>> {
        let sql = format!(
            "SELECT {THREAD_COLS} FROM threads WHERE project_id = ?1 AND deleted_at IS NULL AND side_of IS NULL
             ORDER BY updated_at DESC, id DESC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([project_id], thread_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Every listed project's threads, most recently updated first, archived ones included. Deleted threads, side
    /// chats and removed projects are left out.
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    #[tracing::instrument(name = "store.list_all_threads", skip_all)]
    pub fn list_all_threads(&self) -> Result<Vec<Thread>> {
        let cols = THREAD_COLS
            .split(", ")
            .map(|c| format!("t.{c}"))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT {cols} FROM threads t JOIN projects p ON p.id = t.project_id
             WHERE t.deleted_at IS NULL AND t.side_of IS NULL AND p.removed_at IS NULL
             ORDER BY t.updated_at DESC, t.id DESC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], thread_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The user's own title: a generated one never replaces it (only Regenerate does).
    pub fn set_thread_title(&self, id: i64, title: &str) -> Result<()> {
        self.update_thread(id, "title", title)?;
        self.update_thread(id, "title_source", "user")
    }

    pub fn set_thread_status(&self, id: i64, status: &str) -> Result<()> {
        self.update_thread(id, "status", status)
    }

    /// A fork's context prompt (see `create_fork`) is dropped once it has a session of its own.
    pub fn set_agent_session_id(&self, id: i64, session_id: Option<&str>) -> Result<()> {
        self.update_thread(id, "agent_session_id", session_id)?;
        if session_id.is_some() {
            self.conn
                .execute("UPDATE threads SET fork_context = NULL WHERE id = ?1", [id])?;
        }
        Ok(())
    }

    pub fn set_thread_worktree(
        &self,
        id: i64,
        path: Option<&Path>,
        branch: Option<&str>,
    ) -> Result<()> {
        let path = path.map(path_str).transpose()?;
        let n = self.conn.execute(
            "UPDATE threads SET worktree_path = ?2, worktree_branch = ?3, updated_at = ?4 WHERE id = ?1",
            params![id, path, branch, now_ms()],
        )?;
        expect_one(n, "thread", id)
    }

    pub fn set_thread_worktree_base(&self, id: i64, base: Option<&str>) -> Result<()> {
        self.update_thread(id, "worktree_base", base)
    }

    pub fn set_thread_permission_mode(&self, id: i64, mode: &str) -> Result<()> {
        self.update_thread(id, "permission_mode", mode)
    }

    /// Links (or unlinks) the thread's pull request; the state read for the old one goes with it.
    pub fn set_thread_pr_url(&self, id: i64, url: Option<&str>) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE threads SET pr_url = ?2, pr_state = NULL, updated_at = ?3 WHERE id = ?1",
            params![id, url, now_ms()],
        )?;
        expect_one(n, "thread", id)
    }

    /// The state the PR at `url` was read in. A background read, so `updated_at` (the sidebar's order) stays, and a
    /// PR relinked since the read began is left alone.
    pub fn set_thread_pr_state(&self, id: i64, url: &str, state: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE threads SET pr_state = ?3 WHERE id = ?1 AND pr_url = ?2",
            params![id, url, state],
        )?;
        Ok(())
    }

    fn update_thread(
        &self,
        id: i64,
        column: &'static str,
        value: impl rusqlite::ToSql,
    ) -> Result<()> {
        let sql = format!("UPDATE threads SET {column} = ?2, updated_at = ?3 WHERE id = ?1");
        let n = self.conn.execute(&sql, params![id, value, now_ms()])?;
        expect_one(n, "thread", id)
    }

    /// Links a delegated thread to the thread that started it. The parent must exist.
    pub fn set_thread_parent(&self, id: i64, parent: Option<i64>) -> Result<()> {
        self.update_thread(id, "parent_thread_id", parent)
            .with_context(|| format!("link thread {id} to parent {parent:?}"))
    }

    /// The threads `parent` delegated, oldest first.
    pub fn child_threads(&self, parent: i64) -> Result<Vec<Thread>> {
        let sql =
            format!("SELECT {THREAD_COLS} FROM threads WHERE parent_thread_id = ?1 ORDER BY id");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([parent], thread_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Also deletes the thread's events.
    pub fn delete_thread(&self, id: i64) -> Result<()> {
        let n = self
            .conn
            .execute("DELETE FROM threads WHERE id = ?1", [id])?;
        expect_one(n, "thread", id)
    }
}

fn project_from_row(r: &Row) -> rusqlite::Result<Project> {
    Ok(Project {
        id: r.get(0)?,
        name: r.get(1)?,
        path: PathBuf::from(r.get::<_, String>(2)?),
        created_at: r.get(3)?,
    })
}

/// Paths are stored as TEXT; a lossy conversion could make two folders collide on `UNIQUE`.
fn path_str(path: &Path) -> Result<&str> {
    path.to_str()
        .with_context(|| format!("path is not valid UTF-8: {}", path.display()))
}

fn expect_one(changed: usize, what: &str, id: i64) -> Result<()> {
    if changed == 0 {
        bail!("no {what} with id {id}");
    }
    Ok(())
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}
