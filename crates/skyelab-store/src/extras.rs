//! Generated titles, composer drafts, forks and side chats.

use anyhow::{Context as _, Result};
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};

use crate::{Event, NewThread, Store, THREAD_COLS, Thread, now_ms, path_str, thread_from_row};

/// A composer's unsent text and files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Draft {
    pub text: String,
    /// (name, bytes), in the composer's order.
    pub files: Vec<(String, Vec<u8>)>,
}

impl Draft {
    pub fn is_empty(&self) -> bool {
        self.text.is_empty() && self.files.is_empty()
    }
}

/// A listed draft: its text and how many files it holds (their bytes stay in the store).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftRow {
    pub owner: String,
    pub text: String,
    pub updated_at: i64,
    pub files: usize,
}

impl Store {
    /// Who set the thread's title: `prompt` (its first line), `generated`, `user` or `fork`.
    pub fn title_source(&self, id: i64) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT title_source FROM threads WHERE id = ?1",
                [id],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Stores a generated title. Without `force` it only replaces a first-line title, so a user's rename made while
    /// the title was generated stays. `false`: nothing changed. Not an update the sidebar orders by.
    pub fn set_generated_title(&self, id: i64, title: &str, force: bool) -> Result<bool> {
        let n = self.conn.execute(
            "UPDATE threads SET title = ?2, title_source = 'generated'
             WHERE id = ?1 AND (?3 OR title_source = 'prompt')",
            params![id, title, force],
        )?;
        Ok(n == 1)
    }

