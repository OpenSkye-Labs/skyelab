//! Reading existing Claude Code and Codex sessions for import (fixtures record the on-disk shapes).

use serde_json::json;
use skyelab_agent::import::{self, Entry, MAX_TRANSCRIPT_BYTES};
use skyelab_agent::{Block, Event};
use skyelab_core::Provider;
use std::path::{Path, PathBuf};

/// The fixture with `__CWD__` replaced by `cwd`, JSON-escaped (Windows paths have backslashes).
fn fixture(name: &str, cwd: &Path) -> String {
    let text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .expect("fixture is in the repo");
    let escaped =
        serde_json::to_string(cwd.to_str().expect("temp paths are UTF-8")).expect("a string");
    text.replace("__CWD__", &escaped[1..escaped.len() - 1])
}

const CLAUDE_ID: &str = "5f1c2a3b-0000-4000-8000-00000000c1a0";
const CODEX_ID: &str = "019a0c0d-0000-7000-8000-00000000c0de";

/// A Claude config dir holding the fixture session for `cwd`; returns the transcript's path.
fn claude_home(home: &Path, cwd: &Path) -> PathBuf {
    let dir = home.join("projects").join(import::claude_dir_name(cwd));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let file = dir.join(format!("{CLAUDE_ID}.jsonl"));
    std::fs::write(&file, fixture("claude-transcript.jsonl", cwd)).expect("temp dir");
    file
}

fn result() -> Entry {
    Entry::Agent(Event::Result {
        is_error: false,
        duration_ms: 0,
        total_cost_usd: 0.0,
        result: None,
        prompts: 1,
        usage: None,
        turn_cost_usd: None,
    })
}

fn text(t: &str) -> Entry {
    Entry::Agent(Event::Assistant {
        content: vec![Block::Text { text: t.into() }],
    })
}

#[test]
fn claude_dir_name_matches_claude_codes_encoding() {
    assert_eq!(
        import::claude_dir_name(Path::new("/Users/a/.t3/x_y")),
        "-Users-a--t3-x-y"
    );
}

#[test]
fn claude_session_is_listed_with_title_count_and_cwd() {
    let home = tempfile::tempdir().expect("temp dir");
    let folder = tempfile::tempdir().expect("temp dir");
    let folder = folder.path().canonicalize().expect("exists");
    claude_home(home.path(), &folder);
    let found = import::list(Provider::Claude, home.path(), &folder);
    assert_eq!(found.len(), 1, "{found:?}");
    let s = &found[0];
    assert_eq!(s.id, CLAUDE_ID);
    assert_eq!(s.cwd, folder);
    assert_eq!(s.title, "Login fix");
    assert_eq!(s.messages, 4, "two prompts, two replies with text");
    assert!(s.entries.is_empty(), "a listing keeps no history");
}

#[test]
fn claude_transcript_replays_as_prompts_and_stream_json_events() {
    let home = tempfile::tempdir().expect("temp dir");
    let folder = tempfile::tempdir().expect("temp dir");
    let file = claude_home(home.path(), folder.path());
    let s =
        import::read(Provider::Claude, home.path(), &file, MAX_TRANSCRIPT_BYTES).expect("reads");
    assert_eq!(s.skipped, 1, "the cut-off line");
    assert!(!s.truncated);
    assert_eq!(
        s.entries,
        vec![
            Entry::Prompt("Fix the login bug".into()),
            Entry::Agent(Event::Assistant {
                content: vec![Block::Thinking {
                    thinking: "Look at auth.rs first.".into()
                }]
            }),
            Entry::Agent(Event::Assistant {
                content: vec![Block::ToolUse {
                    id: "toolu_1".into(),
                    name: "Bash".into(),
                    input: json!({"command": "grep -n login src/auth.rs"})
                }]
            }),
            Entry::Agent(Event::User {
                content: vec![Block::ToolResult {
                    tool_use_id: "toolu_1".into(),
                    content: json!("12: fn login()"),
                    is_error: false
                }]
            }),
            text("Fixed it."),
            result(),
            Entry::Prompt("Add a test".into()),
            text("Added."),
            result(),
        ]
    );
}

#[test]
fn claude_sessions_of_other_folders_are_not_listed() {
    let home = tempfile::tempdir().expect("temp dir");
    let root = tempfile::tempdir().expect("temp dir");
    let (folder, sibling) = (root.path().join("app"), root.path().join("app-old"));
    std::fs::create_dir_all(&folder).expect("temp dir");
    // `app-old` encodes with `app`'s name as a prefix; its recorded cwd is what rules it out.
    claude_home(home.path(), &sibling);
    assert!(import::list(Provider::Claude, home.path(), &folder).is_empty());
    let sub = folder.join("crates");
    claude_home(home.path(), &sub);
    let found = import::list(Provider::Claude, home.path(), &folder);
    assert_eq!(found.len(), 1, "a session in a subfolder counts");
    assert_eq!(found[0].cwd, sub);
}

#[test]
fn a_cap_smaller_than_the_transcript_truncates_it() {
    let home = tempfile::tempdir().expect("temp dir");
    let folder = tempfile::tempdir().expect("temp dir");
    let file = claude_home(home.path(), folder.path());
    let first_two = fixture("claude-transcript.jsonl", folder.path())
        .lines()
        .take(3)
        .map(|l| l.len() + 1)
        .sum::<usize>();
    let s =
        import::read(Provider::Claude, home.path(), &file, first_two as u64 + 10).expect("reads");
    assert!(s.truncated);
    assert_eq!(
        s.entries,
        vec![Entry::Prompt("Fix the login bug".into()), result()]
    );
}

#[test]
fn codex_rollout_replays_prompts_and_replies_with_its_index_title() {
    let home = tempfile::tempdir().expect("temp dir");
    let folder = tempfile::tempdir().expect("temp dir");
    let folder = folder.path().canonicalize().expect("exists");
    let day = home.path().join("sessions/2026/11/21");
    std::fs::create_dir_all(&day).expect("temp dir");
    let file = day.join(format!("rollout-2026-11-21T08-00-00-{CODEX_ID}.jsonl"));
    std::fs::write(&file, fixture("codex-rollout.jsonl", &folder)).expect("temp dir");
    std::fs::write(
        home.path().join("session_index.jsonl"),
        format!("{{\"id\":\"{CODEX_ID}\",\"thread_name\":\"Old name\"}}\n{{\"id\":\"{CODEX_ID}\",\"thread_name\":\"Rename module\"}}\n"),
    )
    .expect("temp dir");

    let found = import::list(Provider::Codex, home.path(), &folder);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(
        (found[0].id.as_str(), found[0].title.as_str()),
        (CODEX_ID, "Rename module")
    );
    assert_eq!(found[0].messages, 4);

    let s = import::read(Provider::Codex, home.path(), &file, MAX_TRANSCRIPT_BYTES).expect("reads");
    assert_eq!(s.skipped, 1);
    assert_eq!(
        s.entries,
        vec![
            Entry::Prompt("Rename the module".into()),
            text("Renamed."),
            result(),
            Entry::Prompt("Now run the tests".into()),
            text("All green."),
            result(),
        ]
    );
    assert!(import::list(Provider::Codex, home.path(), &folder.join("elsewhere")).is_empty());
}
