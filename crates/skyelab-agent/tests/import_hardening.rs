//! Review findings on session import: what an account dir can hold must not hang, leak or mislead the scan.

use skyelab_agent::import;
use skyelab_core::Provider;
use std::path::Path;

fn line(cwd: &Path, text: &str) -> String {
    serde_json::json!({"type": "user", "cwd": cwd, "message": {"role": "user", "content": text}})
        .to_string()
}

#[test]
fn a_last_line_ending_exactly_at_the_cap_is_read() {
    let dir = tempfile::tempdir().expect("temp dir");
    let f = dir
        .path()
        .join("5f1c2a3b-0000-4000-8000-00000000c1a1.jsonl");
    let body = format!(
        "{}\n{}",
        line(Path::new("/w"), "one"),
        line(Path::new("/w"), "two")
    );
    std::fs::write(&f, &body).expect("write");
    let t = import::read(Provider::Claude, dir.path(), &f, body.len() as u64).expect("parses");
    assert!(!t.truncated);
    assert_eq!(t.messages, 2);
}

#[test]
fn an_image_only_prompt_opens_its_own_turn() {
    let dir = tempfile::tempdir().expect("temp dir");
    let f = dir
        .path()
        .join("5f1c2a3b-0000-4000-8000-00000000c1a2.jsonl");
    let image = serde_json::json!({"type": "user", "cwd": "/w", "message": {"role": "user",
        "content": [{"type": "image", "source": {"type": "base64", "data": "AAAA"}}]}});
    let reply = serde_json::json!({"type": "assistant", "cwd": "/w",
        "message": {"role": "assistant", "content": [{"type": "text", "text": "a cat"}]}});
    std::fs::write(&f, format!("{image}\n{reply}\n")).expect("write");
    let t = import::read(
        Provider::Claude,
        dir.path(),
        &f,
        import::MAX_TRANSCRIPT_BYTES,
    )
    .expect("parses");
    assert_eq!(
        t.entries.first(),
        Some(&import::Entry::Prompt("[image]".into()))
    );
    assert_eq!(t.entries.len(), 3, "prompt, reply, end of turn");
}

#[test]
fn a_cwd_with_dot_dot_is_not_offered() {
    let home = tempfile::tempdir().expect("temp dir");
    let folder = Path::new("/work/app");
    let dir = home
        .path()
        .join("projects")
        .join(import::claude_dir_name(folder));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let sneaky = line(Path::new("/work/app/../other"), "hi");
    std::fs::write(
        dir.join("5f1c2a3b-0000-4000-8000-00000000c1a3.jsonl"),
        sneaky,
    )
    .expect("write");
    assert!(import::list(Provider::Claude, home.path(), folder).is_empty());
}

#[cfg(unix)]
#[test]
fn links_and_fifos_in_the_account_dir_are_skipped() {
    let home = tempfile::tempdir().expect("temp dir");
    let other = tempfile::tempdir().expect("temp dir");
    let folder = Path::new("/work/app");
    let name = import::claude_dir_name(folder);
    // Another account's session dir, linked in under this account's projects.
    let theirs = other.path().join(&name);
    std::fs::create_dir_all(&theirs).expect("temp dir");
    std::fs::write(
        theirs.join("5f1c2a3b-0000-4000-8000-00000000c1a4.jsonl"),
        line(folder, "theirs"),
    )
    .expect("write");
    std::fs::create_dir_all(home.path().join("projects")).expect("temp dir");
    std::os::unix::fs::symlink(&theirs, home.path().join("projects").join(&name)).expect("link");
    assert!(
        import::list(Provider::Claude, home.path(), folder).is_empty(),
        "a linked dir is not read"
    );

    std::fs::remove_file(home.path().join("projects").join(&name)).expect("unlink");
    let ours = home.path().join("projects").join(&name);
    std::fs::create_dir_all(&ours).expect("temp dir");
    let fifo = ours.join("5f1c2a3b-0000-4000-8000-00000000c1a5.jsonl");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo")
            .success()
    );
    std::os::unix::fs::symlink(
        theirs.join("5f1c2a3b-0000-4000-8000-00000000c1a4.jsonl"),
        ours.join("x.jsonl"),
    )
    .expect("link");
    let (tx, rx) = std::sync::mpsc::channel();
    let home_path = home.path().to_owned();
    std::thread::spawn(move || {
        let _ = tx.send(import::list(Provider::Claude, &home_path, folder).len());
    });
    let found = rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("list hung on a FIFO");
    assert_eq!(found, 0, "neither the FIFO nor the linked file is read");
}
