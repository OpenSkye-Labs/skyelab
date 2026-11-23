//! Ending an agent: reaping it, and killing its whole process tree.

use crate::{Session, launch};
use anyhow::{Context as _, Result};
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};

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

    /// Ends the agent's process tree: SIGKILL to its process group on unix (the group is the agent's pid), closing
    /// its kill-on-close Job Object on Windows.
    fn kill_group(&mut self) {
        #[cfg(windows)]
        drop(self.job.take());
        // `unsafe` is denied, so no libc::killpg: the kill binary signals the group instead.
        #[cfg(unix)]
        {
            let kill =
                launch::resolve_bin(Path::new("kill")).unwrap_or_else(|_| "/bin/kill".into());
            let _ = Command::new(kill)
                .args(["-KILL", "--", &format!("-{}", self.child.id())])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}
