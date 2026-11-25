//! The user's login-shell env for an app opened from the Dock, Finder or a desktop launcher, which gets only the
//! system PATH (`/usr/bin:/bin:/usr/sbin:/sbin` on macOS), so `claude` in `~/.local/bin` isn't found. Release builds
//! run `$SHELL -ilc` once at startup, off the UI thread, and keep PATH plus [`NAMES`]; [`crate::env`] adds them to
//! what children may inherit, still through its allowlist. Debug builds and tests never start a shell.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// What is read from the login shell. Each must also pass `env::terminal_allowed` (agents get the narrower
/// `agent_allowed` part); [`capture`] drops anything that doesn't. Never CLAUDE_CONFIG_DIR, CODEX_HOME or
/// GH_CONFIG_DIR: an account sets those.
pub const NAMES: &[&str] = &[
    "PATH",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "HOMEBREW_PREFIX",
    "HOMEBREW_CELLAR",
    "HOMEBREW_REPOSITORY",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "NODE_EXTRA_CA_CERTS",
    "JAVA_HOME",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_RUNTIME_DIR",
    "SSH_AUTH_SOCK",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "DBUS_SESSION_BUS_ADDRESS",
];

/// Where agent CLIs usually live, searched (after PATH) until the login shell answers or when it never does.
const FALLBACK_DIRS: &[&str] = &[
    "~/.local/bin",
    "/opt/homebrew/bin",
    "/usr/local/bin",
    "~/.npm-global/bin",
    "~/.volta/bin",
    "~/.bun/bin",
    "~/.cargo/bin",
];

const TIMEOUT: Duration = Duration::from_secs(5);
/// A profile can be slow while the machine boots: one longer wait, once.
const SLOW_TIMEOUT: Duration = Duration::from_secs(20);

static STARTED: AtomicBool = AtomicBool::new(false);
static CAPTURED: OnceLock<Vec<(OsString, OsString)>> = OnceLock::new();

/// Starts the capture on its own thread (release builds, or debug ones with `SKYELAB_VERIFY_LOGIN_SHELL` set for a
/// live check; unix; not launched from a terminal). Call it before any child spawns.
pub fn start() {
    use std::io::IsTerminal as _;
    let debug_off =
        cfg!(debug_assertions) && std::env::var_os("SKYELAB_VERIFY_LOGIN_SHELL").is_none();
    // A terminal launch already has the shell's env, and an interactive shell would take over that terminal.
    if debug_off || cfg!(windows) || std::io::stdin().is_terminal() {
        return;
    }
    STARTED.store(true, Ordering::SeqCst);
    let spawned = std::thread::Builder::new()
        .name("login-shell-env".into())
        .spawn(|| {
            let _ = CAPTURED.set(capture(&shells(), TIMEOUT, SLOW_TIMEOUT));
        });
    if spawned.is_err() {
        let _ = CAPTURED.set(Vec::new());
    }
}

/// Blocks until the capture [`start`] began has landed; returns at once when none runs. For checks that run
/// early off the UI thread (the sign-in check), so they don't report a CLI missing that the shell's PATH has.
pub fn wait() {
    if STARTED.load(Ordering::SeqCst) {
        CAPTURED.wait();
    }
}

/// The captured vars (empty until the capture lands, and in debug builds), and the fallback dirs while no PATH
/// was captured.
pub(crate) fn current() -> (&'static [(OsString, OsString)], Vec<PathBuf>) {
    if !STARTED.load(Ordering::SeqCst) {
        return (&[], Vec::new());
    }
    let captured = CAPTURED.get().map_or(&[][..], Vec::as_slice);
    let fallback = if captured.iter().any(|(k, _)| k == "PATH") {
        Vec::new()
    } else {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        fallback_dirs(home.as_deref())
    };
    (captured, fallback)
}

fn fallback_dirs(home: Option<&Path>) -> Vec<PathBuf> {
    FALLBACK_DIRS
        .iter()
        .filter_map(|d| match d.strip_prefix("~/") {
            Some(rest) => home.map(|h| h.join(rest)),
            None => Some(PathBuf::from(d)),
        })
        .filter(|d| d.is_absolute() && d.is_dir())
        .collect()
}

