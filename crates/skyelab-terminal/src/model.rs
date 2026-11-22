use std::{borrow::Cow, collections::HashMap, path::PathBuf, process::ExitStatus, sync::Arc};

use alacritty_terminal::{
    event::{EventListener, WindowSize},
    event_loop::{EventLoop, EventLoopSender, Msg},
    grid::Dimensions,
    sync::FairMutex,
    term::{Config, Term, cell::Flags, test::TermSize},
    tty,
};
use anyhow::Context as _;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};

pub use alacritty_terminal::event::Event;

/// Forwards alacritty's events (output arrived, child exited, …) to whoever owns the receiver.
#[derive(Clone)]
pub struct Listener(UnboundedSender<Event>);

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        // The receiver is gone only when the terminal is being dropped; nothing to tell then.
        let _ = self.0.unbounded_send(event);
    }
}

pub struct SpawnOptions {
    pub cwd: PathBuf,
    /// Program and args; `None` runs the user's login shell.
    pub command: Option<(String, Vec<String>)>,
    /// Extra env for the child, on top of the inherited env (callers pass the account env here).
    pub env: HashMap<String, String>,
    pub cols: u16,
    pub lines: u16,
}

/// A PTY-backed terminal: alacritty's `EventLoop` thread reads the PTY into `Term`, we write into it.
pub struct Terminal {
    term: Arc<FairMutex<Term<Listener>>>,
    pty: EventLoopSender,
    exit: Option<ExitStatus>,
    size: (u16, u16),
    /// macOS discards unread PTY output when the last slave fd closes; holding one makes the child's exit wait for
    /// the reader to drain it.
    _slave: Slave,
}

#[cfg(unix)]
type Slave = std::os::fd::OwnedFd;
#[cfg(windows)]
type Slave = ();

impl Terminal {
    /// Spawns the child. Every event from the PTY thread arrives on the returned receiver;
    /// pass each to [`Terminal::handle_event`].
    pub fn spawn(opts: SpawnOptions) -> anyhow::Result<(Self, UnboundedReceiver<Event>)> {
        let (tx, rx) = unbounded();
        let listener = Listener(tx);
        let (program, args, env) = launch(opts.command, opts.env);
        let options = tty::Options {
            shell: Some(tty::Shell::new(program, args)),
            working_directory: Some(opts.cwd),
            drain_on_exit: true,
            env,
            #[cfg(windows)]
            escape_args: true,
        };
        let window_size = window_size(opts.cols, opts.lines, 0.0, 0.0);
        let (pty, slave) = open_pty(&options, window_size).context("spawning the terminal pty")?;
        let term = Term::new(
            Config::default(),
            &TermSize::new(opts.cols.into(), opts.lines.into()),
            listener.clone(),
        );
        let term = Arc::new(FairMutex::new(term));
        let event_loop = EventLoop::new(term.clone(), listener, pty, true, false)
            .context("starting the terminal event loop")?;
        let pty = event_loop.channel();
        // The loop thread exits on `Msg::Shutdown` (sent on drop) or when the child closes the PTY.
        event_loop.spawn();
        let this = Self {
            term,
            pty,
            exit: None,
            size: (opts.cols, opts.lines),
            _slave: slave,
        };
        Ok((this, rx))
    }

    /// Applies one event from the PTY thread: records the exit status and answers terminal queries.
    pub fn handle_event(&mut self, event: &Event) {
        match event {
            Event::ChildExit(status) => self.exit = Some(*status),
            Event::PtyWrite(text) => self.write(text.clone().into_bytes()),
            _ => {}
        }
    }

    pub fn write(&self, bytes: impl Into<Cow<'static, [u8]>>) {
        // Fails only after the loop thread is gone (child exited); input then has nowhere to go.
        let _ = self.pty.send(Msg::Input(bytes.into()));
    }

