//! Threads made from existing agent sessions: one per session across every project, never two (nested projects
//! can both list a session).

use crate::{NewThread, Store, THREAD_COLS, Thread, images, path_str, thread_from_row};
use anyhow::{Context as _, Result};
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};
use std::collections::HashSet;

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

    /// Whether a thread other than `thread_id` holds agent session `session_id` with its agent alive (starting,
    /// running, or idle between turns): a second resume of it would run two agents on one session.
    pub fn session_busy_elsewhere(&self, thread_id: i64, session_id: &str) -> Result<bool> {
        let mut stmt = self.conn.prepare(
            "SELECT status FROM threads WHERE id != ?1 AND (agent_session_id = ?2 OR imported_session = ?2)",
        )?;
        let statuses = stmt.query_map(params![thread_id, session_id], |r| r.get::<_, String>(0))?;
        for status in statuses {
            if skyelab_core::delegation::is_live(&status?) {
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
                events.first().map_or(at, |e| e.2),
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
        let sql = format!("SELECT {THREAD_COLS} FROM threads WHERE id = ?1");
        let thread = tx.query_row(&sql, [id], thread_from_row)?;
        tx.commit()?;
        Ok(Some(thread))
    }
}