    /// Where the reader left thread `id` scrolled up: (row, offset in the row in px). `None`: at its end.
    pub fn scroll_mark(&self, id: i64) -> Result<Option<(usize, f32)>> {
        let mark: Option<String> = self
            .conn
            .query_row("SELECT scroll_mark FROM threads WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .optional()?
            .flatten();
        Ok(mark.and_then(|m| {
            let (row, offset) = m.split_once(':')?;
            Some((row.parse().ok()?, offset.parse().ok()?))
        }))
    }

    /// Keeps where the reader left thread `id` (`None`: at its end). Not an update the sidebar orders by.
    pub fn set_scroll_mark(&self, id: i64, mark: Option<(usize, f32)>) -> Result<()> {
        let mark = mark.map(|(row, offset)| format!("{row}:{offset}"));
        self.conn.execute(
            "UPDATE threads SET scroll_mark = ?2 WHERE id = ?1",
            params![id, mark],
        )?;
        Ok(())
    }

    /// The draft `owner` (`thread:<id>` or `project:<id>`) holds; empty when none.
    pub fn draft(&self, owner: &str) -> Result<Draft> {
        let text = self
            .conn
            .query_row("SELECT text FROM drafts WHERE owner = ?1", [owner], |r| {
                r.get(0)
            })
            .optional()?
            .unwrap_or_default();
        let mut stmt = self
            .conn
            .prepare("SELECT name, data FROM draft_files WHERE owner = ?1 ORDER BY idx")?;
        let files = stmt
            .query_map([owner], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(Draft { text, files })
    }

    /// Keeps `owner`'s unsent text (empty removes it).
    pub fn set_draft_text(&self, owner: &str, text: &str) -> Result<()> {
        if text.is_empty() {
            self.conn
                .execute("DELETE FROM drafts WHERE owner = ?1", [owner])?;
        } else {
            self.conn.execute(
                "INSERT OR REPLACE INTO drafts (owner, text, updated_at) VALUES (?1, ?2, ?3)",
                params![owner, text, now_ms()],
            )?;
        }
        Ok(())
    }

    /// The drafts whose owner starts with `prefix` (the prompt stash's `stash:`), newest first.
    pub fn drafts_with_prefix(&self, prefix: &str) -> Result<Vec<DraftRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT d.owner, d.text, d.updated_at,
                    (SELECT COUNT(*) FROM draft_files f WHERE f.owner = d.owner)
             FROM drafts d WHERE substr(d.owner, 1, length(?1)) = ?1
             ORDER BY d.updated_at DESC, d.owner DESC",
        )?;
        let rows = stmt.query_map([prefix], |r| {
            Ok(DraftRow {
                owner: r.get(0)?,
                text: r.get(1)?,
                updated_at: r.get(2)?,
                files: r.get::<_, i64>(3)? as usize,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Reads `owner`'s draft and removes it, text and files, in one transaction.
    pub fn take_draft(&mut self, owner: &str) -> Result<Draft> {
        let tx = self.conn.transaction()?;
        let text = tx
            .query_row("SELECT text FROM drafts WHERE owner = ?1", [owner], |r| {
                r.get(0)
            })
            .optional()?
            .unwrap_or_default();
        let files = {
            let mut stmt =
                tx.prepare("SELECT name, data FROM draft_files WHERE owner = ?1 ORDER BY idx")?;
            stmt.query_map([owner], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?
        };
        tx.execute("DELETE FROM drafts WHERE owner = ?1", [owner])?;
        tx.execute("DELETE FROM draft_files WHERE owner = ?1", [owner])?;
        tx.commit()?;
        Ok(Draft { text, files })
    }

    /// Removes `owner`'s draft, text and files.
    pub fn delete_draft(&mut self, owner: &str) -> Result<()> {
        self.take_draft(owner).map(drop)
    }

    /// Replaces `owner`'s unsent files.
    pub fn set_draft_files(&mut self, owner: &str, files: &[(String, Vec<u8>)]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM draft_files WHERE owner = ?1", [owner])?;
        for (i, (name, data)) in files.iter().enumerate() {
            tx.execute(
                "INSERT INTO draft_files (owner, idx, name, data) VALUES (?1, ?2, ?3, ?4)",
                params![owner, i as i64, name, data],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// The prompt a fork's first session starts with, until it has a session of its own.
    pub fn fork_context(&self, id: i64) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT fork_context FROM threads WHERE id = ?1",
                [id],
                |r| r.get(0),
            )
            .optional()?
            .flatten())
    }

    /// Creates a fork of thread `from`: a new `stopped` row with `events` (the part of `from`'s history it keeps) copied
    /// in order, renumbered from 1, their times kept. Usage readings are not copied: they would replace the account's
    /// newer ones. Made in `from`'s worktree (`new.cwd` is it), it shares that worktree's columns. One transaction, so a
    /// failure leaves no half-copied thread.
    pub fn create_fork(
        &mut self,
        new: &NewThread,
        from: i64,
        context: &str,
        events: &[Event],
    ) -> Result<Thread> {
        let now = now_ms();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO threads (project_id, title, cwd, account_id, status, created_at, updated_at, title_source,
                                  forked_from, fork_context)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, 'fork', ?7, ?8)",
            params![
                new.project_id,
                new.title,
                path_str(new.cwd)?,
                new.account_id,
                new.status,
                now,
                from,
                context
            ],
        )
        .with_context(|| format!("fork thread {from}"))?;
        let id = tx.last_insert_rowid();
        // Started in its source's worktree: it shares that worktree, branch and base (T3's fork keeps them too).
        tx.execute(
            "UPDATE threads SET (worktree_path, worktree_branch, worktree_base) =
                 (SELECT worktree_path, worktree_branch, worktree_base FROM threads WHERE id = ?2)
             WHERE id = ?1 AND cwd = (SELECT worktree_path FROM threads WHERE id = ?2)",
            params![id, from],
        )?;
        let mut seq = 0;
        for e in events.iter().filter(|e| !is_usage(e)) {
            seq += 1;
            tx.execute(
                "INSERT INTO events (thread_id, seq, kind, payload_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, seq, e.kind, e.payload_json, e.created_at],
            )?;
        }
        let sql = format!("SELECT {THREAD_COLS} FROM threads WHERE id = ?1");
        let thread = tx.query_row(&sql, [id], thread_from_row)?;
        tx.commit()?;
        Ok(thread)
    }
}

impl Store {
    /// A side chat of thread `parent`: a new row with `new`'s folder and account, in Plan mode, no copied history, and
    /// `context` (a part of the parent's conversation) for its first session. Its title is kept: no title is generated for it.
    pub fn create_side_chat(
        &mut self,
        new: &NewThread,
        parent: i64,
        context: &str,
    ) -> Result<Thread> {
        self.conn
            .execute(
                "INSERT INTO threads (project_id, title, cwd, account_id, status, created_at, updated_at, side_of,
                                      fork_context, title_source, permission_mode)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?7, ?8, 'side', 'plan')",
                params![
                    new.project_id,
                    new.title,
                    path_str(new.cwd)?,
                    new.account_id,
                    new.status,
                    now_ms(),
                    parent,
                    context
                ],
            )
            .with_context(|| format!("create a side chat of thread {parent}"))?;
        let id = self.conn.last_insert_rowid();
        self.thread(id)?.context("the new side chat is gone")
    }

    /// Thread `parent`'s side chats, newest first; deleted ones are left out.
    pub fn side_chats(&self, parent: i64) -> Result<Vec<Thread>> {
        let sql = format!(
            "SELECT {THREAD_COLS} FROM threads WHERE side_of = ?1 AND deleted_at IS NULL ORDER BY id DESC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([parent], thread_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

fn is_usage(e: &Event) -> bool {
    e.kind == "agent"
        && e.payload_json.contains("\"RateLimit\"")
        && serde_json::from_str::<serde_json::Value>(&e.payload_json)
            .is_ok_and(|v| v.get("RateLimit").is_some_and(|r| r.is_object()))
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
