//! Runs agent sessions (Claude Code, Codex, Cursor, or Antigravity via ACP) under the account bound to their
//! folder. No GPUI, no tokio: a std reader thread feeds a flume channel, which GPUI's executor can await.

mod acp;
mod acp_asks;
mod acp_probe;
mod acp_rpc;
mod acp_updates;
pub mod antigravity;
mod attach;
mod claude;
mod codex;
mod cursor;
mod event;
mod git_text;
pub mod handoff;
pub mod import;
mod launch;
mod login_status;
mod mcp_user;
mod pipes;
mod prompts;
mod reap;
pub mod thread_list;
pub mod timeline;
mod title;
pub mod turn_result;
mod usage_limits;
mod version;
pub use cursor::{resolve_cursor_bin, resolve_cursor_bin_in};
pub use git_text::{
    MAX_SUBJECT, clean_commit_message, clean_pr, commit_prompt, generate_commit_message,
    generate_pr, pr_prompt,
};
pub use launch::resolve_bin;
pub use login_status::{Login, login_status};
use pipes::{read_events, read_stderr, write_lines};
pub use title::{clean_title, generate_title, one_shot, title_prompt};
pub use usage_limits::{Limits, usage_limits};
pub use version::agent_version;
#[cfg(any(unix, windows))]
pub mod mcp;
mod models;
mod permission;

pub use attach::{Attachment, Kind as AttachmentKind, MAX_ATTACHMENT_BYTES, MAX_ATTACHMENTS};
pub use claude::{PLAN_TOOL, QUESTION_TOOL};
pub use event::{
    Block, DeltaKind, Event, McpServer, TODO_TOOL, Todo, TodoStatus, Usage, session_gone, todos,
    turn_cost,
};
pub use models::{
    CLAUDE_EFFORTS, CLAUDE_MODELS, ModelInfo, account_default_model, list_antigravity_models,
    list_codex_models, list_cursor_models, model_label,
};
pub use permission::{Decision, Mode, summary};

use anyhow::{Context as _, Result, bail};
use serde_json::Value;
use skyelab_core::{Account, AccountsConfig, Provider};
use std::path::Path;
use std::process::{ExitStatus, Stdio};
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
    child: skyelab_core::process::Owned,
    /// Set once the child is reaped. Its pid may then belong to an unrelated process.
    status: Option<ExitStatus>,
    stdin: flume::Sender<String>,
    events: flume::Receiver<Event>,
    next_request: u64,
    /// Set for Codex: the protocol state the reader thread shares.
    codex: Option<Arc<Mutex<codex::Codex>>>,
    /// Set for ACP providers (Antigravity): the protocol state the reader thread shares.
    acp: Option<Arc<Mutex<acp::Acp>>>,
    /// Set for Cursor: per-turn stream state shared with the reader thread.
    cursor: Option<Arc<Mutex<cursor::CursorState>>>,
    sent: claude::Sent,
    asks: claude::Asks,
    /// The mode it started in: Claude takes Full access later only if it started with it.
    launched: Mode,
    /// Debug mode's live count of agent processes.
    _live: skyelab_core::debug::Live,
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
    /// `--model` for Claude (an alias or a full id), `model` for Codex. `None`: the account's own default.
    pub model: Option<&'a str>,
    /// `--effort` for Claude, the turn's `effort` for Codex. `None`: the default.
    pub effort: Option<&'a str>,
    /// Files sent with the first prompt.
    pub attachments: &'a [Attachment],
    /// Which tool calls go ahead without asking. An ask arrives as [`Event::Permission`].
    pub mode: Mode,
}

