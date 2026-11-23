//! A session listed from a real `projects/` dir, read again at import after `projects/` became a link to another
//! account's dir, is not read: every folder between the account dir and the transcript must still be a real one.
#![cfg(unix)] // symlinks

use skyelab_agent::import;
use skyelab_core::Provider;
use std::os::unix::fs::symlink;
use std::path::Path;

#[test]
fn a_listed_session_is_not_read_through_a_new_link() {
    let home = tempfile::tempdir().expect("temp dir");
    let theirs = tempfile::tempdir().expect("temp dir");
    let folder = tempfile::tempdir().expect("temp dir");
    let folder = folder.path().canonicalize().expect("exists");
    let text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-transcript.jsonl"),
    )
    .expect("fixture")
    .replace("__CWD__", folder.to_str().expect("utf-8"));
    let name = import::claude_dir_name(&folder);
    let file = "5f1c2a3b-0000-4000-8000-00000000c1a0.jsonl";
    for root in [home.path(), theirs.path()] {
        let dir = root.join("projects").join(&name);
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join(file), &text).expect("write");
    }
    let listed = import::list(Provider::Claude, home.path(), &folder);
    let path = listed.first().expect("listed").path.clone();
    let cap = import::MAX_TRANSCRIPT_BYTES;
    assert!(import::read(Provider::Claude, home.path(), &path, cap).is_some());

    std::fs::remove_dir_all(home.path().join("projects")).expect("rm");
    symlink(theirs.path().join("projects"), home.path().join("projects")).expect("link");
    assert!(
        import::read(Provider::Claude, home.path(), &path, cap).is_none(),
        "read through the linked projects/"
    );
}