/// `$SHELL`, then zsh, then sh.
fn shells() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::env::var_os("SHELL")
        .map(PathBuf::from)
        .filter(|s| s.is_absolute())
        .into_iter()
        .collect();
    for s in ["/bin/zsh", "/bin/sh"] {
        if !out.iter().any(|o| o == Path::new(s)) {
            out.push(s.into());
        }
    }
    out
}

/// Asks each of `shells` in turn until one prints a PATH: `timeout` each, plus one `slow` retry for the first that
/// times out. Only allowlisted [`NAMES`] are kept.
pub fn capture(shells: &[PathBuf], timeout: Duration, slow: Duration) -> Vec<(OsString, OsString)> {
    let mut slow_left = true;
    for shell in shells {
        let mut limit = timeout;
        loop {
            match run(shell, limit) {
                Ok(out) => {
                    let vars = parse(&out);
                    if vars.iter().any(|(k, _)| k == "PATH") {
                        return vars;
                    }
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut && slow_left => {
                    slow_left = false;
                    limit = slow;
                }
                Err(_) => break,
            }
        }
    }
    Vec::new()
}

fn run(shell: &Path, limit: Duration) -> std::io::Result<String> {
    let mut cmd = Command::new(shell);
    cmd.arg("-ilc")
        .arg(script())
        .current_dir(std::env::var_os("HOME").unwrap_or_else(|| "/".into()))
        .env_clear()
        .envs(crate::env::terminal_vars(std::env::vars_os(), false));
    let out = crate::process::output_within(&mut cmd, None, limit)?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn start_marker(name: &str) -> String {
    format!("__SKYELAB_ENV_{name}_START__")
}

fn end_marker(name: &str) -> String {
    format!("__SKYELAB_ENV_{name}_END__")
}

/// Prints each of [`NAMES`] between its markers, so rc-file chatter around it is ignored.
fn script() -> String {
    NAMES
        .iter()
        .map(|n| {
            format!(
                "printf '\\n%s\\n' '{}'; printenv {n}; printf '%s\\n' '{}'",
                start_marker(n),
                end_marker(n)
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// The [`NAMES`] found between their markers in `out`, non-empty and allowed for terminals.
pub fn parse(out: &str) -> Vec<(OsString, OsString)> {
    NAMES
        .iter()
        .filter(|n| crate::env::terminal_allowed(OsStr::new(n), false))
        .filter_map(|n| {
            let start = start_marker(n);
            let from = out.find(&start)? + start.len();
            let len = out[from..].find(&end_marker(n))?;
            let value = out[from..from + len]
                .strip_prefix('\n')
                .unwrap_or(&out[from..from + len]);
            let value = value.strip_suffix('\n').unwrap_or(value);
            (!value.is_empty()).then(|| (OsString::from(n), OsString::from(value)))
        })
        .collect()
}

/// `process` plus `captured`: PATH is the captured one, then the process's, then `fallback`, each dir once; any
/// other captured var only fills one the process lacks.
pub fn merge(
    process: impl IntoIterator<Item = (OsString, OsString)>,
    captured: &[(OsString, OsString)],
    fallback: &[PathBuf],
) -> Vec<(OsString, OsString)> {
    let mut vars: Vec<(OsString, OsString)> = process.into_iter().collect();
    if captured.is_empty() && fallback.is_empty() {
        return vars;
    }
    let path_of = |vars: &[(OsString, OsString)]| {
        vars.iter()
            .find(|(k, _)| k == "PATH")
            .map(|(_, v)| v.clone())
    };
    let mut dirs: Vec<PathBuf> = Vec::new();
    for p in [path_of(captured), path_of(&vars)].into_iter().flatten() {
        dirs.extend(std::env::split_paths(&p));
    }
    dirs.extend(fallback.iter().cloned());
    let mut seen = std::collections::HashSet::new();
    dirs.retain(|d| seen.insert(d.clone()));
    for (k, v) in captured {
        if k != "PATH" && !vars.iter().any(|(pk, _)| pk == k) {
            vars.push((k.clone(), v.clone()));
        }
    }
    if let Ok(path) = std::env::join_paths(dirs) {
        vars.retain(|(k, _)| k != "PATH");
        vars.push(("PATH".into(), path));
    }
    vars
}

#[cfg(test)]
#[path = "login_shell_test.rs"]
mod tests;
