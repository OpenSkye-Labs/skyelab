//! T3's settled and snoozed threads. Settling clears a snooze and snoozing clears the settle: a thread
//! is in one shelf at a time. Neither is a thread update, so `updated_at` (the sidebar's order) stays.

use anyhow::Result;
use rusqlite::params;

use crate::{Store, expect_one, now_ms};

impl Store {
    /// Settles thread `id` (`false` un-settles it).
    pub fn set_thread_settled(&self, id: i64, settled: bool) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE threads SET settled_at = ?2,
                 snoozed_until = CASE WHEN ?2 IS NULL THEN snoozed_until END
             WHERE id = ?1",
            params![id, settled.then(now_ms)],
        )?;
        expect_one(n, "thread", id)
    }

    /// Snoozes thread `id` until `until` (ms since the epoch); `None` wakes it (and clears its woke mark).
    pub fn set_thread_snoozed(&self, id: i64, until: Option<i64>) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE threads SET snoozed_until = ?2,
                 settled_at = CASE WHEN ?2 IS NULL THEN settled_at END
             WHERE id = ?1",
            params![id, until],
        )?;
        expect_one(n, "thread", id)
    }

    /// Clears a passed snooze (the thread was looked at); a newer snooze set since is kept.
    pub fn clear_woke(&self, id: i64, now_ms: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE threads SET snoozed_until = NULL WHERE id = ?1 AND snoozed_until <= ?2",
            params![id, now_ms],
        )?;
        Ok(())
    }
}
