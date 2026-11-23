//! Running a short-lived child and reading what it printed, without pipes.
//!
//! On macOS std creates a pipe and only then marks it close-on-exec, so a process another thread spawns in between
//! inherits its write end. A read to EOF then waits for that unrelated process (an agent that runs for hours). Here
//! the child's stdin, stdout and stderr are unnamed temp files (opened close-on-exec in one step), and they are read
//! once the child has exited, so no other process can hold the call open.

use std::fs::File;
use std::io::{self, Read as _, Seek as _, SeekFrom, Write as _};
use std::process::{Child, Command, ExitStatus, Output, Stdio};

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
        let child = cmd
            .stdin(stdin)
            .stdout(stdout.try_clone()?)
            .stderr(stderr.try_clone()?)
            .spawn()?;
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
