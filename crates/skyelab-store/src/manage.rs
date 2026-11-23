//! Pin, archive, unread, undoable delete, project removal, and thread search (schema v7).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use rusqlite::{OptionalExtension, params};

use crate::{
    PROJECT_COLS, Project, Store, THREAD_COLS, Thread, expect_one, now_ms, path_str,
    project_from_row, thread_from_row,
};

/// Bookkeeping columns: setting them never bumps `updated_at`, so a pin or a read never reorders threads.
const PINNED: &str = "pinned_at";
const ARCHIVED: &str = "archived_at";
const DELETED: &str = "deleted_at";

impl Store {
    /// Hides a project (or brings it back). Its threads and their audit stay; nothing on disk is touched.
    pub fn set_project_removed(&self, id: i64, removed: bool) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE projects SET removed_at = ?2 WHERE id = ?1",
            params![id, removed.then(now_ms)],
        )?;
        expect_one(n, "project", id)
    }

    /// The project stored at `path`, brought back if it was removed. `None` when no project has that path.
    pub fn readd_project(&self, path: &Path) -> Result<Option<Project>> {
        let path: PathBuf = path.components().collect();
        let sql = format!(
            "UPDATE projects SET removed_at = NULL WHERE path = ?1 RETURNING {PROJECT_COLS}"
        );
        Ok(self
            .conn
            .query_row(&sql, [path_str(&path)?], project_from_row)
            .optional()?)
    }

    pub fn set_thread_pinned(&self, id: i64, pinned: bool) -> Result<()> {
        self.mark_thread(id, PINNED, pinned)
    }

    pub fn set_thread_archived(&self, id: i64, archived: bool) -> Result<()> {
        self.mark_thread(id, ARCHIVED, archived)
    }

    /// Hides the thread until [`Self::purge_thread`] (or the next open) deletes it; `false` undoes that.
    pub fn set_thread_deleted(&self, id: i64, deleted: bool) -> Result<()> {
        self.mark_thread(id, DELETED, deleted)
    }

    pub fn set_thread_unread(&self, id: i64, unread: bool) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE threads SET unread = ?2 WHERE id = ?1",
            params![id, unread],
        )?;
        expect_one(n, "thread", id)
    }

    /// The thread, unless it is deleted (in its undo window): what may be opened.
    pub fn listed_thread(&self, id: i64) -> Result<Option<Thread>> {
        let sql = format!("SELECT {THREAD_COLS} FROM threads WHERE id = ?1 AND deleted_at IS NULL");
        Ok(self
            .conn
            .query_row(&sql, [id], thread_from_row)
            .optional()?)
    }

    /// Deletes the thread if it is still marked deleted (its undo was not used). Otherwise does nothing.
    /// Returns the purged row (`None`: not a deleted thread, so nothing was purged).
    pub fn purge_thread(&self, id: i64) -> Result<Option<Thread>> {
        let sql =
            format!("SELECT {THREAD_COLS} FROM threads WHERE id = ?1 AND deleted_at IS NOT NULL");
        let row = self
            .conn
            .query_row(&sql, [id], crate::thread_from_row)
            .optional()?;
        self.conn.execute(
            "DELETE FROM threads WHERE id = ?1 AND deleted_at IS NOT NULL",
            [id],
        )?;
        Ok(row)
    }

    fn mark_thread(&self, id: i64, column: &'static str, on: bool) -> Result<()> {
        let sql = format!("UPDATE threads SET {column} = ?2 WHERE id = ?1");
        let n = self.conn.execute(&sql, params![id, on.then(now_ms)])?;
        expect_one(n, "thread", id)
    }

    /// Each listed thread's latest assistant text (its first 400 characters), for the sidebar's preview line, in
    /// `project` or in every project. One query: per thread it walks `events` backwards on the (thread_id, seq) key
    /// and stops at the first text block.
    pub fn last_replies(&self, project: Option<i64>) -> Result<HashMap<i64, String>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.id, (
                SELECT (SELECT substr(json_extract(b.value, '$.text'), 1, 400)
                        FROM json_each(
                            CASE WHEN json_valid(e.payload_json) THEN e.payload_json ELSE '{}' END,
                            '$.Assistant.content') b
                        WHERE json_extract(b.value, '$.type') = 'text'
                          AND trim(json_extract(b.value, '$.text')) <> ''
                        ORDER BY b.key DESC LIMIT 1) AS reply
                FROM events e
                WHERE e.thread_id = t.id AND e.kind = 'agent' AND reply IS NOT NULL
                ORDER BY e.seq DESC LIMIT 1)
             FROM threads t WHERE t.deleted_at IS NULL AND (?1 IS NULL OR t.project_id = ?1)",
        )?;
        let rows = stmt.query_map([project], |r| {
            Ok((r.get(0)?, r.get::<_, Option<String>>(1)?))
        })?;
        let mut out = HashMap::new();
        for row in rows {
            if let (id, Some(text)) = row? {
                out.insert(id, text);
            }
        }
        Ok(out)
    }

    /// Threads whose title or message text (prompts, assistant text) contains `query`, most recently updated
    /// first, in `project` or in every project. Deleted threads and removed projects are left out. Under three
    /// characters only titles are searched: the trigram index needs three.
    pub fn search_threads(
        &self,
        query: &str,
        project: Option<i64>,
        limit: usize,
    ) -> Result<Vec<Thread>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let like = format!(
            "%{}%",
            query
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        // A quoted FTS5 phrase: the query's text, never its syntax.
        let phrase =
            (query.chars().count() >= 3).then(|| format!("\"{}\"", query.replace('"', "\"\"")));
        let cols = THREAD_COLS
            .split(", ")
            .map(|c| format!("t.{c}"))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT {cols} FROM threads t JOIN projects p ON p.id = t.project_id
             WHERE t.deleted_at IS NULL AND p.removed_at IS NULL AND (?2 IS NULL OR t.project_id = ?2)
               AND (t.title LIKE ?1 ESCAPE '\\'
                    OR (?3 IS NOT NULL AND t.id IN (SELECT thread_id FROM event_text WHERE event_text MATCH ?3)))
             ORDER BY t.updated_at DESC, t.id DESC LIMIT ?4"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let rows = stmt.query_map(params![like, project, phrase, limit], thread_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}
