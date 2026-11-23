//! Runs agent sessions (Claude Code, or Codex) under the account bound to their folder. No GPUI, no
//! tokio: a std reader thread feeds a flume channel, which GPUI's executor can await.

mod attach;
mod claude;
mod codex;
mod event;
mod launch;
#[cfg(unix)]
pub mod mcp;
mod models;

pub use attach::{Attachment, Kind as AttachmentKind, MAX_ATTACHMENT_BYTES, MAX_ATTACHMENTS};
pub use event::{Block, Event};
pub use models::{CLAUDE_EFFORTS, CLAUDE_MODELS, ModelInfo, list_codex_models};

use anyhow::{Context as _, Result, bail};
use serde_json::{Value, json};
use skyelab_core::{Account, AccountsConfig, Provider};
use std::io::{BufRead as _, BufReader, Read, Write as _};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};

/// Longest stdout line kept whole; a longer one arrives as a truncated [`Event::Unknown`].
const MAX_LINE: usize = 16 << 20;
/// Stdin lines queued for the writer thread. A full queue means `claude` stopped reading.
const STDIN_QUEUE: usize = 64;

/// A running agent process (`claude`, or `codex app-server`). Dropping it kills the process.
///
/// Stdin writes go through a writer thread, so `send_user_message`, `interrupt` and `kill`
/// never wait behind an agent that stopped reading.
pub struct Session {
    child: Child,
    /// Set once the child is reaped. Its pid may then belong to an unrelated process.
    status: Option<ExitStatus>,
    stdin: flume::Sender<String>,
    events: flume::Receiver<Event>,
    next_request: u64,
    /// Set for Codex: the protocol state the reader thread shares.
    codex: Option<Arc<Mutex<codex::Codex>>>,
    sent: claude::Sent,
    /// Windows: the Job Object the agent runs in. Closing it ends the agent and every process it started (on
    /// Windows `Child::kill` ends only the `claude.cmd` shim's cmd.exe).
    #[cfg(windows)]
    job: Option<win32job::Job>,
}

/// What to launch: the account the caller resolved for `dir` (and recorded), and optionally a
/// session to continue.
pub struct Launch<'a> {
    pub account: &'a Account,
    pub dir: &'a Path,
    /// The session id (from [`Event::Init`]) to continue: `claude --resume`, or Codex's `thread/resume`.
    pub resume: Option<&'a str>,
    pub prompt: &'a str,
    /// A `--mcp-config` file for SkyeLab's own tools (an [`mcp::Grant`]'s), or `None` for no tools. Codex gets
    /// the same server, read from this file and sent with `thread/start`.
    pub mcp_config: Option<&'a Path>,
    /// `--model` for Claude (an alias or a full id), `model` for Codex. `None`: the account's own default.
    pub model: Option<&'a str>,
    /// `--effort` for Claude, the turn's `effort` for Codex. `None`: the default.
    pub effort: Option<&'a str>,
    /// Files sent with the first prompt.
    pub attachments: &'a [Attachment],
}

/// Starts the account's agent (`claude` or `codex`) per `launch` and sends its prompt as the first turn.
pub fn start(config: &AccountsConfig, launch: &Launch) -> Result<Session> {
    let bin = match launch.account.provider {
        Provider::Claude => "claude",
        Provider::Codex => "codex",
    };
    start_with_bin(Path::new(bin), config, launch)
}

