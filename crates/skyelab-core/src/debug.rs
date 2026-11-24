//! Debug mode (docs/debugging.md): the switch, the debug log of RPC, child processes and MCP calls (a ring in memory
//! plus a JSON-lines file), redaction for what goes in it, and live counts of long-lived objects for leak hunting.
//!
//! Off, every hook here is one atomic load: nothing is formatted, stored or written.

use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};
use std::ffi::OsString;
use std::fs::File;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The env switches, read once at launch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Switches {
    /// `SKYELAB_DEBUG=1`: debug mode on for this run (the overlay opens in the first window).
    pub debug: bool,
    /// `SKYELAB_TRACE=path.json`: write a Chrome/Perfetto trace there.
    pub trace: Option<PathBuf>,
    /// `SKYELAB_DEBUG_RENDERS=1`: print renders per second to stderr.
    pub renders: bool,
}

impl Switches {
    pub fn from_env() -> Self {
        Self::parse(|k| std::env::var(k).ok())
    }

    pub fn parse(get: impl Fn(&str) -> Option<String>) -> Self {
        let truthy = |k| {
            get(k).is_some_and(|v| {
                matches!(
                    v.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "on" | "yes"
                )
            })
        };
        Self {
            debug: truthy("SKYELAB_DEBUG"),
            trace: get("SKYELAB_TRACE")
                .filter(|p| !p.trim().is_empty())
                .map(PathBuf::from),
            renders: truthy("SKYELAB_DEBUG_RENDERS"),
        }
    }
}

static ENABLED: AtomicBool = AtomicBool::new(false);

/// Whether debug mode is on. One relaxed load: cheap enough for every hook.
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// What a log entry is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A call between this app and another machine's daemon, either side.
    Rpc,
    /// A child process: git, gh, an agent, a shell.
    Process,
    /// The Tailscale CLI.
    Tailnet,
    /// A tool call an agent made to SkyeLab's MCP server.
    Mcp,
    /// A slow frame.
    Frame,
    /// A folder watcher (live Changes): started, fell back, stopped.
    Watch,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Rpc => "rpc",
            Kind::Process => "process",
            Kind::Tailnet => "tailnet",
            Kind::Mcp => "mcp",
            Kind::Frame => "frame",
            Kind::Watch => "watch",
        }
    }
}

/// One debug log line. Build it inside [`record`]'s closure, so nothing is formatted while debug mode is off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    /// Unix time in milliseconds, set by [`record`].
    pub at_ms: u64,
    pub kind: Kind,
    /// What ran: `client send_message`, `git status --porcelain`, a tool name. Never a prompt or a secret.
    pub what: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ms: Option<u64>,
    pub ok: bool,
    /// Sizes, a pid, an exit code or a redacted error.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

impl Entry {
    pub fn new(kind: Kind, what: impl Into<String>) -> Self {
        Self {
            at_ms: 0,
            kind,
            what: what.into(),
            ms: None,
            ok: true,
            detail: String::new(),
        }
    }

    pub fn ms(mut self, took: Duration) -> Self {
        self.ms = Some(u64::try_from(took.as_millis()).unwrap_or(u64::MAX));
        self
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = detail.into();
        self
    }

    /// A failure; `why` is redacted.
    pub fn failed(mut self, why: &str) -> Self {
        self.ok = false;
        self.detail = redact_text(why);
        self
    }
}

const RING: usize = 1000;
/// A log file bigger than this at launch starts over.
const MAX_FILE: u64 = 8 << 20;

struct Log {
    ring: VecDeque<Entry>,
    file: Option<File>,
}

static LOG: Mutex<Log> = Mutex::new(Log {
    ring: VecDeque::new(),
    file: None,
});

fn log() -> std::sync::MutexGuard<'static, Log> {
    LOG.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Appends the log to `path` as JSON lines (0600 on unix). A file past 8 MB starts over.
