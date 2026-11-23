//! Codex rollouts (`<CODEX_HOME>/sessions/YYYY/MM/DD/rollout-*.jsonl`): prompts and replies only. Tool calls are
//! left out; resuming the thread gives the agent its full history back.

use super::{Lines, Timeline, Transcript, text_of, title_of};
use crate::{Block, Event};
use serde_json::Value;
use std::io::{BufRead as _, BufReader, Read as _};
use std::path::{Path, PathBuf};

/// The most of a rollout's first line read to learn its folder.
const META_LINE_MAX: u64 = 1 << 20;
/// The most of `session_index.jsonl` read for thread names.
const INDEX_MAX: u64 = 4 << 20;

/// Rollouts whose `session_meta` names `folder` or a folder under it.
pub(super) fn files(home: &Path, folder: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(&home.join("sessions"), 4, &mut out);
    out.retain(|p| meta_cwd(p).is_some_and(|c| c.starts_with(folder)));
    out
}

fn walk(dir: &Path, depth: u8, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let (path, Ok(kind)) = (e.path(), e.file_type()) else {
            continue;
        };
        if kind.is_dir() && depth > 0 {
            walk(&path, depth - 1, out);
        } else if kind.is_file()
            && path.extension().is_some_and(|x| x == "jsonl")
            && e.file_name().to_string_lossy().starts_with("rollout-")
        {
            out.push(path);
        }
    }
}

fn meta_cwd(path: &Path) -> Option<PathBuf> {
    let file = std::fs::File::open(path).ok()?;
    let mut line = Vec::new();
    BufReader::new(file.take(META_LINE_MAX))
        .read_until(b'\n', &mut line)
        .ok()?;
    let v: Value = serde_json::from_slice(&line).ok()?;
    (v["type"] == "session_meta").then(|| v["payload"]["cwd"].as_str().map(PathBuf::from))?
}

/// The newest name Codex's index gives thread `id`.
fn indexed_name(home: &Path, id: &str) -> Option<String> {
    let file = std::fs::File::open(home.join("session_index.jsonl")).ok()?;
    BufReader::new(file.take(INDEX_MAX))
        .lines()
        .map_while(Result::ok)
        .filter_map(|l| serde_json::from_str::<Value>(&l).ok())
        .filter(|v| v["id"] == id)
        .filter_map(|v| v["thread_name"].as_str().and_then(title_of))
        .last()
}

pub(super) fn parse(home: &Path, lines: &mut Lines) -> Option<Transcript> {
    let mut tl = Timeline::default();
    let (mut id, mut cwd) = (None, None);
    for v in lines {
        let p = &v["payload"];
        match (v["type"].as_str(), p["type"].as_str()) {
            (Some("session_meta"), _) => {
                id = id.or_else(|| p["id"].as_str().map(str::to_owned));
                cwd = cwd.or_else(|| p["cwd"].as_str().map(PathBuf::from));
            }
            // Older Codex records the prompt as `user_message`, newer as a completed UserMessage item; some
            // versions write both for one prompt.
            (Some("event_msg"), Some("user_message")) => prompt(&mut tl, p["message"].as_str()),
            (Some("event_msg"), Some("item_completed")) if p["item"]["type"] == "UserMessage" => {
                prompt(&mut tl, Some(&text_of(&p["item"]["content"], &["text"])));
            }
            (Some("event_msg"), Some("task_complete")) => tl.end_turn(),
            (Some("response_item"), Some("message")) if p["role"] == "assistant" => {
                let text = text_of(&p["content"], &["output_text"]);
                if !text.trim().is_empty() {
                    tl.agent(Event::Assistant {
                        content: vec![Block::Text { text }],
                    });
                }
            }
            _ => {}
        }
    }
    tl.end_turn();
    let id: String = id?;
    let title = indexed_name(home, &id)
        .or_else(|| tl.first_prompt.as_deref().and_then(title_of))
        .unwrap_or_else(|| "Untitled session".into());
    Some(Transcript {
        id,
        path: PathBuf::new(),
        cwd: cwd?,
        title,
        modified_ms: 0,
        messages: tl.messages,
        skipped: 0,
        truncated: false,
        entries: tl.entries,
    })
}

fn prompt(tl: &mut Timeline, text: Option<&str>) {
    let Some(text) = text.map(str::trim).filter(|t| !t.is_empty()) else {
        return;
    };
    if tl.pending_prompt() != Some(text) {
        tl.prompt(text.to_owned());
    }
}