/// [`start`] with another binary in place of `claude` / `codex`, so tests can run a fake. `bin` is an
/// absolute path, or a bare name looked up in the absolute entries of our PATH. The account's provider
/// picks the protocol.
///
/// `config` is read again here: if `launch.dir` no longer resolves to `launch.account`, nothing
/// starts, so the account the caller recorded is the one the process runs under.
pub fn start_with_bin(bin: &Path, config: &AccountsConfig, launch: &Launch) -> Result<Session> {
    let &Launch {
        account,
        dir,
        resume,
        prompt,
        mcp_config,
        model,
        effort,
        attachments,
    } = launch;
    if let Some(id) = resume
        && !valid_session_id(id)
    {
        bail!("refusing to resume session id {id:?}");
    }
    for (what, v) in [("model", model), ("effort", effort)] {
        if let Some(v) = v
            && !models::valid_name(v)
        {
            bail!("refusing {what} {v:?}");
        }
    }
    attach::check_count(attachments)?;
    let (mut cmd, bin, dir) = launch::agent_command(bin, config, account, dir)?;
    let dir = &dir;
    let codex = match account.provider {
        Provider::Claude => {
            claude_args(&mut cmd, mcp_config, resume);
            cmd.args(model.into_iter().flat_map(|m| ["--model", m]))
                .args(effort.into_iter().flat_map(|e| ["--effort", e]));
            None
        }
        Provider::Codex => {
            cmd.args(codex::args(dir)?);
            let mcp = mcp_config.map(codex::mcp_servers).transpose()?;
            Some(codex::Codex::new(
                dir,
                resume,
                mcp,
                codex::Pick {
                    model: model.map(str::to_owned),
                    effort: effort.map(str::to_owned),
                },
            ))
        }
    };
    cmd.current_dir(dir)
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
    let codex = codex.map(|(state, opening)| (Arc::new(Mutex::new(state)), opening));
    // Built before the threads spawn, so every error below still kills and reaps the child on drop.
    let mut session = Session {
        child,
        status: None,
        stdin: stdin_tx.clone(),
        events,
        next_request: 0,
        codex: codex.as_ref().map(|(state, _)| state.clone()),
        sent: claude::Sent::default(),
        #[cfg(windows)]
        job: None,
    };
    // shortcut: a process the shim starts before this assignment is outside the job (a microsecond window); spawn suspended if that shows up.
    #[cfg(windows)]
    {
        session.job = Some(job_for(&session.child).context("putting the agent in a Job Object")?);
    }
    let parse: Box<Parse> = match codex {
        None => {
            let sent = session.sent.clone();
            Box::new(move |line| vec![claude::parse(line, &sent)])
        }
        Some((state, opening)) => {
            for line in opening {
                session.queue(line)?;
            }
            Box::new(move |line| {
                let Ok(v) = serde_json::from_str::<Value>(line) else {
                    return vec![Event::Unknown(Value::String(line.to_owned()))];
                };
                let (events, replies) = lock(&state).on_line(v);
                for reply in replies {
                    let _ = stdin_tx.try_send(reply); // a full queue: codex stopped reading, nothing to save
                }
                events
            })
        }
    };
    std::thread::Builder::new()
        .name("skyelab-agent-reader".into())
        .spawn(move || read_events(stdout, &tx, parse))
        .context("spawning reader thread")?;
    std::thread::Builder::new()
        .name("skyelab-agent-writer".into())
        .spawn(move || write_lines(stdin, &stdin_rx))
        .context("spawning writer thread")?;
    session.send_message(prompt, attachments)?;
    Ok(session)
}

/// A Job Object that ends every process in it when it closes, with `child` in it.
#[cfg(windows)]
fn job_for(child: &std::process::Child) -> Result<win32job::Job> {
    use std::os::windows::io::AsRawHandle as _;
    let mut limits = win32job::ExtendedLimitInfo::new();
    limits.limit_kill_on_job_close();
    let job = win32job::Job::create_with_limit_info(&limits)?;
    job.assign_process(child.as_raw_handle() as isize)?;
    Ok(job)
}

/// Turns one stdout line into events (and, for Codex, answers it).
type Parse = dyn FnMut(&str) -> Vec<Event> + Send;

fn lock(state: &Mutex<codex::Codex>) -> std::sync::MutexGuard<'_, codex::Codex> {
    // A panic mid-update leaves plain data behind; the next line still gets handled.
    state.lock().unwrap_or_else(|e| e.into_inner())
}