pub fn set_log_file(path: &Path) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let too_big = std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_FILE);
    let mut open = std::fs::OpenOptions::new();
    open.create(true);
    if too_big {
        open.write(true).truncate(true);
    } else {
        open.append(true);
    }
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut open, 0o600);
    log().file = Some(open.open(path)?);
    Ok(())
}

/// Logs the entry `make` builds, when debug mode is on.
pub fn record(make: impl FnOnce() -> Entry) {
    if !enabled() {
        return;
    }
    let mut entry = make();
    entry.at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
    let mut log = log();
    if let Some(file) = log.file.as_mut()
        && let Ok(line) = serde_json::to_string(&entry)
    {
        // shortcut: written under the lock on the caller's thread; move to a writer thread if it ever shows in a trace.
        let _ = writeln!(file, "{line}");
    }
    if log.ring.len() == RING {
        log.ring.pop_front();
    }
    log.ring.push_back(entry);
}

/// The last `n` entries, oldest first.
pub fn recent(n: usize) -> Vec<Entry> {
    let log = log();
    log.ring
        .iter()
        .skip(log.ring.len().saturating_sub(n))
        .cloned()
        .collect()
}

/// The `method` of a JSON-RPC message, without keeping anything else from it (params carry prompts).
pub fn rpc_method(json: &str) -> String {
    #[derive(serde::Deserialize)]
    struct Method {
        method: Option<String>,
    }
    serde_json::from_str::<Method>(json)
        .ok()
        .and_then(|m| m.method)
        .unwrap_or_else(|| "?".into())
}

/// One RPC being timed for the log: `side` is `client` or `daemon`. `None` while debug mode is off.
pub struct RpcCall {
    at: std::time::Instant,
    what: String,
    bytes: usize,
}

impl RpcCall {
    /// Times the request whose JSON text is `json`; only its method name and size are kept.
    pub fn start(side: &str, json: &str) -> Option<Self> {
        enabled().then(|| Self {
            at: std::time::Instant::now(),
            what: format!("{side} {}", rpc_method(json)),
            bytes: json.len(),
        })
    }

    /// Logs the answer: the round trip time, or the error (redacted). It is also an `rpc` event in a trace.
    pub fn done(self, error: Option<&str>) {
        tracing::info!(name: "rpc", what = %self.what, ms = self.at.elapsed().as_millis() as u64, ok = error.is_none());
        record(|| {
            let entry = Entry::new(Kind::Rpc, self.what).ms(self.at.elapsed());
            match error {
                None => entry.detail(format!("{} B", self.bytes)),
                Some(why) => entry.failed(&format!("{} B, {why}", self.bytes)),
            }
        });
    }
}

/// Logs a started child: program and redacted args, never its env. The Tailscale CLI is logged as tailnet.
pub fn record_spawn(cmd: &std::process::Command, started: &std::io::Result<std::process::Child>) {
    record(|| {
        let entry = Entry::new(program_kind(cmd), command_line(cmd));
        match started {
            Ok(child) => entry.detail(format!("pid {}", child.id())),
            Err(e) => entry.failed(&e.to_string()),
        }
    });
}

/// A short-lived child being timed for the log; `None` from [`Started::now`] while debug mode is off. Dropped
/// without [`Started::exited`] (the child was killed past a time limit, or abandoned), it logs that end too.
pub struct Started {
    at: std::time::Instant,
    kind: Kind,
    line: String,
    logged: bool,
}

impl Started {
    pub fn now(cmd: &std::process::Command) -> Option<Self> {
        enabled().then(|| Self {
            at: std::time::Instant::now(),
            kind: program_kind(cmd),
            line: command_line(cmd),
            logged: false,
        })
    }

    /// Logs the child's end: how long it ran and its exit code.
    pub fn exited(mut self, status: std::process::ExitStatus) {
        self.logged = true;
        let (kind, line, took) = (self.kind, std::mem::take(&mut self.line), self.at.elapsed());
        record(|| {
            let entry = Entry::new(kind, line).ms(took);
            match status.code() {
                Some(0) => entry.detail("exit 0"),
                Some(code) => entry.failed(&format!("exit {code}")),
                None => entry.failed("killed by a signal"),
            }
        });
    }
}

