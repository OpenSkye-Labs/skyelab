//! Ways a binding could pick the wrong account: symlinked depth, duplicates, relative paths.

use skyelab_core::{Account, AccountsConfig, Binding};
use std::path::{Path, PathBuf};

fn account(id: &str, dir: &str) -> String {
    format!("[[account]]\nid = \"{id}\"\nlabel = \"{id}\"\nconfig_dir = \"{dir}\"\n")
}

/// Written as the app writes it: with the snapshot taken at bind time.
fn binding(path: &Path, id: &str) -> String {
    let resolved = Binding::new(path.into(), id.into())
        .resolved()
        .map(|r| format!("resolved = {:?}\n", r.display().to_string()))
        .unwrap_or_default();
    format!(
        "[[binding]]\npath = {:?}\naccount = \"{id}\"\n{resolved}",
        path.display().to_string()
    )
}

fn load(toml: &str) -> anyhow::Result<AccountsConfig> {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("accounts.toml");
    std::fs::write(&path, toml).expect("write");
    AccountsConfig::load(&path)
}

#[test]
fn relative_config_dir_rejected_on_load_add_and_update() {
    for bad in ["", "relative/.claude", "~/.claude"] {
        assert!(load(&account("a", bad)).is_err(), "load accepted {bad:?}");
        let mut c = AccountsConfig::default();
        let acc = |dir: &str| Account {
            id: "a".into(),
            label: "A".into(),
            config_dir: PathBuf::from(dir),
        };
        assert!(c.add_account(acc(bad)).is_err(), "add accepted {bad:?}");
        c.add_account(acc("/abs/.claude"))
            .expect("absolute is fine");
        assert!(
            c.update_account(acc(bad)).is_err(),
            "update accepted {bad:?}"
        );
    }
}

#[cfg(unix)]
#[test]
fn symlinked_binding_depth_uses_canonical_path() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let real = std::fs::canonicalize(tmp.path()).expect("canonicalize");
    let deep = real.join("a").join("b").join("c").join("deep");
    std::fs::create_dir_all(&deep).expect("mkdir");
    // `l` -> a/b/c/deep: the written binding path has fewer components than the parent binding.
    let link = real.join("l");
    std::os::unix::fs::symlink(&deep, &link).expect("symlink");
    let parent = real.join("a").join("b");
    let toml = [
        account("outer", "/abs/outer"),
        account("inner", "/abs/inner"),
        binding(&parent, "outer"),
        binding(&link, "inner"),
    ]
    .concat();
    let c = load(&toml).expect("valid");
    let work = deep.join("src");
    std::fs::create_dir_all(&work).expect("mkdir");
    assert_eq!(c.resolve(&work).map(|a| a.id.as_str()), Some("inner"));
    assert_eq!(c.resolve(&deep).map(|a| a.id.as_str()), Some("inner"));
    assert_eq!(
        c.resolve(&parent.join("x")).map(|a| a.id.as_str()),
        Some("outer")
    );
}

#[test]
fn duplicate_account_ids_rejected_on_load() {
    let toml = [account("a", "/abs/one"), account("a", "/abs/two")].concat();
    assert!(load(&toml).is_err());
}

#[test]
fn duplicate_binding_paths_rejected_on_load() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let real = std::fs::canonicalize(tmp.path()).expect("canonicalize");
    let accounts = [account("a", "/abs/a"), account("b", "/abs/b")].concat();

    let same = format!("{accounts}{}{}", binding(&real, "a"), binding(&real, "b"));
    assert!(load(&same).is_err(), "identical paths");

    std::fs::create_dir(real.join("x")).expect("mkdir");
    let alias = format!(
        "{accounts}{}{}",
        binding(&real, "a"),
        binding(&real.join("x").join(".."), "b")
    );
    assert!(load(&alias).is_err(), "x/.. alias");

    #[cfg(unix)]
    {
        let link = real.join("link");
        std::os::unix::fs::symlink(&real, &link).expect("symlink");
        let sym = format!("{accounts}{}{}", binding(&real, "a"), binding(&link, "b"));
        assert!(load(&sym).is_err(), "symlink alias");
    }
}

#[test]
fn empty_or_relative_binding_rejected() {
    let accounts = account("a", "/abs/a");
    for bad in ["", "relative/work", "."] {
        let toml = format!("{accounts}[[binding]]\npath = {bad:?}\naccount = \"a\"\n");
        assert!(load(&toml).is_err(), "load accepted binding {bad:?}");
        let mut c = load(&accounts).expect("valid");
        let b = Binding::new(PathBuf::from(bad), "a".into());
        assert!(c.add_binding(b).is_err(), "add_binding accepted {bad:?}");
        assert!(c.resolve(Path::new("/anywhere")).is_none());
    }
}
