use std::path::Path;

use skyelab_store::Store;

#[test]
fn deleted_ids_are_not_reused() {
    let s = Store::open_in_memory().unwrap();
    let p1 = s.create_project("p1", Path::new("/p1")).unwrap();
    let t1 = s
        .create_thread(p1.id, "a", Path::new("/p1"), "acct-a", "idle")
        .unwrap();
    s.delete_thread(t1.id).unwrap();
    let t2 = s
        .create_thread(p1.id, "b", Path::new("/p1"), "acct-b", "idle")
        .unwrap();
    assert_ne!(
        t1.id, t2.id,
        "a stale handle to a deleted thread must not hit a new one"
    );

    s.delete_thread(t2.id).unwrap();
    s.delete_project(p1.id).unwrap();
    let p2 = s.create_project("p2", Path::new("/p2")).unwrap();
    assert_ne!(p1.id, p2.id);
}

#[test]
fn trailing_slash_is_same_project() {
    let s = Store::open_in_memory().unwrap();
    let p = s.create_project("a", Path::new("/repo/")).unwrap();
    assert_eq!(p.path, Path::new("/repo"));
    assert!(s.create_project("b", Path::new("/repo")).is_err());
}