impl Drop for Started {
    fn drop(&mut self) {
        if !self.logged {
            let (kind, line, took) = (self.kind, std::mem::take(&mut self.line), self.at.elapsed());
            record(|| {
                Entry::new(kind, line)
                    .ms(took)
                    .failed("no exit status: killed past its time limit or abandoned")
            });
        }
    }
}

fn program_name(cmd: &std::process::Command) -> String {
    Path::new(cmd.get_program())
        .file_name()
        .unwrap_or(cmd.get_program())
        .to_string_lossy()
        .into_owned()
}

/// Tailnet for the Tailscale CLI (`tailscale`, the macOS app's `Tailscale`, a fake), else process.
pub fn program_kind(cmd: &std::process::Command) -> Kind {
    match program_name(cmd).to_ascii_lowercase().contains("tailscale") {
        true => Kind::Tailnet,
        false => Kind::Process,
    }
}

/// `program arg arg…` with secrets redacted and at most 400 chars.
pub fn command_line(cmd: &std::process::Command) -> String {
    let args: Vec<OsString> = cmd.get_args().map(OsString::from).collect();
    let mut line = program_name(cmd);
    for arg in redact_args(&args) {
        line.push(' ');
        line.push_str(&arg);
    }
    clip(line, 400)
}

fn clip(mut s: String, max: usize) -> String {
    if s.len() > max {
        let mut end = max;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
        s.push('…');
    }
    s
}

const REDACTED: &str = "<redacted>";
/// A flag whose name holds one of these takes a secret or free text (a prompt, a commit message): its value goes.
const SECRET_FLAG_WORDS: [&str; 13] = [
    "token",
    "secret",
    "password",
    "passwd",
    "key",
    "auth",
    "cookie",
    "credential",
    "header",
    "prompt",
    "message",
    "body",
    "title",
];
/// Short flags that take a message, a header or a prompt (`git commit -m`, `curl -H`, `claude -p`).
const SECRET_SHORT_FLAGS: [&str; 4] = ["-m", "-H", "-p", "-F"];
/// Token shapes that are secrets wherever they show up.
const TOKEN_PREFIXES: [&str; 13] = [
    "ghp_",
    "gho_",
    "ghu_",
    "ghs_",
    "ghr_",
    "github_pat_",
    "sk-",
    "xoxb-",
    "xoxp-",
    "glpat-",
    "tskey-",
    "AKIA",
    "eyJ",
];
/// Longer args are free text or blobs (a prompt, base64): only their length is kept.
const MAX_ARG: usize = 200;

/// `args` as they are safe to log: values of secret-looking flags, token-shaped words, URL credentials and query
/// strings, and over-long args are replaced. `-c` / `--config key=value` keep the (redacted) key; text typed with
/// `input text` (adb) goes.
pub fn redact_args(args: &[OsString]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(args.len());
    let mut hide_next = false;
    let mut config_next = false;
    for arg in args {
        let arg = arg.to_string_lossy();
        if std::mem::take(&mut hide_next) {
            out.push(REDACTED.to_owned());
            continue;
        }
        if std::mem::take(&mut config_next) {
            out.push(config_pair(&arg));
            continue;
        }
        if arg == "-c" || arg == "--config" {
            config_next = true;
            out.push(arg.into_owned());
            continue;
        }
        if let Some(pair) = arg.strip_prefix("--config=") {
            out.push(format!("--config={}", config_pair(pair)));
            continue;
        }
        // `adb shell input text <typed>`: what an agent types can be a password.
        if arg == "text" && out.last().is_some_and(|prev| prev == "input") {
            hide_next = true;
            out.push(arg.into_owned());
            continue;
        }
        // `-m"text"`: a short secret flag with its value attached.
        if let Some(short) = SECRET_SHORT_FLAGS
            .iter()
            .find(|f| arg.len() > 2 && arg.starts_with(*f) && !arg.starts_with("--"))
        {
            out.push(format!("{short}{REDACTED}"));
            continue;
        }
        if let Some(flag) = arg.strip_prefix('-').filter(|_| arg.len() > 1) {
            let (name, value) = match flag.split_once('=') {
                Some((n, v)) => (n, Some(v)),
                None => (flag, None),
            };
            let lower = name.to_ascii_lowercase();
            let secret = SECRET_SHORT_FLAGS.contains(&arg.as_ref())
                || SECRET_FLAG_WORDS.iter().any(|w| lower.contains(w));
            match (secret, value) {
                (true, Some(_)) => out.push(format!("-{name}={REDACTED}")),
                (true, None) => {
                    hide_next = true;
                    out.push(arg.into_owned());
                }
                (false, _) => out.push(redact_word(&arg)),
            }
            continue;
        }
        out.push(redact_word(&arg));
    }
    out
}