fn claude_args(cmd: &mut Command, mcp_config: Option<&Path>, resume: Option<&str>) {
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
    // Our own server only (--strict-mcp-config keeps every other out); its tools enforce their own rules.
    .args(mcp_config.into_iter().flat_map(|path| {
        [
            "--mcp-config".as_ref(),
            path.as_os_str(),
            "--allowedTools".as_ref(),
            "mcp__skyelab".as_ref(),
        ]
    }))
    .args(resume.into_iter().flat_map(|id| ["--resume", id]));
}

/// Claude Code's session ids and Codex's thread ids are UUIDs; allowing only id-safe characters keeps anything flag-like off the command line.
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

fn read_events(
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
            parse(line.trim_end())
        };
        if event.into_iter().any(|e| tx.send(e).is_err()) {
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

    /// Sends a follow-up user turn. Codex queues it behind a running turn, as `claude` does.
    pub fn send_user_message(&mut self, text: &str) -> Result<()> {
        self.send_message(text, &[])
    }

    /// [`send_user_message`](Self::send_user_message) with files attached.
    pub fn send_message(&mut self, text: &str, attachments: &[Attachment]) -> Result<()> {
        attach::check_count(attachments)?;
        if let Some(state) = &self.codex {
            let lines = lock(state).prompt(text, attachments, false)?;
            return lines.into_iter().try_for_each(|l| self.queue(l));
        }
        let message = claude::user_message(text, attachments, &self.sent);
        self.write(&message)
    }

    /// Sends a message into the running turn instead of after it. Claude folds a message sent mid-turn into that
    /// turn already; Codex gets `turn/steer` (or a new turn when none runs).
    pub fn steer(&mut self, text: &str, attachments: &[Attachment]) -> Result<()> {
        attach::check_count(attachments)?;
        if let Some(state) = &self.codex {
            let lines = lock(state).prompt(text, attachments, true)?;
            return lines.into_iter().try_for_each(|l| self.queue(l));
        }
        self.send_message(text, attachments)
    }

    /// Switches the model for the next turns. Claude takes `set_model` (its effort is fixed at launch, so `effort`
    /// applies from the next start); Codex sends both with every turn.
    pub fn set_model(&mut self, model: Option<&str>, effort: Option<&str>) -> Result<()> {
        for v in [model, effort].into_iter().flatten() {
            if !models::valid_name(v) {
                bail!("refusing model {v:?}");
            }
        }
        if let Some(state) = &self.codex {
            lock(state).pick = codex::Pick {
                model: model.map(str::to_owned),
                effort: effort.map(str::to_owned),
            };
            return Ok(());
        }
        self.next_request += 1;
        let request_id = format!("skyelab-{}", self.next_request);
        self.write(&json!({
            "type": "control_request",
            "request_id": request_id,
            "request": { "subtype": "set_model", "model": model },
        }))
    }

    /// Asks the agent to stop the current turn; the process stays alive for the next message.
    pub fn interrupt(&mut self) -> Result<()> {
        if let Some(state) = &self.codex {
            let lines = lock(state).interrupt();
            return lines.into_iter().try_for_each(|l| self.queue(l));
        }
        self.next_request += 1;
        let request_id = format!("skyelab-{}", self.next_request);
        self.write(&json!({
            "type": "control_request",
            "request_id": request_id,
            "request": { "subtype": "interrupt" },
        }))
    }

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

    fn write(&mut self, msg: &serde_json::Value) -> Result<()> {
        self.queue(msg.to_string())
    }

    /// Queues one line for the writer thread; never blocks.
    fn queue(&self, line: String) -> Result<()> {
        self.stdin.try_send(line).map_err(|e| match e {
            flume::TrySendError::Full(_) => {
                anyhow::anyhow!("the agent isn't reading its input; stop it and try again")
            }
            flume::TrySendError::Disconnected(_) => anyhow::anyhow!("the agent's input is closed"),
        })
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.kill();
    }
}
