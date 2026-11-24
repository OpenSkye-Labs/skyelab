//! Imported entries carry the session's own times (each jsonl line's `timestamp`), not the time of the import.

use skyelab_agent::import::{self, MAX_TRANSCRIPT_BYTES};
use skyelab_core::Provider;
use std::path::Path;

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

/// 2026-11-20T10:00:00Z and 2026-11-21T08:00:00Z, in Unix milliseconds.
const CLAUDE_DAY: i64 = 1_795_168_800_000;
const CODEX_DAY: i64 = 1_795_248_000_000;

#[test]
fn claude_entries_carry_their_line_times() {
    let home = tempfile::tempdir().expect("temp dir");
    let folder = tempfile::tempdir().expect("temp dir");
    let dir = home
        .path()
        .join("projects")
        .join(import::claude_dir_name(folder.path()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let file = dir.join("5f1c2a3b-0000-4000-8000-00000000c1a0.jsonl");
    std::fs::write(&file, fixture("claude-transcript.jsonl", folder.path())).expect("write");
    let s =
        import::read(Provider::Claude, home.path(), &file, MAX_TRANSCRIPT_BYTES).expect("reads");
    let at = |ms: i64| CLAUDE_DAY + ms;
    assert_eq!(
        s.times,
        vec![
            at(1_000),   // Fix the login bug
            at(2_000),   // thinking
            at(2_500),   // tool use
            at(3_000),   // tool result
            at(4_000),   // Fixed it.
            at(4_000),   // the turn ends with its last reply
            at(120_000), // Add a test
            at(125_000), // Added.
            at(125_000),
        ]
    );
}

#[test]
fn codex_entries_carry_their_line_times() {
    let home = tempfile::tempdir().expect("temp dir");
    let folder = tempfile::tempdir().expect("temp dir");
    let day = home.path().join("sessions/2026/11/21");
    std::fs::create_dir_all(&day).expect("temp dir");
    let file = day.join("rollout-2026-11-21T08-00-00-019a0c0d-0000-7000-8000-00000000c0de.jsonl");
    std::fs::write(&file, fixture("codex-rollout.jsonl", folder.path())).expect("write");
    let s = import::read(Provider::Codex, home.path(), &file, MAX_TRANSCRIPT_BYTES).expect("reads");
    let at = |ms: i64| CODEX_DAY + ms;
    assert_eq!(
        s.times,
        vec![
            at(300),
            at(4_000),
            at(4_000),
            at(300_000),
            at(309_000),
            at(309_000)
        ]
    );
}

#[test]
fn a_line_without_a_time_keeps_the_last_one() {
    let home = tempfile::tempdir().expect("temp dir");
    // Absolute on every platform (`/work/app` has no drive on Windows, so its lines would be dropped).
    let folder = &home.path().join("app");
    let dir = home
        .path()
        .join("projects")
        .join(import::claude_dir_name(folder));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let file = dir.join("5f1c2a3b-0000-4000-8000-00000000c1a2.jsonl");
    let user = |text: &str, at: Option<&str>| {
        let mut v = serde_json::json!({
            "type": "user", "cwd": folder, "message": {"role": "user", "content": text}
        });
        if let Some(at) = at {
            v["timestamp"] = at.into();
        }
        format!("{v}\n")
    };
    let text = user("one", Some("2026-11-20T10:00:01Z")) + &user("two", None);
    std::fs::write(&file, text).expect("write");
    let s =
        import::read(Provider::Claude, home.path(), &file, MAX_TRANSCRIPT_BYTES).expect("reads");
    let at = CLAUDE_DAY + 1_000;
    assert_eq!(
        s.times,
        vec![at, at, at, at],
        "two prompts, each closing a turn"
    );
}

#[test]
fn a_line_without_a_time_takes_none() {
    let home = tempfile::tempdir().expect("temp dir");
    // Absolute on every platform (`/work/app` has no drive on Windows, so its lines would be dropped).
    let folder = &home.path().join("app");
    let dir = home
        .path()
        .join("projects")
        .join(import::claude_dir_name(folder));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let file = dir.join("5f1c2a3b-0000-4000-8000-00000000c1a1.jsonl");
    let line = serde_json::json!({
        "type": "user", "cwd": folder, "timestamp": "not a time",
        "message": {"role": "user", "content": "hi"}
    });
    std::fs::write(&file, format!("{line}\n")).expect("write");
    let s =
        import::read(Provider::Claude, home.path(), &file, MAX_TRANSCRIPT_BYTES).expect("reads");
    assert_eq!(
        s.times,
        vec![0, 0],
        "unknown: the importer uses the file's time"
    );
}
