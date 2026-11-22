//! Runs Claude Code sessions under the account bound to their folder. No GPUI, no tokio:
//! a std reader thread feeds a flume channel, which GPUI's executor can await.

mod event;
mod launch;

pub use event::{Block, Event};

use anyhow::{Context as _, Result, bail};
use serde_json::{Value, json};
use skyelab_core::{Account, AccountsConfig};
use std::io::{BufRead as _, BufReader, Read, Write as _};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};

/// Longest stdout line kept whole; a longer one arrives as a truncated [`Event::Unknown`].
const MAX_LINE: usize = 16 << 20;
/// Stdin lines queued for the writer thread. A full queue means `claude` stopped reading.
const STDIN_QUEUE: usize = 64;

/// A running `claude` process. Dropping it kills the process.
///
/// Stdin writes go through a writer thread, so `send_user_message`, `interrupt` and `kill`
/// never wait behind a `claude` that stopped reading.
pub struct Session {
    child: Child,
    /// Set once the child is reaped. Its pid may then belong to an unrelated process.
    status: Option<ExitStatus>,
    stdin: flume::Sender<String>,
    events: flume::Receiver<Event>,
    next_request: u64,
}

/// What to launch: the account the caller resolved for `dir` (and recorded), and optionally a
/// session to continue.
pub struct Launch<'a> {
    pub account: &'a Account,
    pub dir: &'a Path,
    /// `claude`'s session id (from [`Event::Init`]) to continue with `--resume`.
    pub resume: Option<&'a str>,
    pub prompt: &'a str,
}

/// Starts `claude` per `launch` and sends its prompt as the first turn.
pub fn start(config: &AccountsConfig, launch: &Launch) -> Result<Session> {
    start_with_bin(Path::new("claude"), config, launch)
}

/// [`start`] with another binary in place of `claude`, so tests can run a fake. `bin` is an
/// absolute path, or a bare name looked up in the absolute entries of our PATH.
///
/// `config` is read again here: if `launch.dir` no longer resolves to `launch.account`, nothing
/// starts, so the account the caller recorded is the one the process runs under.
pub fn start_with_bin(bin: &Path, config: &AccountsConfig, launch: &Launch) -> Result<Session> {
    let &Launch {
        account,
        dir,
        resume,
        prompt,
    } = launch;
    // The child starts in the exact folder that was checked, even if a link in `dir` is retargeted.
    let dir = &std::fs::canonicalize(dir).with_context(|| format!("opening {}", dir.display()))?;
    match config.resolve(dir) {
        Some(now) if now == account => {}
        Some(now) => bail!(
            "{} is now bound to '{}', not '{}'; nothing was started",
            dir.display(),
            now.id,
            account.id
        ),
        None => bail!("{} is not bound to an account", dir.display()),
    }
    if let Some(id) = resume
        && !valid_session_id(id)
    {
        bail!("refusing to resume session id {id:?}");
    }
    let env = account.launch_env()?;
    // A relative dir would resolve inside the untrusted project, and an empty one means ~/.claude.
    if let Some((var, v)) = env.iter().find(|(_, v)| !v.is_absolute()) {
        bail!(
            "{var} for {} must be an absolute path, got {v:?}",
            dir.display()
        );
    }
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
    .args(resume.into_iter().flat_map(|id| ["--resume", id]))
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
    let (stdin_tx, stdin_rx) = flume::bounded(STDIN_QUEUE);
    // Built before the threads spawn, so every error below still kills and reaps the child on drop.
    let mut session = Session {
        child,
        status: None,
        stdin: stdin_tx,
        events,
        next_request: 0,
    };
    std::thread::Builder::new()
        .name("skyelab-agent-reader".into())
        .spawn(move || read_events(stdout, &tx))
        .context("spawning reader thread")?;
    std::thread::Builder::new()
        .name("skyelab-agent-writer".into())
        .spawn(move || write_lines(stdin, &stdin_rx))
        .context("spawning writer thread")?;
    session.send_user_message(prompt)?;
    Ok(session)
}

/// Claude Code's session ids are UUIDs; allowing only id-safe characters keeps anything flag-like off the command line.
fn valid_session_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && !id.starts_with('-')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

// shortcut: a tool that left claude's process group and kept its stdin can block this thread after kill(); it leaks one thread, add a close handle if that shows up.
fn write_lines(mut stdin: ChildStdin, lines: &flume::Receiver<String>) {
    while let Ok(line) = lines.recv() {
        if writeln!(stdin, "{line}")
            .and_then(|()| stdin.flush())
            .is_err()
        {
            break;
        }
    }
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
            let mut line = String::from_utf8_lossy(&buf).into_owned();
            while line.len() > MAX_LINE {
                line.pop(); // a cut mid-character becomes a 3-byte U+FFFD
            }
            Event::Unknown(Value::String(line))
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
    /// once the process has been reaped, by an earlier `kill` or a `try_wait` that returned `Some`.
    // shortcut: after try_wait reaped a self-exited claude, survivors in its group keep running; fine while claude cleans up its own tools.
    pub fn kill(&mut self) -> Result<()> {
        // Unreaped, even a zombie leader still holds its pid and process group id, so they are safe to signal.
        if self.status.is_some() {
            return Ok(());
        }
        // `unsafe` is denied, so no libc::killpg: the kill binary signals the group instead.
        #[cfg(unix)]
        {
            let kill =
                launch::resolve_bin(Path::new("kill")).unwrap_or_else(|_| "/bin/kill".into());
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

    /// Queues one line for the writer thread; never blocks.
    fn write(&mut self, msg: &serde_json::Value) -> Result<()> {
        self.stdin.try_send(msg.to_string()).map_err(|e| match e {
            flume::TrySendError::Full(_) => {
                anyhow::anyhow!("claude isn't reading its input; stop it and try again")
            }
            flume::TrySendError::Disconnected(_) => anyhow::anyhow!("claude's input is closed"),
        })
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.kill();
    }
}