/// Starts the account's agent (`claude`, `codex`, `cursor-agent`, or `agy`) per `launch` and sends its prompt as the
/// first turn.
pub fn start(config: &AccountsConfig, launch: &Launch) -> Result<Session> {
    let bin = match launch.account.provider {
        Provider::Claude => std::path::PathBuf::from("claude"),
        Provider::Codex => std::path::PathBuf::from("codex"),
        Provider::Cursor => std::path::PathBuf::from("cursor-agent"),
        Provider::Antigravity => antigravity::default_bin(),
    };
    start_with_bin(&bin, config, launch)
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
        mode,
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
    let cursor_prompt = match account.provider {
        Provider::Claude => None,
        Provider::Codex => {
            attach::codex_input(prompt, attachments)?; // refused before anything starts (a PDF)
            None
        }
        Provider::Cursor => Some(attach::cursor_prompt(prompt, attachments)?),
        Provider::Antigravity => {
            attach::acp_prompt_blocks(prompt, attachments)?; // refused before anything starts (a PDF)
            None
        }
    };
    let (mut cmd, bin, dir) = launch::agent_command(bin, config, account, dir)?;
    let dir = &dir;
    // After the folder was checked against `account`: only the bound account's own config is read.
    let (report, deny_mcp) = mcp_user::launch_report(
        (account.provider == Provider::Claude).then_some(account.config_dir.as_path()),
        mcp_config,
    );
    // Ask ids carry this, so one stored by an earlier process never names an ask of this one.
    let nonce = format!("{}-", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    enum Driver {
        Claude,
        Codex(Arc<Mutex<codex::Codex>>, Vec<String>),
        Cursor(Arc<Mutex<cursor::CursorState>>),
        Acp(Arc<Mutex<acp::Acp>>, Vec<String>),
    }
    let driver = match account.provider {
        Provider::Claude => {
            launch::claude_args(&mut cmd, mcp_config, deny_mcp.as_deref(), resume, mode);
            cmd.args(model.into_iter().flat_map(|m| ["--model", m]))
                .args(effort.into_iter().flat_map(|e| ["--effort", e]));
            Driver::Claude
        }
        Provider::Codex => {
            cmd.args(codex::args(dir)?);
            let mcp = mcp_config.map(codex::mcp_servers).transpose()?;
            let (mut state, opening) = codex::Codex::new(
                dir,
                resume,
                mcp,
                mode,
                codex::Pick {
                    model: model.map(str::to_owned),
                    effort: effort.map(str::to_owned),
                },
            );
            state.nonce = nonce.clone();
            Driver::Codex(Arc::new(Mutex::new(state)), opening)
        }
        Provider::Cursor => {
            cursor::args(&mut cmd, dir, resume, model, mode);
            Driver::Cursor(Arc::new(Mutex::new(cursor::CursorState::new(
                resume.is_some(),
                model,
            ))))
        }
        Provider::Antigravity => {
            antigravity::ensure_profile_dirs(&account.config_dir)?;
            cmd.envs(antigravity::isolated_env())
                .args(antigravity::acp_args(
                    &account.config_dir,
                    mode == Mode::Bypass,
                ));
            let mcp = mcp_config
                .map(acp_probe::mcp_servers_from_grant)
                .transpose()?
                .unwrap_or_default();
            let (state, opening) = acp::Acp::new(
                antigravity::config(),
                dir,
                resume,
                mcp,
                mode,
                acp::Pick {
                    model: model.map(str::to_owned),
                    effort: effort.map(str::to_owned),
                },
                nonce.clone(),
            );
            Driver::Acp(Arc::new(Mutex::new(state)), opening)
        }
    };
    cmd.current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = skyelab_core::process::spawn(&mut cmd)
        .with_context(|| format!("starting {} in {}", bin.display(), dir.display()))?;
    let stdout = child.stdout.take().context("child stdout was piped")?;
    let stdin = child.stdin.take().context("child stdin was piped")?;
    let stderr = child.stderr.take().context("child stderr was piped")?;

    let (tx, events) = flume::unbounded();
    let _ = tx.send(report); // `events` is held below, so this can't fail
    let (stdin_tx, stdin_rx) = flume::bounded(STDIN_QUEUE);
    let codex_handle = match &driver {
        Driver::Codex(state, _) => Some(state.clone()),
        _ => None,
    };
    let acp_handle = match &driver {
        Driver::Acp(state, _) => Some(state.clone()),
        _ => None,
    };
    let cursor_handle = match &driver {
        Driver::Cursor(state) => Some(state.clone()),
        _ => None,
    };
    // Built before the threads spawn, so every error below still kills and reaps the child on drop.
    let mut session = Session {
        child,
        status: None,
        stdin: stdin_tx.clone(),
        events,
        next_request: 0,
        codex: codex_handle,
        cursor: cursor_handle,
        acp: acp_handle,
        sent: claude::Sent::default(),
        asks: claude::Book::new(nonce),
        launched: mode,
        _live: skyelab_core::debug::Live::new("agent sessions"),
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
    let (parse, eof): (Box<Parse>, Box<dyn FnOnce() -> Vec<Event> + Send>) = match driver {
        Driver::Cursor(state) => {
            let eof_state = state.clone();
            (
                Box::new(move |line, truncated| {
                    if truncated {
                        lock_cursor(&state).on_oversized(line.to_owned())
                    } else {
                        lock_cursor(&state).parse(line)
                    }
                }),
                Box::new(move || lock_cursor(&eof_state).on_eof()),
            )
        }
        Driver::Codex(state, opening) => {
            for line in opening {
                session.queue(line)?;
            }
            let open = state.clone();
            let parse = Box::new(move |line: &str, truncated: bool| {
                if truncated {
                    return vec![Event::Unknown(Value::String(line.to_owned()))];
                }
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
        Driver::Acp(state, opening) => {
            for line in opening {
                session.queue(line)?;
            }
            let open = state.clone();
            let parse = Box::new(move |line: &str, truncated: bool| {
                if truncated {
                    return vec![Event::Unknown(Value::String(line.to_owned()))];
                }
                let Ok(v) = serde_json::from_str::<Value>(line) else {
                    return vec![Event::Unknown(Value::String(line.to_owned()))];
                };
                let (events, replies) = lock_acp(&state).on_line(v);
                for reply in replies {
                    let _ = stdin_tx.try_send(reply);
                }
                events
            });
            (parse, Box::new(move || lock_acp(&open).withdraw_all()))
        }
        Driver::Claude => {
            let (sent, asks) = (session.sent.clone(), session.asks.clone());
            let open = asks.clone();
            (
                Box::new(move |line, truncated| {
                    if truncated {
                        vec![Event::Unknown(Value::String(line.to_owned()))]
                    } else {
                        claude::parse(line, &sent, &asks)
                    }
                }),
                Box::new(move || claude::withdraw_all(&open)),
            )
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
    if let Some(prompt_text) = cursor_prompt {
        // Cursor `-p` reads one prompt from stdin until EOF, then streams its turn and exits.
        drop(stdin_rx);
        std::thread::Builder::new()
            .name("skyelab-agent-writer".into())
            .spawn(move || {
                use std::io::Write as _;
                let mut stdin = stdin;
                let _ = stdin
                    .write_all(prompt_text.as_bytes())
                    .and_then(|()| stdin.flush());
            })
            .context("spawning writer thread")?;
    } else {
        std::thread::Builder::new()
            .name("skyelab-agent-writer".into())
            .spawn(move || write_lines(stdin, &stdin_rx))
            .context("spawning writer thread")?;
        session.send_message(prompt, attachments)?;
    }
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

/// Turns one stdout line into events (and, for Codex, answers it). `truncated` is true when the line exceeded [`MAX_LINE`].
type Parse = dyn FnMut(&str, bool) -> Vec<Event> + Send;

fn lock(state: &Mutex<codex::Codex>) -> std::sync::MutexGuard<'_, codex::Codex> {
    // A panic mid-update leaves plain data behind; the next line still gets handled.
    state.lock().unwrap_or_else(|e| e.into_inner())
}

fn lock_acp(state: &Mutex<acp::Acp>) -> std::sync::MutexGuard<'_, acp::Acp> {
    state.lock().unwrap_or_else(|e| e.into_inner())
}

fn lock_cursor(
    state: &Mutex<cursor::CursorState>,
) -> std::sync::MutexGuard<'_, cursor::CursorState> {
    state.lock().unwrap_or_else(|e| e.into_inner())
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

impl Drop for Session {
    /// Ends the agent's whole tree; dropping `child` then kills and reaps the agent itself, off this thread when slow.
    fn drop(&mut self) {
        if self.status.is_none() {
            self.kill_group();
        }
    }
}

#[cfg(test)]
mod real_claude_test;

mod session_answers;
mod session_control;
