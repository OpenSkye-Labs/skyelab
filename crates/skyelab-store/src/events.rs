//! Thread events: append (image bytes dropped, see `images.rs`) and read back.

use anyhow::Result;
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};

use crate::{Event, Store, expect_one, images, now_ms};

impl Store {
    /// Called with every event after it is stored, under the caller's lock, so it must not block.
    pub fn on_append(&mut self, hook: impl Fn(&Event) + Send + 'static) {
        self.on_append = Some(Box::new(hook));
    }

    /// Stores an event under the thread's next seq and bumps the thread's `updated_at`. Image bytes in the payload
    /// are stored as a placeholder (see `images.rs`); the returned event is what was stored.
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    #[tracing::instrument(name = "store.append_event", skip_all)]
    pub fn append_event(
        &mut self,
        thread_id: i64,
        kind: &str,
        payload_json: &str,
    ) -> Result<Event> {
        let slim = images::omit_images(payload_json);
        let payload_json = slim.as_deref().unwrap_or(payload_json);
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
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    #[tracing::instrument(name = "store.events_since", skip_all)]
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

    /// The model the account's newest run with no model pick named in its init line: what its default resolves to.
    // shortcut: scans the account's agent events (off the UI thread); index init models if stores grow huge.
    pub fn last_default_model(&self, account_id: &str) -> Result<Option<String>> {
        let model = self
            .conn
            .query_row(
                "SELECT json_extract(e.payload_json, '$.Init.model') AS m FROM events e
                 JOIN threads t ON t.id = e.thread_id
                 WHERE t.account_id = ?1 AND t.model IS NULL AND e.kind = 'agent'
                   AND instr(e.payload_json, '\"Init\"') > 0 AND json_valid(e.payload_json) AND m <> ''
                 ORDER BY e.created_at DESC, e.seq DESC LIMIT 1",
                [account_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(model)
    }
}
