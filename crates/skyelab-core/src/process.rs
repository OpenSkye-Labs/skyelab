//! Starting children, and running a short-lived one and reading what it printed, without pipes.
//!
//! On macOS std creates a pipe and only then marks it close-on-exec, so a process another thread spawns in between
//! inherits its write end. A read to EOF then waits for that unrelated process (an agent that runs for hours). That
//! includes the pipe std's own `spawn` reads to learn whether the exec worked: `spawn` itself then never returns.
//! Sockets are made the same way (a listener a child inherits keeps its port bound). So every child this process
//! starts goes through [`spawn`] (or [`exclusive`]), one at a time, and so does a listener whose port must come free
//! when it closes: no fd is ever half made while another thread forks. On top of that, [`output`]'s stdin, stdout and stderr are unnamed temp files
//! (opened close-on-exec in one step), read once the child has exited.

use std::fs::File;
use std::io::{self, Read as _, Seek as _, SeekFrom, Write as _};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::{Mutex, PoisonError};

/// Starts `cmd` while no other child of this process is being started (see the module docs).
pub fn spawn(cmd: &mut Command) -> io::Result<Child> {
    exclusive(|| cmd.spawn())
}

/// Runs `start` under [`spawn`]'s lock: a spawn that doesn't go through a `Command` here (a terminal's PTY), or an
/// fd std makes close-on-exec in a second step that no child may keep (a listener on a fixed port).
pub fn exclusive<T>(start: impl FnOnce() -> T) -> T {
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
    let _held = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
    start()
}

/// A spawned child whose stdout and stderr go to temp files.
pub struct Captured {
    pub child: Child,
    stdout: File,
    stderr: File,
}

impl Captured {
    /// Spawns `cmd` with `input` on stdin (nothing without it).
    pub fn spawn(cmd: &mut Command, input: Option<&[u8]>) -> io::Result<Self> {
        let stdin = match input {
            Some(bytes) => {
                let mut file = tempfile::tempfile()?;
                file.write_all(bytes)?;
                file.seek(SeekFrom::Start(0))?;
                Stdio::from(file)
            }
            None => Stdio::null(),
        };
        let (stdout, stderr) = (tempfile::tempfile()?, tempfile::tempfile()?);
        let child = spawn(
            cmd.stdin(stdin)
                .stdout(stdout.try_clone()?)
                .stderr(stderr.try_clone()?),
        )?;
        Ok(Self {
            child,
            stdout,
            stderr,
        })
    }

    /// What the child printed, once `status` (from `wait` or `try_wait`) says it exited.
    pub fn output(self, status: ExitStatus) -> io::Result<Output> {
        Ok(Output {
            status,
            stdout: read_all(self.stdout)?,
            stderr: read_all(self.stderr)?,
        })
    }

    pub fn wait(mut self) -> io::Result<Output> {
        let status = self.child.wait()?;
        self.output(status)
    }
}

fn read_all(mut file: File) -> io::Result<Vec<u8>> {
    file.seek(SeekFrom::Start(0))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    Ok(buf)
}

/// Like [`Command::output`], with `input` on stdin: runs `cmd` to its exit and returns what it printed.
pub fn output(cmd: &mut Command, input: Option<&[u8]>) -> io::Result<Output> {
    Captured::spawn(cmd, input)?.wait()
}
