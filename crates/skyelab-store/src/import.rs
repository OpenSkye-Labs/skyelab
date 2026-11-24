//! Threads made from existing agent sessions: one per session across every project, never two (nested projects
//! can both list a session).

use crate::{NewThread, Store, THREAD_COLS, Thread, images, path_str, thread_from_row};
use anyhow::{Context as _, Result};
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};
use std::collections::HashSet;
use std::path::Path;

impl Store {
    /// Agent session ids that already have a thread in any project: imported (even if resuming changed the thread's
    /// session id since), or started there.
    pub fn session_ids(&self) -> Result<HashSet<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT agent_session_id FROM threads WHERE agent_session_id IS NOT NULL
             UNION SELECT imported_session FROM threads WHERE imported_session IS NOT NULL",
        )?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Whether a thread other than `thread_id` is running agent session `session_id` (starting or running): a second
    /// resume of it would run two agents on one session.
    // shortcut: `idle` (an agent alive between turns) isn't counted, since a crashed run leaves it on the row and
    // `open` doesn't clear it; count it once `open` resets stale idle rows.
    pub fn session_busy_elsewhere(&self, thread_id: i64, session_id: &str) -> Result<bool> {
        let mut stmt = self.conn.prepare(
            "SELECT status FROM threads WHERE id != ?1 AND (agent_session_id = ?2 OR imported_session = ?2)",
        )?;
        let statuses = stmt.query_map(params![thread_id, session_id], |r| r.get::<_, String>(0))?;
        for status in statuses {
            if skyelab_core::delegation::is_busy(&status?) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Whether another thread sharing worktree `path` (a fork or handoff runs in its source's) has an agent starting
    /// or running: removing the worktree would pull the folder out from under it.
    pub fn worktree_busy_elsewhere(&self, thread_id: i64, path: &Path) -> Result<bool> {
        let mut stmt = self.conn.prepare(
            "SELECT status FROM threads WHERE id != ?1 AND worktree_path = ?2 AND deleted_at IS NULL",
        )?;
        let statuses = stmt.query_map(params![thread_id, path_str(path)?], |r| {
            r.get::<_, String>(0)
        })?;
        for status in statuses {
            if skyelab_core::delegation::is_busy(&status?) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Creates a thread that continues agent session `session_id`, with `events` (`(kind, payload_json, at_ms)`) as
    /// its history; the thread starts at its first event and was last updated `at`. `None`, writing nothing, when
    /// any project already has a thread for that session.
    pub fn import_thread(
        &mut self,
        new: &NewThread,
        session_id: &str,
        at: i64,
        events: &[(&str, String, i64)],
    ) -> Result<Option<Thread>> {
        // IMMEDIATE: two imports of one session can't both see it missing.
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let taken = tx
            .query_row(
                "SELECT 1 FROM threads WHERE agent_session_id = ?1 OR imported_session = ?1",
                params![session_id],
                |_| Ok(()),
            )
            .optional()?;
        if taken.is_some() {
            return Ok(None);
        }
        // The usage trigger counts only events after the thread's start, so the row starts after the history while it
        // goes in, then moves back to the first event: imported turns are not new usage.
        let after = events.iter().map(|e| e.2).fold(at, i64::max);
        tx.execute(
            "INSERT INTO threads (project_id, title, cwd, account_id, status, created_at, updated_at, title_source,
                                  agent_session_id, imported_session)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'import', ?8, ?8)",
            params![
                new.project_id,
                new.title,
                path_str(new.cwd)?,
                new.account_id,
                new.status,
                after,
                at,
                session_id
            ],
        )
        .with_context(|| format!("import session {session_id}"))?;
        let id = tx.last_insert_rowid();
        for (seq, (kind, payload, created)) in events.iter().enumerate() {
            // Image bytes become a placeholder, as `append_event` stores them.
            let slim = images::omit_images(payload);
            let payload = slim.as_deref().unwrap_or(payload);
            tx.execute(
                "INSERT INTO events (thread_id, seq, kind, payload_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, seq as i64 + 1, kind, payload, created],
            )?;
        }
        tx.execute(
            "UPDATE threads SET created_at = ?2 WHERE id = ?1",
            params![id, events.first().map_or(at, |e| e.2)],
        )?;
        let sql = format!("SELECT {THREAD_COLS} FROM threads WHERE id = ?1");
        let thread = tx.query_row(&sql, [id], thread_from_row)?;
        tx.commit()?;
        Ok(Some(thread))
    }
}
