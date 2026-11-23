//! Each account's newest usage readings, kept by the `usage_reading_insert` trigger as agent events arrive.

use anyhow::Result;

use crate::Store;

/// One stored usage reading: an agent `RateLimit` event's payload.
#[derive(Debug, Clone, PartialEq)]
pub struct UsageReading {
    /// `""` for the newest reading of any kind, else the window it measures (`five_hour`, `seven_day`, …).
    pub window: String,
    pub payload_json: String,
    pub created_at: i64,
}

impl Store {
    /// The newest reading per window from threads created under `account_id` (a primary-key lookup).
    pub fn usage_readings(&self, account_id: &str) -> Result<Vec<UsageReading>> {
        let mut stmt = self.conn.prepare(
            "SELECT window, payload_json, created_at FROM usage_readings WHERE account_id = ?1",
        )?;
        let rows = stmt.query_map([account_id], |r| {
            Ok(UsageReading {
                window: r.get(0)?,
                payload_json: r.get(1)?,
                created_at: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}