/// `key=value` with the value hidden and the key redacted like any word (it can hold a URL).
fn config_pair(pair: &str) -> String {
    match pair.split_once('=') {
        Some((key, _)) => format!("{}={REDACTED}", redact_word(key)),
        None => redact_word(pair),
    }
}

/// One arg or word: a token shape goes whole, a URL loses its credentials and query, a long one keeps its length.
fn redact_word(word: &str) -> String {
    if word.chars().count() > MAX_ARG {
        return format!("<{} chars>", word.chars().count());
    }
    if looks_like_token(word) {
        return REDACTED.to_owned();
    }
    redact_url(word)
}

fn looks_like_token(word: &str) -> bool {
    TOKEN_PREFIXES.iter().any(|p| {
        word.match_indices(p).any(|(i, _)| {
            let starts_word = word[..i]
                .chars()
                .next_back()
                .is_none_or(|c| !c.is_ascii_alphanumeric());
            starts_word && word.len() > i + p.len() + 3
        })
    })
}

fn redact_url(word: &str) -> String {
    let Some(scheme_end) = word.find("://") else {
        return word.to_owned();
    };
    let (head, rest) = word.split_at(scheme_end + 3);
    let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(host_end);
    let authority = match authority.rsplit_once('@') {
        Some((_, host)) => format!("{REDACTED}@{host}"),
        None => authority.to_owned(),
    };
    let (tail, fragment) = match tail.split_once('#') {
        Some((before, _)) => (before, format!("#{REDACTED}")),
        None => (tail, String::new()),
    };
    let tail = match tail.split_once('?') {
        Some((path, _)) => format!("{path}?{REDACTED}"),
        None => tail.to_owned(),
    };
    format!("{head}{authority}{tail}{fragment}")
}

/// Free text (an error message) as it is safe to log: token-shaped words and URL credentials go, `Bearer`/`Basic`
/// values go, at most 300 chars.
pub fn redact_text(text: &str) -> String {
    let mut out = Vec::new();
    let mut hide_next = false;
    for word in text.split(' ') {
        if std::mem::take(&mut hide_next) {
            out.push(REDACTED.to_owned());
            continue;
        }
        hide_next = matches!(word, "Bearer" | "Basic" | "token" | "Token");
        out.push(if looks_like_token(word) {
            REDACTED.to_owned()
        } else {
            redact_url(word)
        });
    }
    clip(out.join(" "), 300)
}

static LIVE: Mutex<BTreeMap<&'static str, i64>> = Mutex::new(BTreeMap::new());

fn bump(kind: &'static str, by: i64) {
    *LIVE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .entry(kind)
        .or_default() += by;
}

/// Counts one live object of `kind` while it is held: put it in a field of the thing to count. Always on (objects are
/// made rarely), so the counts are right when debug mode is turned on later.
#[derive(Debug)]
pub struct Live(&'static str);

impl Live {
    pub fn new(kind: &'static str) -> Self {
        bump(kind, 1);
        Self(kind)
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        bump(self.0, -1);
    }
}

/// Every kind ever counted and how many are alive now, by name.
pub fn live_counts() -> Vec<(&'static str, i64)> {
    LIVE.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .map(|(k, v)| (*k, *v))
        .collect()
}
