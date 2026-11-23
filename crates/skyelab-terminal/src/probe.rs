//! What a terminal's shell is running and where: tab titles, the close check and restore read it.

use std::path::PathBuf;

/// What a shell is doing now: its tab's title and whether closing it stops something.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Foreground {
    /// The program running in front of the shell (`sleep`, `vim`), by process name; `None` at the prompt.
    pub command: Option<String>,
    /// The shell's working folder now, when the OS tells (`None`: unknown, keep the last one).
    pub cwd: Option<PathBuf>,
}

/// Asks the OS about a terminal's shell. Cheap to clone; the calls block briefly, so run [`Probe::foreground`] off
/// the UI thread.
#[derive(Clone)]
pub struct Probe {
    pub(crate) pid: u32,
    /// The PTY's controller side: its foreground process group is the command in front (unix).
    #[cfg(unix)]
    pub(crate) controller: std::sync::Arc<std::os::fd::OwnedFd>,
}

impl Probe {
    /// The process in front of the shell, if it isn't the shell itself.
    #[cfg(unix)]
    fn front(&self) -> Option<u32> {
        let group = rustix::termios::tcgetpgrp(&*self.controller).ok()?;
        let group = group.as_raw_nonzero().get() as u32;
        (group != self.pid).then_some(group)
    }

    /// The shell's first child: Windows has no foreground group.
    #[cfg(windows)]
    fn front(&self) -> Option<u32> {
        use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
        let mut sys = System::new().ok()?;
        sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing(),
        );
        let shell = Pid::from_u32(self.pid);
        sys.processes()
            .iter()
            .filter(|(_, p)| p.parent() == Some(shell))
            .map(|(pid, _)| pid.as_u32())
            .min()
    }

    /// A command other than the shell is running (closing would stop it).
    pub fn is_busy(&self) -> bool {
        self.front().is_some()
    }

    pub fn foreground(&self) -> Foreground {
        use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
        let front = self.front();
        let Ok(mut sys) = System::new() else {
            return Foreground::default();
        };
        let pids: Vec<Pid> = std::iter::once(self.pid)
            .chain(front)
            .map(Pid::from_u32)
            .collect();
        sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&pids),
            true,
            ProcessRefreshKind::nothing().with_cwd(UpdateKind::Always),
        );
        let command = front.and_then(|pid| {
            let name = sys.process(Pid::from_u32(pid))?.name().to_string_lossy();
            // Windows names carry `.exe`; a title doesn't need it.
            Some(name.trim_end_matches(".exe").to_owned())
        });
        let cwd = sys
            .process(Pid::from_u32(self.pid))
            .and_then(|p| p.cwd())
            .filter(|p| !p.as_os_str().is_empty())
            .map(PathBuf::from);
        Foreground { command, cwd }
    }
}

