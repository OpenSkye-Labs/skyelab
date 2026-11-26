//! Ending an agent: reaping it, and killing its whole process tree.

use crate::Session;
#[cfg(unix)]
use crate::launch;
use anyhow::{Context as _, Result};
use std::process::ExitStatus;

impl Session {
    /// The exit status once the agent has exited (reaping it), or `None` while it runs. Reaping it also ends what
    /// is left of its process group (a tool it started that still holds its stdout).
    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>> {
        if self.status.is_some() {
            return Ok(self.status);
        }
        // Unix: signal the group while the unreaped leader still holds its pid and group id, so it can't reach
        // anyone else's. Windows: the Job Object names the tree, so it closes after the reap.
        #[cfg(unix)]
        {
            if !self.exited_unreaped()? {
                return Ok(None);
            }
            self.kill_group();
        }
        self.status = self
            .child
            .try_wait()
            .context("checking whether the agent exited")?;
        #[cfg(not(unix))]
        if self.status.is_some() {
            self.kill_group();
        }
        Ok(self.status)
    }

    /// Kills the process (and on unix its whole process group) and waits for it. Does nothing
    /// once the process has been reaped, by an earlier `kill` or a `try_wait` that returned `Some`.
    pub fn kill(&mut self) -> Result<()> {
        // Unreaped, even a zombie leader still holds its pid and process group id, so they are safe to signal.
        if self.status.is_some() {
            return Ok(());
        }
        self.kill_group();
        // An already-exited child makes kill() error; wait() still reaps it.
        let _ = self.child.kill();
        self.status = Some(self.child.wait().context("waiting for the agent to exit")?);
        Ok(())
    }

    /// Whether the agent has exited, without reaping it.
    #[cfg(unix)]
    fn exited_unreaped(&self) -> Result<bool> {
        use rustix::process::{Pid, WaitId, WaitIdOptions, waitid};
        let pid = i32::try_from(self.child.id())
            .ok()
            .and_then(Pid::from_raw)
            .context("the agent's pid")?;
        let options = WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT;
        Ok(waitid(WaitId::Pid(pid), options)
            .context("checking whether the agent exited")?
            .is_some())
    }

    /// Ends the agent's process tree: on unix every descendant (a command it started in a session of its own, as
    /// Codex runs them, leaves its group), then SIGKILL to its process group (the group is the agent's pid); on
    /// Windows, closing its kill-on-close Job Object.
    pub(crate) fn kill_group(&mut self) {
        #[cfg(windows)]
        drop(self.job.take());
        #[cfg(unix)]
        {
            kill_started_within(self.child.id(), (0, u64::MAX));
            launch::kill_group(self.child.id());
        }
    }
}

/// How long an interrupted Codex command gets to end on its own before Stop kills it.
pub(crate) const COMMAND_GRACE: std::time::Duration = std::time::Duration::from_secs(2);

/// After `grace`, kills what agent `pid` started from `since` until now (the commands a turn left running) with
/// every process under them. Only the agent's own descendants, and none older than `since` (an MCP server it
/// started earlier) or newer than now (the next turn's).
pub(crate) fn end_commands_after(
    pid: u32,
    since: std::time::SystemTime,
    grace: std::time::Duration,
) {
    let secs = |t: std::time::SystemTime| {
        t.duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    };
    // Linux start times come from a boot time rounded to the second, so a command can read up to 1 s early.
    let window = (
        secs(since).saturating_sub(1),
        secs(std::time::SystemTime::now()),
    );
    let spawned = std::thread::Builder::new()
        .name("skyelab-end-commands".into())
        .spawn(move || {
            std::thread::sleep(grace);
            kill_started_within(pid, window);
        });
    if let Err(e) = spawned {
        tracing::warn!("couldn't start the thread that ends Codex's interrupted commands: {e}");
    }
}

#[cfg(not(feature = "process-tree"))]
fn kill_started_within(_agent: u32, _window: (u64, u64)) {}

/// Kills each descendant of `agent` that started within `window` (seconds since the epoch, inclusive), and all
/// below it. Only while `agent` runs: an ended agent's children belong to init, so none are found.
#[cfg(feature = "process-tree")]
fn kill_started_within(agent: u32, window: (u64, u64)) {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
    let Ok(mut sys) = System::new() else {
        return;
    };
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    let procs = sys.processes();
    let parent_of = |p: Pid| procs.get(&p).and_then(|p| p.parent());
    let under = |mut p: Pid, top: Pid| {
        // Bounded: a pid reused mid-walk can't loop it.
        for _ in 0..256 {
            let Some(up) = parent_of(p) else {
                return false;
            };
            if up == top {
                return true;
            }
            p = up;
        }
        false
    };
    let agent = Pid::from_u32(agent);
    let roots: Vec<Pid> = procs
        .iter()
        .filter(|(pid, p)| (window.0..=window.1).contains(&p.start_time()) && under(**pid, agent))
        .map(|(pid, _)| *pid)
        .collect();
    for (pid, p) in procs {
        if roots.iter().any(|r| r == pid || under(*pid, *r)) {
            p.kill();
        }
    }
}