    /// Resizes the grid and the PTY. Cell size is in pixels and only informs the child.
    pub fn resize(&mut self, cols: u16, lines: u16, cell_width: f32, cell_height: f32) {
        let (cols, lines) = (cols.max(1), lines.max(1));
        if (cols, lines) == self.size {
            return;
        }
        self.size = (cols, lines);
        self.term
            .lock()
            .resize(TermSize::new(cols.into(), lines.into()));
        let _ = self.pty.send(Msg::Resize(window_size(
            cols,
            lines,
            cell_width,
            cell_height,
        )));
    }

    /// `(columns, lines)` of the grid.
    pub fn size(&self) -> (u16, u16) {
        let term = self.term();
        (term.columns() as u16, term.screen_lines() as u16)
    }

    pub fn exit_status(&self) -> Option<ExitStatus> {
        self.exit
    }

    /// Locks the grid. Keep the guard short: the PTY thread waits on it to parse output.
    pub fn term(&self) -> impl std::ops::Deref<Target = Term<Listener>> + '_ {
        self.term.lock()
    }

    /// The visible screen as text, one line per row, trailing blanks trimmed.
    pub fn text(&self) -> String {
        let term = self.term();
        let grid = term.grid();
        let mut lines = vec![String::new(); grid.screen_lines()];
        for cell in grid.display_iter() {
            let row = (cell.point.line.0 + grid.display_offset() as i32) as usize;
            if cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }
            if let Some(line) = lines.get_mut(row) {
                line.push(cell.c);
                line.extend(cell.zerowidth().into_iter().flatten());
            }
        }
        lines
            .iter()
            .map(|l| l.trim_end())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.pty.send(Msg::Shutdown);
    }
}

/// The PTY plus a parent-held fd of its slave side (`()` on Windows).
#[cfg(unix)]
fn open_pty(options: &tty::Options, size: WindowSize) -> std::io::Result<(tty::Pty, Slave)> {
    let size = rustix_openpty::rustix::termios::Winsize {
        ws_row: size.num_lines,
        ws_col: size.num_cols,
        ws_xpixel: size.num_cols.saturating_mul(size.cell_width),
        ws_ypixel: size.num_lines.saturating_mul(size.cell_height),
    };
    let fds = rustix_openpty::openpty(None, Some(&size))?;
    let slave = fds.user.try_clone()?;
    Ok((tty::from_fd(options, 0, fds.controller, fds.user)?, slave))
}

#[cfg(windows)]
fn open_pty(options: &tty::Options, size: WindowSize) -> std::io::Result<(tty::Pty, Slave)> {
    tty::new(options, size, 0).map(|pty| (pty, ()))
}

fn window_size(cols: u16, lines: u16, cell_width: f32, cell_height: f32) -> WindowSize {
    WindowSize {
        num_cols: cols,
        num_lines: lines,
        cell_width: cell_width as u16,
        cell_height: cell_height as u16,
    }
}

#[cfg(test)]
const CLAUDE_CONFIG_DIR: &str = "CLAUDE_CONFIG_DIR";
/// Vars that pick an account or carry a login. A terminal gets one only when the caller set it (the account env).
const ACCOUNT_VARS: [&str; 6] = [
    "CLAUDE_CONFIG_DIR",
    "GH_CONFIG_DIR",
    "GH_TOKEN",
    "GITHUB_TOKEN",
    "GH_ENTERPRISE_TOKEN",
    "GITHUB_ENTERPRISE_TOKEN",
];

