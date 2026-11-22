use std::{borrow::Cow, collections::HashMap, path::PathBuf, process::ExitStatus, sync::Arc};

use alacritty_terminal::{
    event::{EventListener, WindowSize},
    event_loop::{EventLoop, EventLoopSender, Msg},
    grid::Dimensions,
    sync::FairMutex,
    term::{Config, Term, test::TermSize},
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
}

impl Terminal {
    /// Spawns the child. Every event from the PTY thread arrives on the returned receiver;
    /// pass each to [`Terminal::handle_event`].
    pub fn spawn(opts: SpawnOptions) -> anyhow::Result<(Self, UnboundedReceiver<Event>)> {
        let (tx, rx) = unbounded();
        let listener = Listener(tx);
        let mut env = opts.env;
        env.entry("TERM".into()).or_insert("xterm-256color".into());
        env.entry("COLORTERM".into()).or_insert("truecolor".into());
        let options = tty::Options {
            shell: Some(shell(opts.command)),
            working_directory: Some(opts.cwd),
            drain_on_exit: true,
            env,
            #[cfg(windows)]
            escape_args: true,
        };
        let window_size = window_size(opts.cols, opts.lines, 0.0, 0.0);
        let pty = tty::new(&options, window_size, 0).context("spawning the terminal pty")?;
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
            if let Some(line) = lines.get_mut(row) {
                line.push(cell.c);
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

fn window_size(cols: u16, lines: u16, cell_width: f32, cell_height: f32) -> WindowSize {
    WindowSize {
        num_cols: cols,
        num_lines: lines,
        cell_width: cell_width as u16,
        cell_height: cell_height as u16,
    }
}

/// The child's command line. On unix it goes through `env -u CLAUDE_CONFIG_DIR` so the parent's account never
/// leaks into the terminal; the caller's env still sets it when the folder is bound.
fn shell(command: Option<(String, Vec<String>)>) -> tty::Shell {
    let (program, args) = command.unwrap_or_else(|| {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| default_shell().into());
        (shell, login_args())
    });
    #[cfg(unix)]
    let (program, args) = {
        let mut wrapped = vec!["-u".into(), "CLAUDE_CONFIG_DIR".into(), program];
        wrapped.extend(args);
        ("/usr/bin/env".to_string(), wrapped)
    };
    // shortcut: on Windows an inherited CLAUDE_CONFIG_DIR is not cleared; fix when Windows terminals ship.
    tty::Shell::new(program, args)
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
