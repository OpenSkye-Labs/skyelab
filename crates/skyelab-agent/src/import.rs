//! Existing Claude Code and Codex sessions, read from an account's own dir so they can become threads.
//! Read only: files are opened for reading, never written. Each transcript is read up to a byte cap, and lines that
//! don't parse are skipped and counted.

use crate::{Block, Event};
use serde_json::Value;
use skyelab_core::Provider;
use std::io::{BufRead as _, BufReader, Read as _};
use std::path::{Path, PathBuf};

#[path = "import_codex.rs"]
mod codex;

/// The most of one transcript that is read.
pub const MAX_TRANSCRIPT_BYTES: u64 = 32 << 20;
/// The most sessions one listing returns (the newest).
pub const MAX_SESSIONS: usize = 500;
/// Titles longer than this are cut.
const MAX_TITLE: usize = 80;

/// One thing a session's timeline shows: what the user typed, or an agent event.
#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    Prompt(String),
    Agent(Event),
}

/// A session found on disk.
#[derive(Debug, Clone, PartialEq)]
pub struct Transcript {
    /// What `--resume` (or Codex's thread resume) takes.
    pub id: String,
    pub path: PathBuf,
    /// The folder the session ran in.
    pub cwd: PathBuf,
    pub title: String,
    /// The file's last change, in Unix milliseconds.
    pub modified_ms: i64,
    /// User prompts plus agent replies with text.
    pub messages: usize,
    /// Lines that didn't parse.
    pub skipped: usize,
    /// The file was longer than the cap; the rest was not read.
    pub truncated: bool,
    /// The history, oldest first; empty in a [`list`].
    pub entries: Vec<Entry>,
    /// When each entry happened, in Unix milliseconds (its line's `timestamp`, else the last one before it); 0 when no
    /// line before it had one.
    pub times: Vec<i64>,
}

/// Sessions in `home` (the account's `CLAUDE_CONFIG_DIR` or `CODEX_HOME`) that ran in `folder` or under it, newest
/// first. Unreadable files are left out.
pub fn list(provider: Provider, home: &Path, folder: &Path) -> Vec<Transcript> {
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = match provider {
        Provider::Claude => claude_files(home, folder),
        Provider::Codex => codex::files(home, folder),
    }
    .into_iter()
    .filter_map(|p| Some((regular_file(&p)?.modified().ok()?, p)))
    .collect();
    // Newest first, and only as many as are listed: each one is read whole to count its messages.
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    files.truncate(MAX_SESSIONS);
    let names = match provider {
        Provider::Claude => Default::default(),
        Provider::Codex => codex::names(home),
    };
    files
        .iter()
        .filter_map(|(_, f)| read_file(provider, f, MAX_TRANSCRIPT_BYTES))
        .filter(|t| t.cwd.starts_with(folder))
        .map(|mut t| {
            if let Some(name) = names.get(&t.id) {
                t.title.clone_from(name);
            }
            t.entries = Vec::new();
            t.times = Vec::new();
            t
        })
        .collect()
}

/// `path`'s metadata when it is a plain file: not a link (it could point into another account's dir) and not a FIFO
/// or device (opening one can block for ever).
fn regular_file(path: &Path) -> Option<std::fs::Metadata> {
    std::fs::symlink_metadata(path).ok().filter(|m| m.is_file())
}

/// Reads one transcript, up to `cap` bytes. `None` when it can't be opened or names no session or folder.
pub fn read(provider: Provider, home: &Path, path: &Path, cap: u64) -> Option<Transcript> {
    let mut t = read_file(provider, path, cap)?;
    if provider == Provider::Codex
        && let Some(name) = codex::names(home).remove(&t.id)
    {
        t.title = name;
    }
    Some(t)
}

fn read_file(provider: Provider, path: &Path, cap: u64) -> Option<Transcript> {
    regular_file(path)?;
    let file = std::fs::File::open(path).ok()?;
    let meta = file.metadata().ok()?;
    let modified_ms = meta
        .modified()
        .ok()
        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis() as i64);
    let mut lines = Lines::new(file, cap, meta.len() > cap);
    let mut t = match provider {
        Provider::Claude => claude(path, &mut lines)?,
        Provider::Codex => codex::parse(&mut lines)?,
    };
    (t.path, t.modified_ms, t.skipped, t.truncated) = (
        path.to_owned(),
        modified_ms,
        lines.skipped,
        meta.len() > cap,
    );
    (valid_id(&t.id) && plain_path(&t.cwd)).then_some(t)
}

/// An absolute path without `.` or `..`, so comparing it with a folder by its components means what it says.
fn plain_path(p: &Path) -> bool {
    use std::path::Component;
    p.is_absolute()
        && p.components()
            .all(|c| !matches!(c, Component::CurDir | Component::ParentDir))
}

