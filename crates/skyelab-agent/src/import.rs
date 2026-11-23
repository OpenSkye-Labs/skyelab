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
}

/// Sessions in `home` (the account's `CLAUDE_CONFIG_DIR` or `CODEX_HOME`) that ran in `folder` or under it, newest
/// first. Unreadable files are left out.
pub fn list(provider: Provider, home: &Path, folder: &Path) -> Vec<Transcript> {
    let files = match provider {
        Provider::Claude => claude_files(home, folder),
        Provider::Codex => codex::files(home, folder),
    };
    let mut found: Vec<Transcript> = files
        .iter()
        .filter_map(|f| read(provider, home, f, MAX_TRANSCRIPT_BYTES))
        .filter(|t| t.cwd.starts_with(folder))
        .map(|mut t| {
            t.entries = Vec::new();
            t
        })
        .collect();
    found.sort_by_key(|t| std::cmp::Reverse(t.modified_ms));
    found.truncate(MAX_SESSIONS);
    found
}

/// Reads one transcript, up to `cap` bytes. `None` when it can't be opened or names no session or folder.
pub fn read(provider: Provider, home: &Path, path: &Path, cap: u64) -> Option<Transcript> {
    let file = std::fs::File::open(path).ok()?;
    let meta = file.metadata().ok()?;
    let modified_ms = meta
        .modified()
        .ok()
        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis() as i64);
    let mut lines = Lines::new(file, cap);
    let mut t = match provider {
        Provider::Claude => claude(path, &mut lines)?,
        Provider::Codex => codex::parse(home, &mut lines)?,
    };
    (t.path, t.modified_ms, t.skipped, t.truncated) = (
        path.to_owned(),
        modified_ms,
        lines.skipped,
        meta.len() > cap,
    );
    valid_id(&t.id).then_some(t)
}

/// A session id is passed to the agent as an argument: ids and uuids only, never something that reads as a flag.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('-')
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// The name Claude Code gives a folder's dir under `projects/`: every character but ASCII letters and digits
/// becomes `-`.
pub fn claude_dir_name(folder: &Path) -> String {
    folder
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Claude Code cuts long dir names at this many characters (then adds a hash).
const CLAUDE_DIR_NAME_MAX: usize = 200;

/// Transcripts in dirs named like `folder` or a folder under it. Their recorded cwd decides afterwards.
fn claude_files(home: &Path, folder: &Path) -> Vec<PathBuf> {
    let name = claude_dir_name(folder);
    let prefix = &name[..name.len().min(CLAUDE_DIR_NAME_MAX)];
    let Ok(dirs) = std::fs::read_dir(home.join("projects")) else {
        return Vec::new();
    };
    dirs.flatten()
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
    cap: u64,
    read: u64,
}

impl Lines {
    fn new(file: std::fs::File, cap: u64) -> Self {
        Lines {
            reader: BufReader::new(file.take(cap)),
            skipped: 0,
            cap,
            read: 0,
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
            self.read += n as u64;
            if buf.last() != Some(&b'\n') && self.read >= self.cap {
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
    pub(crate) messages: usize,
    pub(crate) first_prompt: Option<String>,
    open: bool,
}

impl Timeline {
    pub(crate) fn prompt(&mut self, text: String) {
        self.end_turn();
        self.first_prompt.get_or_insert_with(|| text.clone());
        self.entries.push(Entry::Prompt(text));
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
        self.entries.push(Entry::Agent(event));
    }

    pub(crate) fn end_turn(&mut self) {
        if std::mem::take(&mut self.open) {
            self.entries.push(Entry::Agent(Event::Result {
                is_error: false,
                duration_ms: 0,
                total_cost_usd: 0.0,
                result: None,
                prompts: 1,
                usage: None,
            }));
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
                let typed = claude_user_text(&text_of(content, &["text"]));
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
    })
}

fn has_tool_result(content: &Value) -> bool {
    content
        .as_array()
        .is_some_and(|a| a.iter().any(|b| b["type"] == "tool_result"))
}
