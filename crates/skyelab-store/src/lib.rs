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

mod manage;
mod model;
pub use model::ModelChoice;
mod migrations;
use migrations::MIGRATIONS;

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
}

/// A command a project keeps for its terminals: a "Run" entry, and with `setup`, run in each new worktree.
/// Kept here, never in the repo, so an agent can't plant one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectScript {
    pub id: i64,
    pub project_id: i64,
    pub name: String,
    pub command: String,
    /// Run in every new worktree of the project (installing dependencies, say).
    pub setup: bool,
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
    /// The threads [`Store::open`] purged, until [`Store::take_purged`] hands them over (their git checkpoints go too).
    purged: Vec<Thread>,
}

const PROJECT_COLS: &str = "id, name, path, created_at";
const THREAD_COLS: &str = "id, project_id, title, cwd, account_id, agent_session_id, status, created_at, updated_at, worktree_path, worktree_branch, worktree_base, pr_url, parent_thread_id, permission_mode, pinned_at, archived_at, unread";

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
        tx.pragma_update(None, "user_version", latest)?;
        tx.commit()?;
        Ok(Self {
            conn,
            on_append: None,
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
        })
    }

    pub fn thread(&self, id: i64) -> Result<Option<Thread>> {
        let sql = format!("SELECT {THREAD_COLS} FROM threads WHERE id = ?1");
        Ok(self
            .conn
            .query_row(&sql, [id], thread_from_row)
            .optional()?)
    }

    /// Most recently updated first, archived ones included. Deleted ones (still in their undo window) are left out.
    pub fn list_threads(&self, project_id: i64) -> Result<Vec<Thread>> {
        let sql = format!(
            "SELECT {THREAD_COLS} FROM threads WHERE project_id = ?1 AND deleted_at IS NULL
             ORDER BY updated_at DESC, id DESC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([project_id], thread_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Every listed project's threads, most recently updated first, archived ones included. Deleted threads and
    /// removed projects are left out.
    pub fn list_all_threads(&self) -> Result<Vec<Thread>> {
        let cols = THREAD_COLS
            .split(", ")
            .map(|c| format!("t.{c}"))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT {cols} FROM threads t JOIN projects p ON p.id = t.project_id
             WHERE t.deleted_at IS NULL AND p.removed_at IS NULL
             ORDER BY t.updated_at DESC, t.id DESC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], thread_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn set_thread_title(&self, id: i64, title: &str) -> Result<()> {
        self.update_thread(id, "title", title)
    }

    pub fn set_thread_status(&self, id: i64, status: &str) -> Result<()> {
        self.update_thread(id, "status", status)
    }

    pub fn set_agent_session_id(&self, id: i64, session_id: Option<&str>) -> Result<()> {
        self.update_thread(id, "agent_session_id", session_id)
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

    pub fn set_thread_pr_url(&self, id: i64, url: Option<&str>) -> Result<()> {
        self.update_thread(id, "pr_url", url)
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

    /// Called with every event after it is stored, under the caller's lock, so it must not block.
    pub fn on_append(&mut self, hook: impl Fn(&Event) + Send + 'static) {
        self.on_append = Some(Box::new(hook));
    }

    /// Stores an event under the thread's next seq and bumps the thread's `updated_at`.
    pub fn append_event(
        &mut self,
        thread_id: i64,
        kind: &str,
        payload_json: &str,
    ) -> Result<Event> {
        let now = now_ms();
        // IMMEDIATE takes the write lock before reading MAX(seq), so another connection can't claim the same seq.
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let n = tx.execute(
            "UPDATE threads SET updated_at = ?2 WHERE id = ?1",
            params![thread_id, now],
        )?;
        expect_one(n, "thread", thread_id)?;
        let seq: i64 = tx.query_row(
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM events WHERE thread_id = ?1",
            [thread_id],
            |r| r.get(0),
        )?;
        tx.execute(
            "INSERT INTO events (thread_id, seq, kind, payload_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![thread_id, seq, kind, payload_json, now],
        )?;
        tx.commit()?;
        let event = Event {
            thread_id,
            seq,
            kind: kind.to_owned(),
            payload_json: payload_json.to_owned(),
            created_at: now,
        };
        if let Some(hook) = &self.on_append {
            hook(&event);
        }
        Ok(event)
    }

    /// Events with `seq > after_seq`, in order. Pass 0 for all of them.
    pub fn events_since(&self, thread_id: i64, after_seq: i64) -> Result<Vec<Event>> {
        let mut stmt = self.conn.prepare(
            "SELECT thread_id, seq, kind, payload_json, created_at FROM events
             WHERE thread_id = ?1 AND seq > ?2 ORDER BY seq",
        )?;
        let rows = stmt.query_map(params![thread_id, after_seq], |r| {
            Ok(Event {
                thread_id: r.get(0)?,
                seq: r.get(1)?,
                kind: r.get(2)?,
                payload_json: r.get(3)?,
                created_at: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
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

fn thread_from_row(r: &Row) -> rusqlite::Result<Thread> {
    Ok(Thread {
        id: r.get(0)?,
        project_id: r.get(1)?,
        title: r.get(2)?,
        cwd: PathBuf::from(r.get::<_, String>(3)?),
        account_id: r.get(4)?,
        agent_session_id: r.get(5)?,
        status: r.get(6)?,
        created_at: r.get(7)?,
        updated_at: r.get(8)?,
        worktree_path: r.get::<_, Option<String>>(9)?.map(PathBuf::from),
        worktree_branch: r.get(10)?,
        worktree_base: r.get(11)?,
        pr_url: r.get(12)?,
        parent_thread_id: r.get(13)?,
        permission_mode: r.get(14)?,
        pinned_at: r.get(15)?,
        archived_at: r.get(16)?,
        unread: r.get(17)?,
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
