//! Prompt stash entries are drafts under a `stash:` owner: listed by prefix without their files' bytes, and taken
//! (read and removed, text and files) in one go.

use skyelab_store::{Draft, Store};

#[test]
fn stash_drafts() {
    let mut s = Store::open_in_memory().expect("store");
    s.set_draft_text("thread:1", "a thread's draft")
        .expect("draft");
    s.set_draft_text("stash:1", "older").expect("stash 1");
    s.set_draft_text("stash:2", "newer").expect("stash 2");
    s.set_draft_files(
        "stash:2",
        &[
            ("a.txt".into(), b"aa".to_vec()),
            ("b.txt".into(), b"b".to_vec()),
        ],
    )
    .expect("files");

    let rows = s.drafts_with_prefix("stash:").expect("list");
    let listed: Vec<(&str, &str, usize)> = rows
        .iter()
        .map(|r| (r.owner.as_str(), r.text.as_str(), r.files))
        .collect();
    assert_eq!(
        listed,
        [("stash:2", "newer", 2), ("stash:1", "older", 0)],
        "LISTING"
    );

    let taken = s.take_draft("stash:2").expect("take");
    assert_eq!(
        taken,
        Draft {
            text: "newer".into(),
            files: vec![
                ("a.txt".into(), b"aa".to_vec()),
                ("b.txt".into(), b"b".to_vec())
            ],
        }
    );
    assert!(
        s.draft("stash:2").expect("read").is_empty(),
        "TAKE_LEFT_FILES"
    );
    s.delete_draft("stash:1").expect("delete");
    assert!(s.drafts_with_prefix("stash:").expect("list").is_empty());
    assert_eq!(s.draft("thread:1").expect("read").text, "a thread's draft");
}
