//! Starting children, and running a short-lived one and reading what it printed, without pipes.
//!
//! On macOS std creates a pipe and only then marks it close-on-exec, so a process another thread spawns in between
//! inherits its write end. A read to EOF then waits for that unrelated process (an agent that runs for hours). That
//! includes the pipe std's own `spawn` reads to learn whether the exec worked: `spawn` itself then never returns.
//! Sockets are made the same way (a listener a child inherits keeps its port bound). So every child this process
//! starts goes through [`spawn`] (or [`exclusive`]), one at a time, and so do the listeners we bind: no pipe or
//! listener is ever half made while another thread forks. Sockets made elsewhere (accepted or outgoing connections)
//! can still reach a child; closing them then doesn't end the connection until that child exits. On top of that,
//! [`output`]'s stdin, stdout and stderr are unnamed temp files (opened close-on-exec in one step), read once the
//! child has exited.

use std::fs::File;
use std::io::{self, Read as _, Seek as _, SeekFrom, Write as _};
use std::ops::{Deref, DerefMut};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

/// Starts `cmd` while no other child of this process is being started (see the module docs).
pub fn spawn(cmd: &mut Command) -> io::Result<Owned> {
    let started = exclusive(|| cmd.spawn());
    crate::debug::record_spawn(cmd, &started);
    started.map(|child| Owned(Some(child)))
}

/// A child this process started. Dropped before it was reaped, it is killed and reaped, so it never outlives its
/// owner and never stays a zombie (they count toward the per-user process limit). One meant to outlive its handle
/// (an editor, the installer) is [`Owned::detach`]ed instead.
#[derive(Debug)]
pub struct Owned(Option<Child>);

const TAKEN: &str = "only detach and drop take the child, and both consume the handle";

impl Owned {
    /// Lets the child run on, and reaps it on a thread of its own once it exits.
    pub fn detach(mut self) {
        if let Some(child) = self.0.take() {
            reap_in_background(child);
        }
    }
}

impl Deref for Owned {
    type Target = Child;
    fn deref(&self) -> &Child {
        self.0.as_ref().expect(TAKEN)
    }
}

impl DerefMut for Owned {
    fn deref_mut(&mut self) -> &mut Child {
        self.0.as_mut().expect(TAKEN)
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        let Some(mut child) = self.0.take() else {
            return;
        };
        // std keeps the status of a reaped child, so this never signals a pid that was reused.
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        let _ = child.kill();
        // A killed child is usually gone within a few ms; the rare slow one is reaped off the caller (the UI thread).
        for _ in 0..20 {
            if !matches!(child.try_wait(), Ok(None)) {
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        reap_in_background(child);
    }
}

fn reap_in_background(mut child: Child) {
    let spawned = std::thread::Builder::new()
        .name("skyelab-reap".into())
        .spawn(move || child.wait());
    if let Err(e) = spawned {
        tracing::warn!("no thread to reap a child: {e}");
    }
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
    pub child: Owned,
    stdout: File,
    stderr: File,
    /// Timed for the debug log while debug mode is on.
    debug: Option<crate::debug::Started>,
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
            debug: crate::debug::Started::now(cmd),
        })
    }

    /// What the child printed, once `status` (from `wait` or `try_wait`) says it exited.
    pub fn output(self, status: ExitStatus) -> io::Result<Output> {
        if let Some(started) = self.debug {
            started.exited(status);
        }
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
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn output(cmd: &mut Command, input: Option<&[u8]>) -> io::Result<Output> {
    let _span = tracing::info_span!("process", program = ?cmd.get_program()).entered();
    Captured::spawn(cmd, input)?.wait()
}

/// [`output`], with the child killed once `limit` passes: then a `TimedOut` error, never a wait forever.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn output_within(
    cmd: &mut Command,
    input: Option<&[u8]>,
    limit: Duration,
) -> io::Result<Output> {
    let _span = tracing::info_span!("process", program = ?cmd.get_program()).entered();
    let mut run = Captured::spawn(cmd, input)?;
    let start = Instant::now();
    loop {
        if let Some(status) = run.child.try_wait()? {
            return run.output(status);
        }
        if start.elapsed() > limit {
            let _ = run.child.kill();
            let _ = run.child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("no answer within {}s", limit.as_secs()),
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn output_within_kills_a_child_past_its_limit() {
        let start = Instant::now();
        let err = output_within(
            Command::new("/bin/sleep").arg("30"),
            None,
            Duration::from_millis(200),
        )
        .expect_err("timed out");
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert!(start.elapsed() < Duration::from_secs(5));
        let out = output_within(
            Command::new("/bin/echo").arg("hi"),
            None,
            Duration::from_secs(5),
        )
        .expect("runs");
        assert_eq!(out.stdout, b"hi\n");
    }
}
