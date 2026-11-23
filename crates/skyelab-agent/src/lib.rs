//! Runs agent sessions (Claude Code, or Codex) under the account bound to their folder. No GPUI, no
//! tokio: a std reader thread feeds a flume channel, which GPUI's executor can await.

mod claude;
mod codex;
mod event;
mod launch;
mod pipes;
mod reap;
pub use launch::resolve_bin;
use pipes::{read_events, read_stderr, write_lines};
#[cfg(unix)]
pub mod mcp;
mod permission;

pub use claude::{PLAN_TOOL, QUESTION_TOOL};
pub use event::{Block, DeltaKind, Event, TODO_TOOL, Todo, TodoStatus, Usage, session_gone, todos};
pub use permission::{Decision, Mode, summary};

use anyhow::{Context as _, Result, bail};
use serde_json::{Value, json};
use skyelab_core::{Account, AccountsConfig, Provider};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};

/// Longest stdout line kept whole; a longer one arrives as a truncated [`Event::Unknown`].
const MAX_LINE: usize = 16 << 20;
/// Stdin lines queued for the writer thread. A full queue means `claude` stopped reading.
const STDIN_QUEUE: usize = 64;
/// The most stderr kept per session: the newest bytes win.
pub const STDERR_TAIL: usize = 64 << 10;

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
    asks: claude::Asks,
    /// The mode it started in: Claude takes Full access later only if it started with it.
    launched: Mode,
    /// Where [`Session::answer`] reports its answer. Weak, so the channel still closes with the agent's stdout.
    events_tx: flume::WeakSender<Event>,
    /// stderr not yet taken by [`Session::take_stderr`], at most [`STDERR_TAIL`] bytes.
    stderr: Arc<Mutex<Vec<u8>>>,
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
    /// Which tool calls go ahead without asking. An ask arrives as [`Event::Permission`].
    pub mode: Mode,
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
        mode,
    } = launch;
    // The child starts in the exact folder that was checked, even if a link in `dir` is retargeted.
    let dir = &std::fs::canonicalize(dir).with_context(|| format!("opening {}", dir.display()))?;
    match config.resolve(dir) {
        Some(now) if now.same_login(account) => {}
        Some(now) if now.id == account.id => bail!(
            "account '{}' changed since this launch was prepared (its config dir, gh login or ssh setting); \
             nothing was started",
            now.id
        ),
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
    // Never inherit the parent's account or credentials: start empty, add the allowlist, then the bound account,
    // and git's https credentials from that account's gh only (`git push` in the agent's own shell).
    cmd.env_clear()
        .envs(skyelab_core::env::inherited_agent_env(account.ssh_agent))
        .envs(env)
        .envs(skyelab_core::env::account_git_env(account, dir));
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0); // so kill() reaches its children too
    let codex = match account.provider {
        Provider::Claude => {
            claude_args(&mut cmd, mcp_config, resume, mode);
            None
        }
        Provider::Codex => {
            cmd.args(codex::args(dir)?);
            let mcp = mcp_config.map(codex::mcp_servers).transpose()?;
            Some(codex::Codex::new(dir, resume, mcp, mode))
        }
    };
    cmd.current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .with_context(|| format!("starting {} in {}", bin.display(), dir.display()))?;
    let stdout = child.stdout.take().context("child stdout was piped")?;
    let stdin = child.stdin.take().context("child stdin was piped")?;
    let stderr = child.stderr.take().context("child stderr was piped")?;

    let (tx, events) = flume::unbounded();
    let (stdin_tx, stdin_rx) = flume::bounded(STDIN_QUEUE);
    // Ask ids carry this, so one stored by an earlier process never names an ask of this one.
    let nonce = format!("{}-", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let codex = codex.map(|(mut state, opening)| {
        state.nonce = nonce.clone();
        (Arc::new(Mutex::new(state)), opening)
    });
    // Built before the threads spawn, so every error below still kills and reaps the child on drop.
    let mut session = Session {
        child,
        status: None,
        stdin: stdin_tx.clone(),
        events,
        next_request: 0,
        codex: codex.as_ref().map(|(state, _)| state.clone()),
        sent: claude::Sent::default(),
        asks: claude::Book::new(nonce),
        launched: mode,
        events_tx: tx.downgrade(),
        stderr: Arc::default(),
        #[cfg(windows)]
        job: None,
    };
    // shortcut: a process the shim starts before this assignment is outside the job (a microsecond window); spawn suspended if that shows up.
    #[cfg(windows)]
    {
        session.job = Some(job_for(&session.child).context("putting the agent in a Job Object")?);
    }
    // At stdout's end the agent is gone: nothing it still asks can be answered.
    let (parse, eof): (Box<Parse>, Box<dyn FnOnce() -> Vec<Event> + Send>) = match codex {
        None => {
            let (sent, asks) = (session.sent.clone(), session.asks.clone());
            let open = asks.clone();
            (
                Box::new(move |line| claude::parse(line, &sent, &asks)),
                Box::new(move || claude::withdraw_all(&open)),
            )
        }
        Some((state, opening)) => {
            for line in opening {
                session.queue(line)?;
            }
            let open = state.clone();
            let parse = Box::new(move |line: &str| {
                let Ok(v) = serde_json::from_str::<Value>(line) else {
                    return vec![Event::Unknown(Value::String(line.to_owned()))];
                };
                let (events, replies) = lock(&state).on_line(v);
                for reply in replies {
                    let _ = stdin_tx.try_send(reply); // a full queue: codex stopped reading, nothing to save
                }
                events
            });
            (parse, Box::new(move || lock(&open).withdraw_all()))
        }
    };
    std::thread::Builder::new()
        .name("skyelab-agent-reader".into())
        .spawn(move || {
            read_events(stdout, &tx, parse);
            for e in eof() {
                let _ = tx.send(e);
            }
        })
        .context("spawning reader thread")?;
    let tail = session.stderr.clone();
    std::thread::Builder::new()
        .name("skyelab-agent-stderr".into())
        .spawn(move || read_stderr(stderr, &tail))
        .context("spawning stderr thread")?;
    std::thread::Builder::new()
        .name("skyelab-agent-writer".into())
        .spawn(move || write_lines(stdin, &stdin_rx))
        .context("spawning writer thread")?;
    session.send_user_message(prompt)?;
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

fn claude_args(cmd: &mut Command, mcp_config: Option<&Path>, resume: Option<&str>, mode: Mode) {
    cmd.args([
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        // Text and thinking as they stream, as `Event::Delta`.
        "--include-partial-messages",
        // `-p` skips the trust dialog, so the project's own settings and .mcp.json must not load.
        "--setting-sources",
        "user",
        "--strict-mcp-config",
        // Every permission ask comes to us as a `can_use_tool` control request, answered in the thread.
        "--permission-prompt-tool",
        "stdio",
    ])
    // Always named, so the account's own `defaultMode` setting can't quietly loosen Ask.
    .args(match mode {
        Mode::Default | Mode::Plan | Mode::AcceptEdits => vec!["--permission-mode", mode.claude()],
        Mode::Bypass => vec![
            "--permission-mode",
            mode.claude(),
            "--allow-dangerously-skip-permissions",
        ],
    })
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
        if let Some(state) = &self.codex {
            let lines = lock(state).prompt(text)?;
            return lines.into_iter().try_for_each(|l| self.queue(l));
        }
        let message = claude::user_message(text, &self.sent);
        self.write(&message)
    }

    /// Answers the [`Event::Permission`] with this `id`; the agent was waiting for it. Then the session's events
    /// carry [`Event::PermissionResolved`]. Fails, sending nothing, when `id` is no longer waiting (answered,
    /// withdrawn, or never asked).
    pub fn answer(&mut self, id: &str, decision: Decision) -> Result<()> {
        if decision == Decision::Withdrawn {
            bail!("an answer allows or denies");
        }
        let line = match &self.codex {
            Some(state) => lock(state).answer(id, decision),
            None => claude::answer(&self.asks, id, decision).map(|v| v.to_string()),
        };
        let Some(line) = line else {
            bail!("the agent is no longer waiting for that answer");
        };
        // Forgotten only once queued: a full queue leaves it open, to be answered again.
        self.queue(line)?;
        match &self.codex {
            Some(state) => lock(state).forget(id),
            None => claude::forget(&self.asks, id),
        }
        if let Some(tx) = self.events_tx.upgrade() {
            let _ = tx.send(Event::PermissionResolved {
                id: id.to_owned(),
                decision,
            });
        }
        Ok(())
    }

    /// Answers the AskUserQuestion ask `id` with each question's picked labels, in question order (a typed "Other"
    /// answer is a label too). Then the session's events carry [`Event::PermissionResolved`] (allowed once). Fails,
    /// sending nothing, when `id` isn't a waiting question or the count doesn't match its questions.
    pub fn answer_questions(&mut self, id: &str, answers: &[Vec<String>]) -> Result<()> {
        let line = match &self.codex {
            Some(state) => lock(state).answer_questions(id, answers),
            None => claude::answer_questions(&self.asks, id, answers).map(|v| v.to_string()),
        };
        let Some(line) = line else {
            bail!("the agent is no longer waiting for these answers");
        };
        self.queue(line)?;
        match &self.codex {
            Some(state) => lock(state).forget(id),
            None => claude::forget(&self.asks, id),
        }
        if let Some(tx) = self.events_tx.upgrade() {
            let _ = tx.send(Event::PermissionResolved {
                id: id.to_owned(),
                decision: Decision::AllowOnce,
            });
        }
        Ok(())
    }

    /// Changes the running agent's permission mode: Claude's at once, Codex's from its next turn. Claude takes Full
    /// access only if it started in it; otherwise this fails and it keeps its mode until it starts again.
    pub fn set_mode(&mut self, mode: Mode) -> Result<()> {
        if let Some(state) = &self.codex {
            lock(state).set_mode(mode);
            return Ok(());
        }
        if mode == Mode::Bypass && self.launched != Mode::Bypass {
            bail!(
                "Full access applies when this agent starts again: stop it, then send a message to continue"
            );
        }
        self.next_request += 1;
        let request_id = format!("skyelab-{}", self.next_request);
        self.write(&json!({
            "type": "control_request",
            "request_id": request_id,
            "request": { "subtype": "set_permission_mode", "mode": mode.claude() },
        }))
    }

    /// What the agent wrote to stderr since the last call (the newest [`STDERR_TAIL`] bytes of it).
    pub fn take_stderr(&self) -> String {
        let bytes = std::mem::take(&mut *self.stderr.lock().unwrap_or_else(|e| e.into_inner()));
        String::from_utf8_lossy(&bytes).into_owned()
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
