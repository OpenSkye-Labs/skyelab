//! Runs Claude Code sessions under the account bound to their folder. No GPUI, no tokio:
//! a std reader thread feeds a flume channel, which GPUI's executor can await.

mod event;
mod launch;

pub use event::{Block, Event};

use anyhow::{Context as _, Result};
use serde_json::{Value, json};
use skyelab_core::AccountsConfig;
use std::io::{BufRead as _, BufReader, Read, Write as _};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};

/// Longest stdout line kept whole; a longer one arrives as a truncated [`Event::Unknown`].
const MAX_LINE: usize = 16 << 20;

/// A running `claude` process. Dropping it kills the process.
///
/// Writes to stdin block if `claude` stops reading, so call `send_user_message` and `interrupt`
/// off the UI thread (e.g. GPUI's background executor).
pub struct Session {
    child: Child,
    /// Set once the child is reaped. Its pid may then belong to an unrelated process.
    status: Option<ExitStatus>,
    stdin: ChildStdin,
    events: flume::Receiver<Event>,
    next_request: u64,
}

/// Starts `claude` in `dir` with the account bound to it, and sends `prompt` as the first turn.
pub fn start(config: &AccountsConfig, dir: &Path, prompt: &str) -> Result<Session> {
    start_with_bin(Path::new("claude"), config, dir, prompt)
}

/// [`start`] with another binary in place of `claude`, so tests can run a fake. `bin` is an
/// absolute path, or a bare name looked up in the absolute entries of our PATH.
pub fn start_with_bin(
    bin: &Path,
    config: &AccountsConfig,
    dir: &Path,
    prompt: &str,
) -> Result<Session> {
    let env = config.launch_env(dir)?;
    let bin = launch::resolve_bin(bin)?;
    let mut cmd = Command::new(&bin);
    // Never inherit the parent's account or credentials: start empty, add the allowlist, then the bound account.
    cmd.env_clear().envs(launch::inherited_env()).envs(env);
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0); // so kill() reaches its children too
    cmd.args([
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        // `-p` skips the trust dialog, so the project's own settings and .mcp.json must not load.
        "--setting-sources",
        "user",
        "--strict-mcp-config",
    ])
    .current_dir(dir)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    // shortcut: stderr goes to ours, not to events; surface it once the UI shows launch failures.
    .stderr(Stdio::inherit());
    let mut child = cmd
        .spawn()
        .with_context(|| format!("starting {} in {}", bin.display(), dir.display()))?;
    let stdout = child.stdout.take().context("child stdout was piped")?;
    let stdin = child.stdin.take().context("child stdin was piped")?;

    let (tx, events) = flume::unbounded();
    // Built before the reader spawns, so every error below still kills and reaps the child on drop.
    let mut session = Session {
        child,
        status: None,
        stdin,
        events,
        next_request: 0,
    };
    std::thread::Builder::new()
        .name("skyelab-agent-reader".into())
        .spawn(move || read_events(stdout, &tx))
        .context("spawning reader thread")?;
    session.send_user_message(prompt)?;
    Ok(session)
}

fn read_events(stdout: impl Read, tx: &flume::Sender<Event>) {
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
            Event::Unknown(Value::String(String::from_utf8_lossy(&buf).into_owned()))
        } else {
            let line = String::from_utf8_lossy(&buf);
            if line.trim().is_empty() {
                continue;
            }
            Event::parse(line.trim_end())
        };
        if tx.send(event).is_err() {
            break;
        }
    }
}

impl Session {
    /// Events in the order `claude` printed them.
    ///
    /// The channel closes when the last holder of claude's stdout closes it, which can be a tool
    /// `claude` started that outlives it, not when the process exits. So a turn is over on an
    /// [`Event::Result`], and the session is over once [`kill`](Self::kill) returns or
    /// [`try_wait`](Self::try_wait) is `Some`.
    pub fn events(&self) -> &flume::Receiver<Event> {
        &self.events
    }

    /// Sends a follow-up user turn.
    pub fn send_user_message(&mut self, text: &str) -> Result<()> {
        self.write(&json!({
            "type": "user",
            "message": { "role": "user", "content": text },
            "parent_tool_use_id": null,
            "session_id": "",
        }))
    }

    /// Asks `claude` to stop the current turn; the process stays alive for the next message.
    pub fn interrupt(&mut self) -> Result<()> {
        self.next_request += 1;
        let request_id = format!("skyelab-{}", self.next_request);
        self.write(&json!({
            "type": "control_request",
            "request_id": request_id,
            "request": { "subtype": "interrupt" },
        }))
    }

    /// The exit status once `claude` has exited (reaping it), or `None` while it runs.
    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>> {
        if self.status.is_none() {
            self.status = self
                .child
                .try_wait()
                .context("checking whether claude exited")?;
        }
        Ok(self.status)
    }

    /// Kills the process (and on unix its whole process group) and waits for it. Does nothing
    /// once the process has been reaped.
    // shortcut: a claude that exited on its own and was reaped leaves any survivors in its group running; fine while claude cleans up its own tools.
    pub fn kill(&mut self) -> Result<()> {
        if self.try_wait()?.is_some() {
            return Ok(());
        }
        // `unsafe` is denied, so no libc::killpg: the kill binary signals the group instead.
        #[cfg(unix)]
        if let Ok(kill) = launch::resolve_bin(Path::new("kill")) {
            let _ = Command::new(kill)
                .args(["-KILL", "--", &format!("-{}", self.child.id())])
                .stderr(Stdio::null())
                .status();
        }
        // An already-exited child makes kill() error; wait() still reaps it.
        let _ = self.child.kill();
        self.status = Some(self.child.wait().context("waiting for claude to exit")?);
        Ok(())
    }

    fn write(&mut self, msg: &serde_json::Value) -> Result<()> {
        writeln!(self.stdin, "{msg}")
            .and_then(|()| self.stdin.flush())
            .context("writing to claude stdin")
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.kill();
    }
}