/// The child's program, args and env: the caller's env plus `TERM`/`COLORTERM`, and none of the parent's
/// [`ACCOUNT_VARS`] the caller didn't set. Unix runs the command through `env -u ..`. On Windows alacritty adds
/// every inherited var the env map doesn't name, and has no way to remove one, so each is overridden with an empty
/// value instead.
fn launch(
    command: Option<(String, Vec<String>)>,
    mut env: HashMap<String, String>,
) -> (String, Vec<String>, HashMap<String, String>) {
    env.entry("TERM".into()).or_insert("xterm-256color".into());
    env.entry("COLORTERM".into()).or_insert("truecolor".into());
    let (program, args) = command.unwrap_or_else(|| {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| default_shell().into());
        (shell, login_args())
    });
    let unset: Vec<&str> = ACCOUNT_VARS
        .into_iter()
        .filter(|v| env.get(*v).is_none_or(|v| v.is_empty()))
        .collect();
    if cfg!(windows) {
        // shortcut: the child sees VAR="" rather than no var. A tool that falls back only when it is
        // absent (Node's `env.X ?? default`) would resolve "" against the cwd. Upgrade when alacritty's Options can
        // remove a var, or by wrapping the command in a shell that unsets it.
        for v in unset {
            env.insert(v.into(), String::new());
        }
        return (program, args, env);
    }
    // `--` so a program name with a leading `-` is never read as an env option. env(1) still reads a program name
    // containing `=` as an assignment; no caller passes one.
    let mut wrapped = Vec::new();
    for v in unset {
        wrapped.extend(["-u".to_string(), v.to_string()]);
        env.remove(v);
    }
    wrapped.extend(["--".into(), program]);
    wrapped.extend(args);
    ("/usr/bin/env".to_string(), wrapped, env)
}

fn default_shell() -> &'static str {
    if cfg!(windows) {
        "powershell.exe"
    } else {
        "/bin/sh"
    }
}

fn login_args() -> Vec<String> {
    if cfg!(windows) {
        vec![]
    } else {
        vec!["-l".into()]
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::{io::Read, time::Duration};

    use alacritty_terminal::tty::{ChildEvent, EventedPty, EventedReadWrite};

    use super::*;

    #[test]
    fn output_outlives_a_child_that_exits_before_the_first_read() {
        let options = tty::Options {
            shell: Some(tty::Shell::new("printf".into(), vec!["XYZ".into()])),
            working_directory: None,
            drain_on_exit: true,
            env: HashMap::new(),
        };
        let (mut pty, _slave) = open_pty(&options, window_size(80, 24, 0.0, 0.0)).expect("pty");
        // Read nothing until printf is long done: without the held slave its output is gone by then.
        std::thread::sleep(Duration::from_secs(2));
        let mut buf = [0u8; 64];
        let n = pty.reader().read(&mut buf).expect("read after exit");
        let mut waited = 0;
        while !matches!(pty.next_child_event(), Some(ChildEvent::Exited(_))) {
            waited += 1;
            assert!(waited < 1000, "printf never exited");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(&buf[..n], b"XYZ");
    }
}

#[cfg(test)]
#[path = "gh_env_test.rs"]
mod gh_env_test;

#[cfg(test)]
mod env_tests {
    use super::*;

    #[test]
    fn child_env_never_inherits_the_parents_config_dir() {
        let cmd = || Some(("prog".to_string(), vec!["a".to_string()]));
        let (program, args, env) = launch(cmd(), HashMap::new());
        assert_eq!(env["TERM"], "xterm-256color");
        if cfg!(windows) {
            assert_eq!(
                (program.as_str(), args.as_slice()),
                ("prog", &["a".to_string()][..])
            );
            assert_eq!(env.get(CLAUDE_CONFIG_DIR).map(String::as_str), Some(""));
        } else {
            assert_eq!(program, "/usr/bin/env");
            assert_eq!(args[..2], ["-u", CLAUDE_CONFIG_DIR]);
            assert_eq!(args[args.len() - 3..], ["--", "prog", "a"]);
            assert!(!env.contains_key(CLAUDE_CONFIG_DIR));
        }

        let account = HashMap::from([(CLAUDE_CONFIG_DIR.to_string(), "/acct".to_string())]);
        let (_, args, env) = launch(cmd(), account);
        assert!(!args.iter().any(|a| a == CLAUDE_CONFIG_DIR), "{args:?}");
        assert_eq!(env[CLAUDE_CONFIG_DIR], "/acct");
    }
}
