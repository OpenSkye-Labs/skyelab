use std::{borrow::Cow, collections::HashMap, path::PathBuf, process::ExitStatus, sync::Arc};

use alacritty_terminal::{
    event::{EventListener, WindowSize},
    event_loop::{EventLoop, EventLoopSender, Msg},
    grid::{Dimensions, Scroll},
    index::{Column, Line, Point, Side},
    selection::{Selection, SelectionType},
    sync::FairMutex,
    term::{Config, Term, cell::Flags, test::TermSize},
    tty,
};
use anyhow::Context as _;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};

use crate::links::{LinkKind, find_links};
use crate::probe::Probe;

pub use alacritty_terminal::event::Event;

/// Lines of scrollback kept above the screen.
const HISTORY: usize = 10_000;

/// A link under a cell: its text and its grid span (first and last cell, inclusive).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub kind: LinkKind,
    pub text: String,
    pub start: Point,
    pub end: Point,
}

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
    cwd: PathBuf,
    /// What the tab title and the close check ask about the shell; `None` when its pid couldn't be read.
    probe: Option<Probe>,
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
        let (program, args, env) = launch(opts.command, opts.env, &parent_names(), cfg!(windows));
        let cwd = opts.cwd.clone();
        let options = tty::Options {
            shell: Some(tty::Shell::new(program, args)),
            working_directory: Some(opts.cwd),
            drain_on_exit: true,
            env,
            #[cfg(windows)]
            escape_args: true,
        };
        let window_size = window_size(opts.cols, opts.lines, 0.0, 0.0);
        // The shell is a child like any other: started under the process-wide spawn lock.
        let (pty, slave, probe) =
            skyelab_core::process::exclusive(|| open_pty(&options, window_size))
                .context("spawning the terminal pty")?;
        let config = Config {
            scrolling_history: HISTORY,
            ..Config::default()
        };
        let term = Term::new(
            config,
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
            cwd,
            probe,
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

    /// Ends the shell now (alacritty hangs it up as its loop stops), not when the last frame lets go of the view.
    pub fn shutdown(&self) {
        let _ = self.pty.send(Msg::Shutdown);
    }

    /// A handle that asks what the shell runs and where, from any thread. `None` once the shell has exited.
    pub fn probe(&self) -> Option<Probe> {
        self.exit.is_none().then(|| self.probe.clone()).flatten()
    }

    /// The folder the terminal started in; relative path links resolve against it.
    pub fn cwd(&self) -> &std::path::Path {
        &self.cwd
    }

    /// Moves the view through the scrollback; `Delta(n > 0)` goes up into history.
    pub fn scroll(&mut self, scroll: Scroll) {
        self.term.lock().scroll_display(scroll);
    }

    /// How many lines the view is scrolled up from the bottom.
    pub fn display_offset(&self) -> usize {
        self.term().grid().display_offset()
    }

    /// Lines of scrollback above the screen.
    pub fn history_size(&self) -> usize {
        self.term().grid().history_size()
    }

    /// Starts a selection at a viewport cell (row 0 is the top visible row).
    pub fn select(&mut self, kind: SelectionType, row: usize, col: usize, side: Side) {
        let mut term = self.term.lock();
        let at = viewport_point(&term, row, col);
        term.selection = Some(Selection::new(kind, at, side));
    }

    /// Moves the end of the current selection to a viewport cell.
    pub fn extend_selection(&mut self, row: usize, col: usize, side: Side) {
        let mut term = self.term.lock();
        let at = viewport_point(&term, row, col);
        if let Some(s) = term.selection.as_mut() {
            s.update(at, side);
        }
    }

    pub fn clear_selection(&mut self) {
        self.term.lock().selection = None;
    }

    /// The selected text, `None` when nothing (or only an empty span) is selected.
    pub fn selection_text(&self) -> Option<String> {
        self.term().selection_to_string().filter(|s| !s.is_empty())
    }

    /// The link at a viewport cell, if any. Scans the (soft-wrapped) line holding the cell.
    pub fn link_at(&self, row: usize, col: usize) -> Option<Link> {
        let term = self.term();
        let at = viewport_point(&term, row, col);
        let (first, last) = (term.line_search_left(at), term.line_search_right(at));
        // One entry per char of the line's text: the cell it came from.
        let mut text = String::new();
        let mut cells = Vec::new();
        let grid = term.grid();
        let mut p = first;
        loop {
            let cell = &grid[p];
            if !cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
            {
                // Concealed text can't be a link: the user can't see what they'd click.
                text.push(if cell.flags.contains(Flags::HIDDEN) {
                    ' '
                } else {
                    cell.c
                });
                cells.push(p);
            }
            // `add` clamps at the grid's last cell instead of moving past it.
            if p == last {
                break;
            }
            p = p.add(&*term, alacritty_terminal::index::Boundary::Grid, 1);
        }
        let here = cells.iter().rposition(|c| *c <= at)?;
        let link = find_links(&text)
            .into_iter()
            .find(|l| (l.start..l.end).contains(&here))?;
        Some(Link {
            kind: link.kind,
            text: text
                .chars()
                .skip(link.start)
                .take(link.end - link.start)
                .collect(),
            start: cells[link.start],
            end: cells[link.end - 1],
        })
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

/// A viewport cell as a grid point, clamped to the grid.
fn viewport_point<T>(term: &Term<T>, row: usize, col: usize) -> Point {
    let row = row.min(term.screen_lines().saturating_sub(1));
    let col = col.min(term.columns().saturating_sub(1));
    Point::new(
        Line(row as i32 - term.grid().display_offset() as i32),
        Column(col),
    )
}

/// The PTY plus a parent-held fd of its slave side (`()` on Windows), and the shell's probe.
#[cfg(unix)]
fn open_pty(
    options: &tty::Options,
    size: WindowSize,
) -> std::io::Result<(tty::Pty, Slave, Option<Probe>)> {
    let size = rustix_openpty::rustix::termios::Winsize {
        ws_row: size.num_lines,
        ws_col: size.num_cols,
        ws_xpixel: size.num_cols.saturating_mul(size.cell_width),
        ws_ypixel: size.num_lines.saturating_mul(size.cell_height),
    };
    let fds = rustix_openpty::openpty(None, Some(&size))?;
    let slave = fds.user.try_clone()?;
    let controller = Arc::new(fds.controller.try_clone()?);
    let pty = tty::from_fd(options, 0, fds.controller, fds.user)?;
    let probe = Probe {
        pid: pty.child().id(),
        controller,
    };
    Ok((pty, slave, Some(probe)))
}

#[cfg(windows)]
fn open_pty(
    options: &tty::Options,
    size: WindowSize,
) -> std::io::Result<(tty::Pty, Slave, Option<Probe>)> {
    let pty = tty::new(options, size, 0)?;
    let probe = pty
        .child_watcher()
        .pid()
        .map(|pid| Probe { pid: pid.get() });
    Ok((pty, (), probe))
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

/// The names of this process's env vars, which [`launch`] filters.
// shortcut: a var whose name isn't UTF-8 can't be named to `env -u` and passes through; none of the credential
// vars is such a name. Upgrade if alacritty takes OsString args.
fn parent_names() -> Vec<String> {
    std::env::vars_os()
        .filter_map(|(k, _)| k.into_string().ok())
        .collect()
}

/// The child's program, args and env: the terminal-allowed part of the parent's env (`skyelab_core::env`, the
/// agents' allowlist plus shell vars), the caller's env (the bound account), and `TERM`/`COLORTERM`. Every other
/// var in `parent` is removed: unix runs the command through `env -u ..`. On Windows alacritty adds every inherited
/// var the env map doesn't name, and has no way to remove one, so each is overridden with an empty value instead.
/// `windows` picks the branch, so tests cover both on any OS.
fn launch(
    command: Option<(String, Vec<String>)>,
    mut env: HashMap<String, String>,
    parent: &[String],
    windows: bool,
) -> (String, Vec<String>, HashMap<String, String>) {
    env.entry("TERM".into()).or_insert("xterm-256color".into());
    env.entry("COLORTERM".into()).or_insert("truecolor".into());
    let (program, args) = command.unwrap_or_else(default_command);
    let unset: Vec<String> = parent
        .iter()
        // Windows names are case-insensitive, so `Gh_Config_Dir` in the parent is the caller's `GH_CONFIG_DIR`.
        .filter(|v| {
            !env.keys()
                .any(|k| k == *v || (windows && k.eq_ignore_ascii_case(v)))
        })
        .filter(|v| !skyelab_core::env::terminal_allowed(v.as_ref(), windows))
        .cloned()
        .collect();
    if windows {
        // shortcut: on Windows the child sees VAR="" (alacritty can't remove a var); upgrade when its Options can.
        for v in unset {
            env.insert(v, String::new());
        }
        return (program, args, env);
    }
    // `--` so a program name with a leading `-` is never read as an env option. env(1) still reads a program name
    // containing `=` as an assignment; no caller passes one.
    let mut wrapped = Vec::new();
    for v in unset {
        env.remove(&v);
        wrapped.extend(["-u".to_string(), v]);
    }
    wrapped.extend(["--".into(), program]);
    wrapped.extend(args);
    ("/usr/bin/env".to_string(), wrapped, env)
}

/// The shell a terminal runs when the caller names no command.
#[cfg(not(feature = "test-support"))]
fn default_command() -> (String, Vec<String>) {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| default_shell().into());
    (shell, login_args())
}

/// Tests (feature `test-support`) get a shell that reads no startup file, so a run never executes the developer's
/// own rc (it can start daemons or touch credentials). `sh` without `-l` reads only `$ENV`, which [`launch`] drops;
/// PowerShell's profile is skipped by `-NoProfile`.
#[cfg(feature = "test-support")]
fn default_command() -> (String, Vec<String>) {
    if cfg!(windows) {
        (
            "powershell.exe".into(),
            vec!["-NoLogo".into(), "-NoProfile".into()],
        )
    } else {
        ("/bin/sh".into(), vec![])
    }
}

#[cfg(not(feature = "test-support"))]
fn default_shell() -> &'static str {
    if cfg!(windows) {
        "powershell.exe"
    } else {
        "/bin/sh"
    }
}

#[cfg(not(feature = "test-support"))]
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
        let (mut pty, _slave, _) = open_pty(&options, window_size(80, 24, 0.0, 0.0)).expect("pty");
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
        for windows in [false, true] {
            config_dir_case(windows);
        }
    }

    fn config_dir_case(windows: bool) {
        let cmd = || Some(("prog".to_string(), vec!["a".to_string()]));
        let parent = [CLAUDE_CONFIG_DIR.to_string()];
        let (program, args, env) = launch(cmd(), HashMap::new(), &parent, windows);
        assert_eq!(env["TERM"], "xterm-256color");
        if windows {
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
        let (_, args, env) = launch(cmd(), account, &parent, windows);
        assert!(!args.iter().any(|a| a == CLAUDE_CONFIG_DIR), "{args:?}");
        assert_eq!(env[CLAUDE_CONFIG_DIR], "/acct");
    }
}

#[cfg(test)]
#[path = "codex_env_test.rs"]
mod codex_env_test;

#[cfg(test)]
#[path = "env_policy_test.rs"]
mod env_policy_test;

#[cfg(test)]
#[path = "account_env_platform_test.rs"]
mod account_env_platform_test;