/// A session id is passed to the agent as an argument: ids and uuids only, never something that reads as a flag.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('-')
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// The name Claude Code gives a folder's dir under `projects/`: every UTF-16 unit but ASCII letters and digits
/// becomes `-`; a name over [`CLAUDE_DIR_NAME_MAX`] is cut there and gets `-` plus a hash of the folder path.
pub fn claude_dir_name(folder: &Path) -> String {
    let path = folder.to_string_lossy();
    let name: String = path
        .encode_utf16()
        .map(|u| match char::from_u32(u.into()) {
            Some(c) if c.is_ascii_alphanumeric() => c,
            _ => '-',
        })
        .collect();
    if name.len() <= CLAUDE_DIR_NAME_MAX {
        return name;
    }
    format!(
        "{}-{}",
        &name[..CLAUDE_DIR_NAME_MAX],
        base36(java_hash(&path).unsigned_abs())
    )
}

/// Claude Code cuts long dir names at this many characters (then adds a hash).
const CLAUDE_DIR_NAME_MAX: usize = 200;

/// JavaScript's `h = (h << 5) - h + charCodeAt(i) | 0` over the UTF-16 units of `s`.
fn java_hash(s: &str) -> i32 {
    s.encode_utf16()
        .fold(0i32, |h, u| h.wrapping_mul(31).wrapping_add(i32::from(u)))
}

fn base36(mut n: u32) -> String {
    let mut digits = Vec::new();
    loop {
        digits.push(char::from_digit(n % 36, 36).expect("n % 36 is a base-36 digit"));
        n /= 36;
        if n == 0 {
            break;
        }
    }
    digits.iter().rev().collect()
}

/// Transcripts in dirs named like `folder` or a folder under it. Their recorded cwd decides afterwards.
fn claude_files(home: &Path, folder: &Path) -> Vec<PathBuf> {
    let name = claude_dir_name(folder);
    // A subfolder's name starts with the folder's, up to the cut; a long folder's own dir is matched whole.
    let prefix = &name[..name.len().min(CLAUDE_DIR_NAME_MAX)];
    let Ok(dirs) = std::fs::read_dir(home.join("projects")) else {
        return Vec::new();
    };
    // `file_type` doesn't follow links: a linked dir could be another account's.
    dirs.flatten()
        .filter(|d| d.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|d| d.file_name().to_string_lossy().starts_with(prefix))
        .filter_map(|d| std::fs::read_dir(d.path()).ok())
        .flat_map(|files| files.flatten().map(|f| f.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
        .collect()
}

/// JSON lines of a file, read up to a cap. Lines that aren't JSON are counted, not returned.
pub(crate) struct Lines {
    reader: BufReader<std::io::Take<std::fs::File>>,
    pub(crate) skipped: usize,
    /// The file is longer than the cap, so the last line read may be cut off.
    cut: bool,
}

impl Lines {
    fn new(file: std::fs::File, cap: u64, cut: bool) -> Self {
        Lines {
            reader: BufReader::new(file.take(cap)),
            skipped: 0,
            cut,
        }
    }
}

impl Iterator for Lines {
    type Item = Value;

    fn next(&mut self) -> Option<Value> {
        let mut buf = Vec::new();
        loop {
            buf.clear();
            let n = self.reader.read_until(b'\n', &mut buf).ok()?;
            if n == 0 {
                return None;
            }
            if buf.last() != Some(&b'\n') && self.cut {
                return None; // cut off by the cap: not the file's fault
            }
            if buf.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            match serde_json::from_slice(&buf) {
                Ok(v) => return Some(v),
                Err(_) => self.skipped += 1,
            }
        }
    }
}

/// Builds a timeline: closes each turn with a result, as a live turn ends.
#[derive(Default)]
pub(crate) struct Timeline {
    pub(crate) entries: Vec<Entry>,
    /// Each entry's time, as [`Transcript::times`].
    pub(crate) times: Vec<i64>,
    /// The time of the line being read (the last one seen); 0 before any.
    at: i64,
    pub(crate) messages: usize,
    pub(crate) first_prompt: Option<String>,
    open: bool,
}

impl Timeline {
    /// Reads line `v`'s `timestamp` (RFC 3339, as Claude Code and Codex write it): the time of what it adds. A line
    /// without one keeps the last time seen, so the history stays in order.
    pub(crate) fn line(&mut self, v: &Value) {
        if let Some(t) = v["timestamp"]
            .as_str()
            .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
        {
            self.at = t.timestamp_millis();
        }
    }

    fn push(&mut self, entry: Entry, at: i64) {
        self.entries.push(entry);
        self.times.push(at);
    }

    pub(crate) fn prompt(&mut self, text: String) {
        self.end_turn();
        self.first_prompt.get_or_insert_with(|| text.clone());
        self.push(Entry::Prompt(text), self.at);
        self.messages += 1;
        self.open = true;
    }

    /// The last prompt, while its turn has no reply yet.
    pub(crate) fn pending_prompt(&self) -> Option<&str> {
        match self.entries.last() {
            Some(Entry::Prompt(p)) if self.open => Some(p),
            _ => None,
        }
    }

    pub(crate) fn agent(&mut self, event: Event) {
        if let Event::Assistant { content } = &event
            && content.iter().any(|b| matches!(b, Block::Text { .. }))
        {
            self.messages += 1;
        }
        self.push(Entry::Agent(event), self.at);
    }

    pub(crate) fn end_turn(&mut self) {
        if std::mem::take(&mut self.open) {
            // A turn ends with its last entry: the line that closes it may come much later (the next prompt).
            let at = self.times.last().copied().unwrap_or(self.at);
            self.push(
                Entry::Agent(Event::Result {
                    is_error: false,
                    duration_ms: 0,
                    total_cost_usd: 0.0,
                    result: None,
                    prompts: 1,
                    usage: None,
                }),
                at,
            );
        }
    }
}

/// One line of a title: the first line of `text`, cut to [`MAX_TITLE`] characters.
pub(crate) fn title_of(text: &str) -> Option<String> {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    Some(match line.char_indices().nth(MAX_TITLE) {
        Some((i, _)) => format!("{}…", &line[..i]),
        None => line.to_owned(),
    })
}

/// Text blocks of a message's content (a string, or an array of `{type: text, text}` blocks).
pub(crate) fn text_of(content: &Value, kinds: &[&str]) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|b| b["type"].as_str().is_some_and(|t| kinds.contains(&t)))
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Claude Code's own UI, recorded as user turns: command output, task notices, interruptions.
const CLAUDE_UI_OUTPUT: &[&str] = &[
    "<task-notification>",
    "<local-command-stdout>",
    "<local-command-stderr>",
    "<local-command-caveat>",
    "<bash-stdout>",
    "<bash-stderr>",
];

fn between<'a>(text: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = text.find(open)? + open.len();
    let end = text[start..].find(close)? + start;
    Some(text[start..end].trim())
}

