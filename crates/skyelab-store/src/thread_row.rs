//! How a `threads` row reads into a [`Thread`].

use crate::Thread;
use rusqlite::Row;
use std::path::PathBuf;

pub(crate) const THREAD_COLS: &str = "id, project_id, title, cwd, account_id, agent_session_id, status, created_at, updated_at, worktree_path, worktree_branch, worktree_base, pr_url, parent_thread_id, permission_mode, pinned_at, archived_at, unread, forked_from, pr_state, side_of, scheduled_task_id, settled_at, snoozed_until, compare_group";

pub(crate) fn thread_from_row(r: &Row) -> rusqlite::Result<Thread> {
    Ok(Thread {
        id: r.get(0)?,
        project_id: r.get(1)?,
        title: r.get(2)?,
        cwd: PathBuf::from(r.get::<_, String>(3)?),
        account_id: r.get(4)?,
        agent_session_id: r.get(5)?,
        status: r.get(6)?,
        created_at: r.get(7)?,
        updated_at: r.get(8)?,
        worktree_path: r.get::<_, Option<String>>(9)?.map(PathBuf::from),
        worktree_branch: r.get(10)?,
        worktree_base: r.get(11)?,
        pr_url: r.get(12)?,
        parent_thread_id: r.get(13)?,
        permission_mode: r.get(14)?,
        pinned_at: r.get(15)?,
        archived_at: r.get(16)?,
        unread: r.get(17)?,
        forked_from: r.get(18)?,
        pr_state: r.get(19)?,
        side_of: r.get(20)?,
        scheduled_task_id: r.get(21)?,
        settled_at: r.get(22)?,
        snoozed_until: r.get(23)?,
        compare_group: r.get(24)?,
    })
}
