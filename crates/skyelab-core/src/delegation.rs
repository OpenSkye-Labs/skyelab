//! When one agent may start or drive another thread. A delegated thread runs under its target folder's bound
//! account, and only when that is the caller's own account, so a work agent can't spend the personal plan.

use crate::{Account, AccountsConfig};
use std::path::Path;

/// A user's thread is depth 0; a thread at this depth can't delegate.
pub const MAX_DEPTH: u32 = 2;
/// Live (starting or running) children one thread may have at a time.
pub const MAX_CHILDREN: usize = 4;

impl AccountsConfig {
    /// The account a thread that `caller_account` starts in `dir` runs under. Refused when `dir` is unbound
    /// (never a default account) or bound to another account.
    pub fn delegation_account(&self, caller_account: &str, dir: &Path) -> Result<&Account, String> {
        let Some(target) = self.resolve(dir) else {
            return Err(format!(
                "{} is not bound to an account; SkyeLab never picks one, so nothing was started",
                dir.display()
            ));
        };
        if target.id != caller_account {
            return Err(format!(
                "{} is bound to account '{}', but this agent runs under '{}'; \
                 an agent can only delegate within its own account",
                dir.display(),
                target.id,
                caller_account
            ));
        }
        Ok(target)
    }

    /// Whether `caller_account` may read or drive a thread recorded under `thread_account` whose project is
    /// `project_dir`: both must be the caller's account.
    pub fn check_drive(
        &self,
        caller_account: &str,
        thread_account: &str,
        project_dir: &Path,
    ) -> Result<(), String> {
        if thread_account != caller_account {
            return Err(format!(
                "that thread ran under account '{thread_account}', not '{caller_account}'"
            ));
        }
        self.delegation_account(caller_account, project_dir)
            .map(|_| ())
    }
}

/// A thread with this store status is working on a turn, so it holds one of its parent's child slots.
pub fn is_busy(status: &str) -> bool {
    matches!(status, "starting" | "running")
}

/// Refuses a start from a caller at `depth` that already has `live_children`.
pub fn check_limits(depth: u32, live_children: usize) -> Result<(), String> {
    check_depth(depth)?;
    check_children(live_children)
}

/// Refuses a start from a caller at `depth`.
pub fn check_depth(depth: u32) -> Result<(), String> {
    if depth >= MAX_DEPTH {
        return Err(format!(
            "delegation depth limit reached: this thread is {depth} levels down (max {MAX_DEPTH})"
        ));
    }
    Ok(())
}

/// Refuses starting or resuming one more child of a parent that already has `live_children` busy.
pub fn check_children(live_children: usize) -> Result<(), String> {
    if live_children >= MAX_CHILDREN {
        return Err(format!(
            "this thread already has {live_children} running children (max {MAX_CHILDREN}); wait for one to finish"
        ));
    }
    Ok(())
}
