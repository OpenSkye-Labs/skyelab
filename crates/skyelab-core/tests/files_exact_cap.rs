//! Exactly `cap` matches is the whole result, not a cut one.

use skyelab_core::files::search;
use std::sync::atomic::AtomicBool;

#[test]
fn exact_cap_is_not_truncated() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("a.txt"), "needle\nneedle\n").expect("write");
    let found = search(dir.path(), "needle", 2, &AtomicBool::new(false)).expect("search");
    assert_eq!(found.files[0].lines.len(), 2);
    assert!(!found.truncated, "exactly-cap result reported as truncated");
}
