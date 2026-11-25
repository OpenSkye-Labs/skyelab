//! The frozen dir's rr-cache link: git's writes go through it to the repo, and dropping the frozen dir never deletes
//! through it.

use std::path::Path;

use crate::frozen::link_rr_cache;

fn real_rr_cache(git_dir: &Path) -> std::path::PathBuf {
    let rr = git_dir.join("rr-cache");
    std::fs::create_dir_all(rr.join("abc")).expect("rr-cache");
    std::fs::write(rr.join("abc/postimage"), "kept\n").expect("record");
    rr
}

/// Writes through the link land in the real dir, and dropping the private dir leaves the real dir's records.
fn assert_link_writes_through_and_survives_drop(rr: &Path, private: tempfile::TempDir) {
    let link = private.path().join("rr-cache");
    std::fs::write(link.join("abc/preimage"), "new\n").expect("write through the link");
    drop(private);
    assert_eq!(
        std::fs::read_to_string(rr.join("abc/preimage")).expect("written"),
        "new\n"
    );
    assert_eq!(
        std::fs::read_to_string(rr.join("abc/postimage")).expect("kept"),
        "kept\n"
    );
}

#[test]
fn the_rr_cache_link_writes_through_and_survives_the_private_dir() {
    let repo = tempfile::tempdir().expect("tmp");
    let rr = real_rr_cache(repo.path());
    let private = tempfile::tempdir().expect("private");
    link_rr_cache(&rr, &private.path().join("rr-cache")).expect("link");
    assert_link_writes_through_and_survives_drop(&rr, private);
}

#[test]
fn a_missing_rr_cache_is_not_linked() {
    let repo = tempfile::tempdir().expect("tmp");
    let private = tempfile::tempdir().expect("private");
    let link = private.path().join("rr-cache");
    link_rr_cache(&repo.path().join("rr-cache"), &link).expect("no-op");
    assert!(std::fs::symlink_metadata(&link).is_err());
}

/// A repo whose rr-cache is itself a link (to anywhere) gets no link: the target is always a real dir in the git dir.
#[cfg(unix)]
#[test]
fn a_linked_rr_cache_is_not_followed() {
    let elsewhere = tempfile::tempdir().expect("elsewhere");
    let repo = tempfile::tempdir().expect("tmp");
    let rr = repo.path().join("rr-cache");
    std::os::unix::fs::symlink(elsewhere.path(), &rr).expect("symlink");
    let private = tempfile::tempdir().expect("private");
    let link = private.path().join("rr-cache");
    link_rr_cache(&rr, &link).expect("no-op");
    assert!(std::fs::symlink_metadata(&link).is_err());
}

/// The fallback Windows takes without Developer Mode: the symlink is refused here (CI's runner may allow it), so
/// the link must be a junction. Runs on Windows CI only.
#[cfg(windows)]
#[test]
fn a_refused_symlink_falls_back_to_a_junction_that_survives_the_private_dir() {
    let repo = tempfile::tempdir().expect("tmp");
    let rr = real_rr_cache(repo.path());
    let private = tempfile::tempdir().expect("private");
    let link = private.path().join("rr-cache");
    let refuse: fn(&Path, &Path) -> std::io::Result<()> =
        |_, _| Err(std::io::ErrorKind::PermissionDenied.into());
    crate::frozen::link_dir_or_junction(&rr, &link, refuse).expect("junction");
    assert!(
        junction::exists(&link).expect("reparse point"),
        "a junction"
    );
    assert_link_writes_through_and_survives_drop(&rr, private);
}

/// A repo whose rr-cache is a junction gets no link either. Runs on Windows CI only.
#[cfg(windows)]
#[test]
fn a_junctioned_rr_cache_is_not_followed() {
    let elsewhere = tempfile::tempdir().expect("elsewhere");
    let repo = tempfile::tempdir().expect("tmp");
    let rr = repo.path().join("rr-cache");
    junction::create(elsewhere.path(), &rr).expect("junction");
    let private = tempfile::tempdir().expect("private");
    let link = private.path().join("rr-cache");
    link_rr_cache(&rr, &link).expect("no-op");
    assert!(std::fs::symlink_metadata(&link).is_err());
}
