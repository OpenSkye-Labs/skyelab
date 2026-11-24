//! The threads around the agent's pipes: stdin's writer, stdout's event reader, stderr's tail keeper.

use crate::{Event, MAX_LINE, STDERR_TAIL};
use serde_json::Value;
use std::io::{BufRead as _, BufReader, Read, Write as _};
use std::process::ChildStdin;
use std::sync::Mutex;

// shortcut: a tool that left claude's process group and kept its stdin can block this thread after kill(); it leaks one thread, add a close handle if that shows up.
pub(crate) fn write_lines(mut stdin: ChildStdin, lines: &flume::Receiver<String>) {
    while let Ok(line) = lines.recv() {
        if writeln!(stdin, "{line}")
            .and_then(|()| stdin.flush())
            .is_err()
        {
            break;
        }
    }
}

/// Keeps the newest [`STDERR_TAIL`] bytes of the agent's stderr, and copies it to ours as before.
pub(crate) fn read_stderr(mut stderr: impl Read, tail: &Mutex<Vec<u8>>) {
    let mut chunk = [0u8; 8192];
    while let Ok(n) = stderr.read(&mut chunk) {
        if n == 0 {
            break;
        }
        let _ = std::io::stderr().write_all(&chunk[..n]);
        let mut buf = tail.lock().unwrap_or_else(|e| e.into_inner());
        buf.extend_from_slice(&chunk[..n]);
        if buf.len() > STDERR_TAIL {
            let cut = buf.len() - STDERR_TAIL;
            buf.drain(..cut);
        }
    }
}

pub(crate) fn read_events(
    stdout: impl Read,
    tx: &flume::Sender<Event>,
    mut parse: impl FnMut(&str) -> Vec<Event>,
) {
    let mut stdout = BufReader::new(stdout);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        let limit = MAX_LINE as u64 + 1;
        if !matches!((&mut stdout).take(limit).read_until(b'\n', &mut buf), Ok(n) if n > 0) {
            break;
        }
        let event = if buf.len() > MAX_LINE && buf.last() != Some(&b'\n') {
            buf.truncate(MAX_LINE);
            let _ = stdout.skip_until(b'\n');
            let mut line = String::from_utf8_lossy(&buf).into_owned();
            while line.len() > MAX_LINE {
                line.pop(); // a cut mid-character becomes a 3-byte U+FFFD
            }
            vec![Event::Unknown(Value::String(line))]
        } else {
            let line = String::from_utf8_lossy(&buf);
            if line.trim().is_empty() {
                continue;
            }
            let _span = tracing::info_span!("agent.parse_line").entered();
            parse(line.trim_end())
        };
        if event.into_iter().any(|e| tx.send(e).is_err()) {
            break;
        }
    }
}
