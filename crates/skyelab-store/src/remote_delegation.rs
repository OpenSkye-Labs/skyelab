//! Delegation across machines. On the serving machine, `delegated_from` says which caller on which machine started
//! a thread; on the calling machine, `remote_children` lists the threads a local thread started elsewhere. Each
//! store has a random `store_id`, so a caller whose thread ids restarted (a fresh data dir) reaches nothing old.

use crate::{Store, now_ms};
use anyhow::{Context as _, Result};
use rusqlite::{OptionalExtension as _, params};

/// Version 29. `IF NOT EXISTS` and `OR IGNORE`: migration tests rewind `user_version` without dropping later tables.
pub(crate) const MIGRATION_V29: &str = "
CREATE TABLE IF NOT EXISTS delegated_from (
    thread_id        INTEGER PRIMARY KEY REFERENCES threads(id) ON DELETE CASCADE,
    origin_node      TEXT NOT NULL,
    origin_store     TEXT NOT NULL,
    origin_machine   TEXT NOT NULL,
    parent_thread_id INTEGER NOT NULL,
    parent_title     TEXT NOT NULL,
    origin_account   TEXT NOT NULL,
    depth            INTEGER NOT NULL,
    request_id       TEXT
);
CREATE UNIQUE INDEX IF NOT EXISTS delegated_from_request
    ON delegated_from(origin_node, origin_store, parent_thread_id, request_id) WHERE request_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS delegated_from_parent ON delegated_from(origin_node, origin_store, parent_thread_id);
CREATE TABLE IF NOT EXISTS store_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
INSERT OR IGNORE INTO store_meta VALUES ('store_id', lower(hex(randomblob(16))));
CREATE TABLE IF NOT EXISTS remote_children (
    parent_thread_id INTEGER NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    machine          TEXT NOT NULL,
    node_id          TEXT NOT NULL,
    thread_id        INTEGER NOT NULL,
    title            TEXT NOT NULL DEFAULT '',
    created_at       INTEGER NOT NULL,
    PRIMARY KEY (parent_thread_id, node_id, thread_id)
);
";

/// Where a thread started by another machine's agent came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelegatedOrigin {
    /// The calling machine's Tailscale StableID, from whois.
    pub origin_node: String,
    /// The calling machine's store id.
    pub origin_store: String,
    /// The calling machine's MagicDNS name.
    pub origin_machine: String,
    /// The calling thread's id there.
    pub parent_thread_id: i64,
    pub parent_title: String,
    /// The caller's account id there.
    pub origin_account: String,
    /// This thread's delegation depth.
    pub depth: u32,
    pub request_id: Option<String>,
}

/// A thread a local thread started on another machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteChild {
    pub parent_thread_id: i64,
    pub machine: String,
    pub node_id: String,
    /// Its id on that machine.
    pub thread_id: i64,
    pub title: String,
    pub created_at: i64,
}

const ORIGIN_COLS: &str = "origin_node, origin_store, origin_machine, parent_thread_id, parent_title, \
                           origin_account, depth, request_id";

fn origin_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<DelegatedOrigin> {
    Ok(DelegatedOrigin {
        origin_node: r.get(0)?,
        origin_store: r.get(1)?,
        origin_machine: r.get(2)?,
        parent_thread_id: r.get(3)?,
        parent_title: r.get(4)?,
        origin_account: r.get(5)?,
        depth: r.get(6)?,
        request_id: r.get(7)?,
    })
}

impl Store {
    /// This store's random id, made by the v29 migration.
    pub fn store_id(&self) -> Result<String> {
        self.conn
            .query_row(
                "SELECT value FROM store_meta WHERE key = 'store_id'",
                [],
                |r| r.get(0),
            )
            .context("reading the store id")
    }

    /// Records where `thread_id` came from. Fails if the caller already has a thread under that request id.
    pub fn set_delegated_from(&self, thread_id: i64, o: &DelegatedOrigin) -> Result<()> {
        self.conn
            .execute(
                &format!("INSERT INTO delegated_from (thread_id, {ORIGIN_COLS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"),
                params![
                    thread_id,
                    o.origin_node,
                    o.origin_store,
                    o.origin_machine,
                    o.parent_thread_id,
                    o.parent_title,
                    o.origin_account,
                    o.depth,
                    o.request_id
                ],
            )
            .context("recording where the delegated thread came from")?;
        Ok(())
    }

