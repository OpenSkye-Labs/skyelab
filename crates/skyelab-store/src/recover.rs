//! Startup recovery on [`Store::open`]: mark threads the previous run left `starting`/`running` as `interrupted`
//! (remembering active local ones so the app can optionally resume them), and purge expired soft-deleted threads.

use anyhow::{Context as _, Result};

use crate::{Store, THREAD_COLS, Thread, thread_from_row};

impl Store {
    /// Recovers stale `starting`/`running` threads and purges soft-deleted rows left from the previous run.
    pub(crate) fn recover_on_open(&mut self) -> Result<()> {
        let cols = THREAD_COLS
            .split(", ")
            .map(|c| format!("t.{c}"))
            .collect::<Vec<_>>()
            .join(", ");
        let active_sql = format!(
            "SELECT {cols} FROM threads t JOIN projects p ON p.id = t.project_id
             WHERE t.status IN ('starting', 'running')
               AND t.deleted_at IS NULL
               AND t.archived_at IS NULL
               AND t.side_of IS NULL
               AND p.removed_at IS NULL
             ORDER BY t.updated_at DESC, t.id DESC"
        );
        let mut recovered: Vec<Thread> = self
            .conn
            .prepare(&active_sql)?
            .query_map([], thread_from_row)?
            .collect::<rusqlite::Result<_>>()
            .context("listing threads interrupted by the last run")?;
        // shortcut: assumes one app per store; a second instance on the same data dir would interrupt the first
        // one's live threads. Upgrade with an owner pid per row or an instance lock if that becomes real.
        self.conn
            .execute(
                "UPDATE threads SET status = 'interrupted' WHERE status IN ('starting', 'running')",
                [],
            )
            .context("marking the last run's threads interrupted")?;
        for thread in &mut recovered {
            thread.status = "interrupted".to_owned();
        }
        self.recovered = recovered;
        // A delete whose undo window the last run didn't see out is final now.
        let purged_sql = format!("SELECT {THREAD_COLS} FROM threads WHERE deleted_at IS NOT NULL");
        self.purged = self
            .conn
            .prepare(&purged_sql)?
            .query_map([], thread_from_row)?
            .collect::<rusqlite::Result<_>>()
            .context("listing deleted threads")?;
        self.conn
            .execute("DELETE FROM threads WHERE deleted_at IS NOT NULL", [])
            .context("purging deleted threads")?;
        Ok(())
    }

    /// The threads the last [`Store::open`] purged (once: later calls get none).
    pub fn take_purged(&mut self) -> Vec<Thread> {
        std::mem::take(&mut self.purged)
    }

    /// The active local threads the last [`Store::open`] recovered from `starting`/`running` to `interrupted`
    /// (once: later calls get none). Threads explicitly stopped by Stop, tab close or clean Quit already hold
    /// `stopped` or `interrupted` and are never included here.
    pub fn take_recovered(&mut self) -> Vec<Thread> {
        std::mem::take(&mut self.recovered)
    }
}
