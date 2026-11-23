//! The newest stored events of one kind across an account's threads, for its usage readings.

use anyhow::Result;
use rusqlite::params;

use crate::{Event, Store};

impl Store {
    /// Up to `limit` events of `kind` whose payload contains `needle`, from threads created under `account_id`,
    /// newest first.
    ///
    /// shortcut: a LIKE scan over the account's events; add an indexed column if usage reads get slow.
    pub fn account_events_matching(
        &self,
        account_id: &str,
        kind: &str,
        needle: &str,
        limit: usize,
    ) -> Result<Vec<Event>> {
        let mut stmt = self.conn.prepare(
            "SELECT e.thread_id, e.seq, e.kind, e.payload_json, e.created_at
             FROM events e JOIN threads t ON t.id = e.thread_id
             WHERE t.account_id = ?1 AND e.kind = ?2 AND instr(e.payload_json, ?3) > 0
             ORDER BY e.created_at DESC, e.seq DESC LIMIT ?4",
        )?;
        let rows = stmt.query_map(params![account_id, kind, needle, limit as i64], |r| {
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