/// What the user typed, as Claude Code records it: a slash command as `/name args`, a shell run as `!cmd`, and
/// nothing for the UI's own lines.
fn claude_user_text(text: &str) -> String {
    let t = text.trim();
    if CLAUDE_UI_OUTPUT.iter().any(|p| t.starts_with(p))
        || t.starts_with("[Request interrupted by user")
    {
        return String::new();
    }
    if let Some(cmd) = t
        .strip_prefix("<bash-input>")
        .and_then(|r| r.strip_suffix("</bash-input>"))
    {
        return format!("!{cmd}");
    }
    if t.starts_with("<command-name>") || t.starts_with("<command-message>") {
        let name = between(t, "<command-name>", "</command-name>").unwrap_or_default();
        let args = between(t, "<command-args>", "</command-args>").unwrap_or_default();
        return format!("{name} {args}").trim().to_owned();
    }
    t.to_owned()
}

fn claude(path: &Path, lines: &mut Lines) -> Option<Transcript> {
    let mut tl = Timeline::default();
    let (mut cwd, mut custom, mut ai, mut summary) = (None, None, None, None);
    for v in lines {
        tl.line(&v);
        let kind = v["type"].as_str().unwrap_or_default();
        match kind {
            "custom-title" => custom = v["customTitle"].as_str().and_then(title_of),
            "summary" => summary = v["summary"].as_str().and_then(title_of),
            _ => {}
        }
        if let Some(t) = v["aiTitle"].as_str().and_then(title_of) {
            ai = Some(t);
        }
        if v["isSidechain"] == true || v["isMeta"] == true || v["isCompactSummary"] == true {
            continue;
        }
        if cwd.is_none() {
            cwd = v["cwd"].as_str().map(PathBuf::from);
        }
        let content = &v["message"]["content"];
        match kind {
            "user" if content.is_string() || !has_tool_result(content) => {
                let mut typed = claude_user_text(&text_of(content, &["text"]));
                if typed.is_empty() && has_block(content, "image") {
                    typed = "[image]".into(); // still a prompt: its reply is its own turn
                }
                if !typed.is_empty() {
                    tl.prompt(typed);
                }
            }
            "user" | "assistant" => {
                if let Some(e @ (Event::Assistant { .. } | Event::User { .. })) =
                    Event::from_claude(v)
                {
                    tl.agent(e);
                }
            }
            _ => {}
        }
    }
    tl.end_turn();
    let first = tl.first_prompt.as_deref().and_then(title_of);
    Some(Transcript {
        id: path.file_stem()?.to_string_lossy().into_owned(),
        path: PathBuf::new(),
        cwd: cwd?,
        title: custom
            .or(ai)
            .or(summary)
            .or(first)
            .unwrap_or_else(|| "Untitled session".into()),
        modified_ms: 0,
        messages: tl.messages,
        skipped: 0,
        truncated: false,
        entries: tl.entries,
        times: tl.times,
    })
}

fn has_tool_result(content: &Value) -> bool {
    has_block(content, "tool_result")
}

fn has_block(content: &Value, kind: &str) -> bool {
    content
        .as_array()
        .is_some_and(|a| a.iter().any(|b| b["type"] == kind))
}
