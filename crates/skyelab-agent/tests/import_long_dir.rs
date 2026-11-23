//! Claude Code's dir name for a long folder: the first 200 encoded characters, `-`, then the folder path's 32-bit
//! Java string hash (over UTF-16 units) in base 36, unsigned. Recorded from Claude Code 2.1.330:
//! `n=e.replace(/[^a-zA-Z0-9]/g,"-"); n.length<=200 ? n : `${n.slice(0,200)}-${Math.abs(hash(e)).toString(36)}``.

use skyelab_agent::import;
use skyelab_core::Provider;
use std::path::{Path, PathBuf};

#[test]
fn short_names_are_encoded_per_utf16_unit() {
    assert_eq!(import::claude_dir_name(Path::new("/x/é")), "-x--");
    // An emoji is two UTF-16 units, so two dashes, as JavaScript's replace makes.
    assert_eq!(import::claude_dir_name(Path::new("/x/😀")), "-x---");
}

#[test]
fn long_names_are_cut_and_hashed() {
    let a = PathBuf::from(format!("/work/{}/é😀", "a".repeat(240)));
    assert_eq!(
        import::claude_dir_name(&a),
        format!("-work-{}-vu1jl8", "a".repeat(194))
    );
    // A negative hash is written unsigned.
    let d = PathBuf::from(format!("/work/{}", "d".repeat(240)));
    assert_eq!(
        import::claude_dir_name(&d),
        format!("-work-{}-gfpl01", "d".repeat(194))
    );
}

#[test]
fn long_claude_dir_name_is_found() {
    let home = tempfile::tempdir().expect("temp dir");
    let root = tempfile::tempdir().expect("temp dir");
    let root = root.path().canonicalize().expect("exists");
    let folder = root.join("n".repeat(230));
    // Claude Code's own name for it, written out (not from claude_dir_name), so the test checks the shape.
    let encoded: String = folder
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    assert!(encoded.len() > 200);
    let dir = home
        .path()
        .join("projects")
        .join(format!("{}-1abc2d", &encoded[..200]));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let line = serde_json::json!({
        "type": "user", "cwd": folder, "timestamp": "2026-11-20T10:00:00Z",
        "message": {"role": "user", "content": "hi"}
    });
    std::fs::write(
        dir.join("5f1c2a3b-0000-4000-8000-00000000c1a2.jsonl"),
        format!("{line}\n"),
    )
    .expect("write");
    let found = import::list(Provider::Claude, home.path(), &folder);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].cwd, folder);
}
