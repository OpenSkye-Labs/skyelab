//! Runs Claude Code sessions under the account bound to their folder. No GPUI, no tokio:
//! a std reader thread feeds a flume channel, which GPUI's executor can await.

mod event;

pub use event::{Block, Event};

use anyhow::{Context as _, Result};
use serde_json::json;
use skyelab_core::AccountsConfig;
use std::io::{BufRead as _, BufReader, Write as _};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};

/// Parent env that would pick an account or credential other than the bound one.
const ACCOUNT_ENV: &[&str] = &[
    "CLAUDE_CONFIG_DIR",
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "CLAUDE_CODE_OAUTH_TOKEN",
    "ANTHROPIC_BASE_URL",
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
];

/// A running `claude` process. Dropping it kills the process.
///
/// Writes to stdin block if `claude` stops reading, so call `send_user_message` and `interrupt`
/// off the UI thread (e.g. GPUI's background executor).
pub struct Session {
    child: Child,
    stdin: ChildStdin,
    events: flume::Receiver<Event>,
    next_request: u64,
}

/// Starts `claude` in `dir` with the account bound to it, and sends `prompt` as the first turn.
pub fn start(config: &AccountsConfig, dir: &Path, prompt: &str) -> Result<Session> {
    start_with_bin(Path::new("claude"), config, dir, prompt)
}

/// [`start`] with another binary in place of `claude`, so tests can run a fake.
pub fn start_with_bin(
    bin: &Path,
    config: &AccountsConfig,
    dir: &Path,
    prompt: &str,
) -> Result<Session> {
    let env = config.launch_env(dir)?;
    let mut cmd = Command::new(bin);
    // Never inherit the parent's account: clear it, then set the bound one.
    for var in ACCOUNT_ENV {
        cmd.env_remove(var);
    }
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0); // so kill() reaches its children too
    cmd.args([
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
    ])
    .current_dir(dir)
    .envs(env)
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
    std::thread::Builder::new()
        .name("skyelab-agent-reader".into())
        .spawn(move || {
            let mut stdout = BufReader::new(stdout);
            let mut buf = Vec::new();
            while matches!(stdout.read_until(b'\n', &mut buf), Ok(n) if n > 0) {
                let line = String::from_utf8_lossy(&buf);
                if !line.trim().is_empty() && tx.send(Event::parse(line.trim_end())).is_err() {
                    break;
                }
                buf.clear();
            }
        })
        .context("spawning reader thread")?;

    let mut session = Session {
        child,
        stdin,
        events,
        next_request: 0,
    };
    session.send_user_message(prompt)?;
    Ok(session)
}

impl Session {
    /// Events in the order `claude` printed them. The channel closes when the process exits.
    pub fn events(&self) -> &flume::Receiver<Event> {
        &self.events
    }

    /// Sends a follow-up user turn.
    pub fn send_user_message(&mut self, text: &str) -> Result<()> {
        self.write(&json!({
            "type": "user",
            "message": { "role": "user", "content": text },
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

    /// Kills the process (and on unix its whole process group) and waits for it.
    pub fn kill(&mut self) -> Result<()> {
        // `unsafe` is denied, so no libc::killpg: the kill binary signals the group instead.
        #[cfg(unix)]
        let _ = Command::new("kill")
            .args(["-KILL", "--", &format!("-{}", self.child.id())])
            .stderr(Stdio::null())
            .status();
        // An already-exited child makes kill() error; wait() still reaps it.
        let _ = self.child.kill();
        self.child.wait().context("waiting for claude to exit")?;
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
