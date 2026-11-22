//! A folder that doesn't exist yet, written through a symlink or `..`, must resolve to the binding that owns its real location.

use skyelab_core::AccountsConfig;
use std::path::Path;

fn account(id: &str, dir: &str) -> String {
    format!("[[account]]\nid = \"{id}\"\nlabel = \"{id}\"\nconfig_dir = \"{dir}\"\n")
}

fn binding(path: &Path, id: &str) -> String {
    format!(
        "[[binding]]\npath = {:?}\naccount = \"{id}\"\n",
        path.display().to_string()
    )
}

#[cfg(unix)]
#[test]
fn nonexistent_dir_through_symlink_resolves_to_owning_binding() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(tmp.path()).expect("canonicalize");
    let deep = root.join("a").join("b").join("c").join("deep");
    std::fs::create_dir_all(&deep).expect("mkdir");
    std::fs::create_dir(root.join("x")).expect("mkdir");
    std::os::unix::fs::symlink(&deep, root.join("l")).expect("symlink");

    let toml = [
        account("outer", "/abs/outer"),
        account("inner", "/abs/inner"),
        binding(&root, "outer"),
        binding(&deep, "inner"),
    ]
    .concat();
    let cfg_dir = tempfile::tempdir().expect("tempdir");
    let path = cfg_dir.path().join("accounts.toml");
    std::fs::write(&path, toml).expect("write");
    let c = AccountsConfig::load(&path).expect("valid");

    let via_link = root.join("l").join("newproj");
    assert_eq!(
        c.resolve(&via_link).map(|a| a.id.as_str()),
        Some("inner"),
        "WRONG ACCOUNT via symlink for not-yet-existing folder"
    );
    let via_dotdot = root.join("x").join("..").join("a/b/c/deep/newproj");
    assert_eq!(
        c.resolve(&via_dotdot).map(|a| a.id.as_str()),
        Some("inner"),
        "WRONG ACCOUNT via .. for not-yet-existing folder"
    );
}
