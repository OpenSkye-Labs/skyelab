//! A folder that isn't a git repo still skips what its .gitignore names.

use skyelab_core::files::walk;

#[test]
fn walk_honours_gitignore_outside_a_repo() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join(".gitignore"), "target/\n").expect("write");
    std::fs::create_dir(dir.path().join("target")).expect("mkdir");
    std::fs::write(dir.path().join("target/out.rs"), "x").expect("write");
    std::fs::write(dir.path().join("a.rs"), "x").expect("write");
    assert_eq!(walk(dir.path(), 100), [".gitignore", "a.rs"]);
}
