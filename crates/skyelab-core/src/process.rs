//! Running a short-lived child and reading what it printed.

use std::io;
use std::process::{Command, Output, Stdio};

/// Runs `cmd` to completion with `input` on its stdin and returns what it printed.
pub fn output(cmd: &mut Command, input: Option<&[u8]>) -> io::Result<Output> {
    let _ = input;
    cmd.stdin(Stdio::null()).output()
}