    /// Where `thread_id` came from, if another machine's agent started it.
    pub fn delegated_from(&self, thread_id: i64) -> Result<Option<DelegatedOrigin>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {ORIGIN_COLS} FROM delegated_from WHERE thread_id = ?1"),
                [thread_id],
                origin_from_row,
            )
            .optional()?)
    }

    /// The thread this caller already started under `request_id`, if any.
    pub fn delegated_by_request(
        &self,
        node: &str,
        store: &str,
        parent: i64,
        request_id: &str,
    ) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row(
                "SELECT thread_id FROM delegated_from WHERE origin_node = ?1 AND origin_store = ?2 \
                 AND parent_thread_id = ?3 AND request_id = ?4",
                params![node, store, parent, request_id],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// `(id, status)` of the threads delegated from `node`: only those of one caller (`store`, parent id) when given.
    pub fn delegated_statuses(
        &self,
        node: &str,
        caller: Option<(&str, i64)>,
    ) -> Result<Vec<(i64, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.id, t.status FROM delegated_from d JOIN threads t ON t.id = d.thread_id \
             WHERE d.origin_node = ?1 AND (?2 IS NULL OR (d.origin_store = ?2 AND d.parent_thread_id = ?3)) \
             ORDER BY t.id",
        )?;
        let (store, parent) = caller.unzip();
        let rows = stmt.query_map(params![node, store, parent], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Records a thread `parent` started on another machine.
    pub fn add_remote_child(&self, c: &RemoteChild) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO remote_children (parent_thread_id, machine, node_id, thread_id, title, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![c.parent_thread_id, c.machine, c.node_id, c.thread_id, c.title, now_ms()],
            )
            .context("recording the thread started on another machine")?;
        Ok(())
    }

    /// The threads `parent` started on other machines, oldest first.
    pub fn remote_children(&self, parent: i64) -> Result<Vec<RemoteChild>> {
        let mut stmt = self.conn.prepare(
            "SELECT parent_thread_id, machine, node_id, thread_id, title, created_at FROM remote_children \
             WHERE parent_thread_id = ?1 ORDER BY created_at, thread_id",
        )?;
        let rows = stmt.query_map([parent], |r| {
            Ok(RemoteChild {
                parent_thread_id: r.get(0)?,
                machine: r.get(1)?,
                node_id: r.get(2)?,
                thread_id: r.get(3)?,
                title: r.get(4)?,
                created_at: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NewThread;

    fn thread(store: &Store, project: &crate::Project) -> i64 {
        store
            .create_thread(&NewThread {
                project_id: project.id,
                title: "t",
                cwd: &project.path,
                account_id: "work",
                status: "running",
            })
            .expect("thread")
            .id
    }

    fn origin(parent: i64, request: Option<&str>) -> DelegatedOrigin {
        DelegatedOrigin {
            origin_node: "nA".into(),
            origin_store: "s".into(),
            origin_machine: "a.example.ts.net".into(),
            parent_thread_id: parent,
            parent_title: "parent".into(),
            origin_account: "x".into(),
            depth: 1,
            request_id: request.map(str::to_owned),
        }
    }

    #[test]
    fn the_request_index_refuses_a_second_row_and_counts_scope_by_caller() {
        let store = Store::open_in_memory().expect("store");
        let p = store
            .create_project("p", std::path::Path::new("/w/p"))
            .expect("project");
        let (a, b, c) = (thread(&store, &p), thread(&store, &p), thread(&store, &p));
        store
            .set_delegated_from(a, &origin(7, Some("r1")))
            .expect("first");
        assert!(
            store.set_delegated_from(b, &origin(7, Some("r1"))).is_err(),
            "same caller, same request"
        );
        store
            .set_delegated_from(b, &origin(8, Some("r1")))
            .expect("another parent may reuse it");
        store
            .set_delegated_from(c, &origin(7, None))
            .expect("no request id");
        assert_eq!(
            store
                .delegated_by_request("nA", "s", 7, "r1")
                .expect("read"),
            Some(a)
        );
        assert_eq!(
            store
                .delegated_by_request("nA", "other", 7, "r1")
                .expect("read"),
            None
        );
        assert_eq!(
            store
                .delegated_statuses("nA", Some(("s", 7)))
                .expect("read")
                .len(),
            2
        );
        assert_eq!(store.delegated_statuses("nA", None).expect("read").len(), 3);
        assert!(
            store
                .delegated_statuses("nB", None)
                .expect("read")
                .is_empty()
        );
        assert_eq!(
            store.delegated_from(a).expect("read"),
            Some(origin(7, Some("r1")))
        );
        store.delete_thread(a).expect("delete");
        assert_eq!(
            store.delegated_from(a).expect("read"),
            None,
            "goes with its thread"
        );
        assert_eq!(store.store_id().expect("id").len(), 32);
    }
}
