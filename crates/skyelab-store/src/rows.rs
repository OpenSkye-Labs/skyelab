//! Row changes for the shell stream: every write to `projects` or `threads`, from any method, reaches the hooks,
//! because SQLite's update hook fires per row and no caller has to remember to.

use std::sync::{Arc, Mutex, PoisonError};

use anyhow::Result;
use rusqlite::OptionalExtension;

use crate::{PROJECT_COLS, Project, Store, THREAD_COLS, Thread, project_from_row, thread_from_row};

/// A row that was inserted, updated or deleted. Re-read it to see what it is now (it may be gone).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RowChange {
    Project(i64),
    Thread(i64),
}

/// Returns false once it wants no more changes (its receiver is gone); it is dropped then.
pub(crate) type RowHook = Box<dyn FnMut(RowChange) -> bool + Send>;
pub(crate) type RowHooks = Arc<Mutex<Vec<RowHook>>>;

/// Installs SQLite's update hook on `conn`, feeding `hooks`.
pub(crate) fn install(conn: &mut rusqlite::Connection, hooks: RowHooks) -> Result<()> {
    conn.update_hook(Some(move |_, _: &str, table: &str, id: i64| {
        let change = match table {
            "projects" => RowChange::Project(id),
            "threads" => RowChange::Thread(id),
            _ => return,
        };
        let mut hooks = hooks.lock().unwrap_or_else(PoisonError::into_inner);
        hooks.retain_mut(|hook| hook(change));
    }))?;
    Ok(())
}

impl Store {
    /// Called with every project and thread row change, as it is written (before its transaction commits), under
    /// the caller's lock: it must not block or touch the store. Hand the id to another thread and re-read it there.
    pub fn on_rows_changed(&self, hook: impl FnMut(RowChange) -> bool + Send + 'static) {
        self.row_hooks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Box::new(hook));
    }

    /// The project if [`Store::list_projects`] lists it (not removed).
    pub fn shell_project(&self, id: i64) -> Result<Option<Project>> {
        let sql =
            format!("SELECT {PROJECT_COLS} FROM projects WHERE id = ?1 AND removed_at IS NULL");
        Ok(self
            .conn
            .query_row(&sql, [id], project_from_row)
            .optional()?)
    }

    /// The thread if [`Store::list_all_threads`] lists it (not deleted, not a side chat, its project not removed).
    pub fn shell_thread(&self, id: i64) -> Result<Option<Thread>> {
        let cols = THREAD_COLS
            .split(", ")
            .map(|c| format!("t.{c}"))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT {cols} FROM threads t JOIN projects p ON p.id = t.project_id
             WHERE t.id = ?1 AND t.deleted_at IS NULL AND t.side_of IS NULL AND p.removed_at IS NULL"
        );
        Ok(self
            .conn
            .query_row(&sql, [id], thread_from_row)
            .optional()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NewThread;
    use std::path::Path;

    #[test]
    fn every_row_write_reaches_the_hook_and_a_dead_hook_is_dropped() {
        let store = Store::open_in_memory().expect("store");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        store.on_rows_changed(move |c| {
            log.lock().expect("log").push(c);
            true
        });
        let calls = Arc::new(Mutex::new(0));
        let count = calls.clone();
        store.on_rows_changed(move |_| {
            *count.lock().expect("count") += 1;
            false
        });
        let p = store
            .create_project("app", Path::new("/src/app"))
            .expect("project");
        let t = store
            .create_thread(&NewThread {
                project_id: p.id,
                title: "t",
                cwd: Path::new("/src/app"),
                account_id: "work",
                status: "idle",
            })
            .expect("thread");
        store.set_thread_title(t.id, "renamed").expect("rename");
        store.set_thread_settled(t.id, true).expect("settle");
        store.delete_thread(t.id).expect("delete");
        let seen = seen.lock().expect("log").clone();
        assert_eq!(seen.first(), Some(&RowChange::Project(p.id)));
        assert!(
            seen.iter().skip(1).all(|c| *c == RowChange::Thread(t.id)),
            "{seen:?}"
        );
        assert!(
            seen.len() >= 5,
            "create, two title writes, settle, delete: {seen:?}"
        );
        assert_eq!(
            *calls.lock().expect("count"),
            1,
            "dropped after saying false"
        );
        assert_eq!(store.shell_thread(t.id).expect("read"), None);
        assert!(store.shell_project(p.id).expect("read").is_some());
    }
}
