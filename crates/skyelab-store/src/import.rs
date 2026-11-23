//! Threads made from existing agent sessions: one per session per project, never two.

use crate::{NewThread, Store, THREAD_COLS, Thread, images, path_str, thread_from_row};
use anyhow::{Context as _, Result};
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};
use std::collections::HashSet;

impl Store {
    /// Agent session ids that already have a thread in project `id` (imported, or started here).
    pub fn session_ids(&self, project_id: i64) -> Result<HashSet<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT agent_session_id FROM threads WHERE project_id = ?1 AND agent_session_id IS NOT NULL",
        )?;
        let rows = stmt.query_map([project_id], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Creates a thread that continues agent session `session_id`, with `events` (`(kind, payload_json)`) as its
    /// history, all dated `at`. `None`, writing nothing, when the project already has a thread for that session.
    pub fn import_thread(
        &mut self,
        new: &NewThread,
        session_id: &str,
        at: i64,
        events: &[(&str, String)],
    ) -> Result<Option<Thread>> {
        // IMMEDIATE: two imports of one session can't both see it missing.
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let taken = tx
            .query_row(
                "SELECT 1 FROM threads WHERE project_id = ?1 AND agent_session_id = ?2",
                params![new.project_id, session_id],
                |_| Ok(()),
            )
            .optional()?;
        if taken.is_some() {
            return Ok(None);
        }
        tx.execute(
            "INSERT INTO threads (project_id, title, cwd, account_id, status, created_at, updated_at, title_source,
                                  agent_session_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, 'import', ?7)",
            params![
                new.project_id,
                new.title,
                path_str(new.cwd)?,
                new.account_id,
                new.status,
                at,
                session_id
            ],
        )
        .with_context(|| format!("import session {session_id}"))?;
        let id = tx.last_insert_rowid();
        for (seq, (kind, payload)) in events.iter().enumerate() {
            // Image bytes become a placeholder, as `append_event` stores them.
            let slim = images::omit_images(payload);
            let payload = slim.as_deref().unwrap_or(payload);
            tx.execute(
                "INSERT INTO events (thread_id, seq, kind, payload_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, seq as i64 + 1, kind, payload, at],
            )?;
        }
        let sql = format!("SELECT {THREAD_COLS} FROM threads WHERE id = ?1");
        let thread = tx.query_row(&sql, [id], thread_from_row)?;
        tx.commit()?;
        Ok(Some(thread))
    }
}
