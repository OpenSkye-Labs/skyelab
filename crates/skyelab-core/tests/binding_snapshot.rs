#![cfg(unix)]
//! A binding remembers the real folder it was confirmed on. If that folder later turns into
//! something else (a symlink to `/`), the binding goes stale and stops picking an account.

use skyelab_core::{Account, AccountsConfig, Binding};
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

fn config(path: &Path) -> AccountsConfig {
    let mut c = AccountsConfig::default();
    c.add_account(Account {
        id: "acme".into(),
        label: "Acme".into(),
        config_dir: "/home/me/.claude-acme".into(),
        provider: Default::default(),
        gh_config_dir: None,
        ssh_agent: false,
    })
    .expect("account");
    c.add_binding(Binding::new(path.into(), "acme".into()))
        .expect("bind");
    c
}

fn id(c: &AccountsConfig, dir: &Path) -> Option<String> {
    c.resolve(dir).map(|a| a.id.clone())
}

fn real_tempdir() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let real = std::fs::canonicalize(dir.path()).expect("canonicalize");
    (dir, real)
}

#[test]
fn swapped_for_symlink_is_stale() {
    let (_tmp, root) = real_tempdir();
    let acme = root.join("work/acme");
    let elsewhere = root.join("elsewhere");
    std::fs::create_dir_all(acme.join("api")).expect("mkdir");
    std::fs::create_dir_all(elsewhere.join("secret")).expect("mkdir");
    let c = config(&acme);
    assert_eq!(id(&c, &acme.join("api")).as_deref(), Some("acme"));
    assert!(!c.bindings[0].is_stale());

    std::fs::remove_dir_all(&acme).expect("rm");
    symlink(&elsewhere, &acme).expect("symlink");

    assert!(c.bindings[0].is_stale());
    assert_eq!(id(&c, &elsewhere), None);
    assert_eq!(id(&c, &elsewhere.join("secret")), None);
    assert_eq!(id(&c, &acme.join("secret")), None);
    assert!(c.launch_env(&elsewhere).is_err());
}

#[test]
fn swapped_for_symlink_to_root_does_not_claim_the_disk() {
    let (_tmp, root) = real_tempdir();
    let acme = root.join("acme");
    std::fs::create_dir(&acme).expect("mkdir");
    let c = config(&acme);
    std::fs::remove_dir(&acme).expect("rm");
    symlink("/", &acme).expect("symlink");
    assert_eq!(id(&c, Path::new("/")), None);
    assert_eq!(id(&c, Path::new("/tmp")), None);
}

#[test]
fn stale_binding_blocks_fallback_to_a_shallower_one() {
    let (_tmp, root) = real_tempdir();
    let work = root.join("work");
    let sub = work.join("sub");
    let other = work.join("other");
    std::fs::create_dir_all(&sub).expect("mkdir");
    std::fs::create_dir_all(&other).expect("mkdir");
    let mut c = config(&work);
    c.add_account(Account {
        id: "personal".into(),
        label: "Personal".into(),
        config_dir: "/home/me/.claude".into(),
        provider: Default::default(),
        gh_config_dir: None,
        ssh_agent: false,
    })
    .expect("account");
    c.add_binding(Binding::new(sub.clone(), "personal".into()))
        .expect("bind");
    assert_eq!(id(&c, &sub).as_deref(), Some("personal"));

    std::fs::remove_dir(&sub).expect("rm");
    symlink(&other, &sub).expect("symlink");
    // /work/other is under the valid /work binding, but the stale /work/sub now points there: ask.
    assert_eq!(id(&c, &other), None);
    assert_eq!(id(&c, &sub), None);
    // A folder only the valid binding covers still resolves.
    std::fs::create_dir(work.join("plain")).expect("mkdir");
    assert_eq!(id(&c, &work.join("plain")).as_deref(), Some("acme"));
}

#[test]
fn symlink_bound_as_symlink_still_resolves() {
    let (_tmp, root) = real_tempdir();
    let real = root.join("real");
    let link = root.join("link");
    std::fs::create_dir_all(real.join("src")).expect("mkdir");
    symlink(&real, &link).expect("symlink");
    let c = config(&link);
    assert!(!c.bindings[0].is_stale());
    assert_eq!(c.bindings[0].resolved(), Some(real.as_path()));
    assert_eq!(id(&c, &link.join("src")).as_deref(), Some("acme"));
    assert_eq!(id(&c, &real.join("src")).as_deref(), Some("acme"));

    // Retargeting the link is a swap too.
    let other = root.join("other");
    std::fs::create_dir(&other).expect("mkdir");
    std::fs::remove_file(&link).expect("rm link");
    symlink(&other, &link).expect("symlink");
    assert!(c.bindings[0].is_stale());
    assert_eq!(id(&c, &other), None);
    assert_eq!(
        id(&c, &real.join("src")),
        None,
        "where it pointed when confirmed"
    );
}

#[test]
fn reconfirm_updates_snapshot() {
    let (_tmp, root) = real_tempdir();
    let acme = root.join("acme");
    let moved = root.join("moved");
    std::fs::create_dir(&acme).expect("mkdir");
    std::fs::create_dir(&moved).expect("mkdir");
    let mut c = config(&acme);
    std::fs::remove_dir(&acme).expect("rm");
    symlink(&moved, &acme).expect("symlink");
    assert_eq!(id(&c, &moved), None);

    assert!(
        c.reconfirm_binding(&acme, &acme).is_err(),
        "must match what the user saw"
    );
    c.reconfirm_binding(&acme, &moved).expect("reconfirm");
    assert!(!c.bindings[0].is_stale());
    assert_eq!(c.bindings[0].resolved(), Some(moved.as_path()));
    assert_eq!(id(&c, &moved).as_deref(), Some("acme"));
    assert!(c.reconfirm_binding(&root.join("unbound"), &root).is_err());
}

#[test]
fn snapshot_round_trips() {
    let (_tmp, root) = real_tempdir();
    let acme = root.join("acme");
    std::fs::create_dir(&acme).expect("mkdir");
    let c = config(&acme);
    let file = root.join("cfg/accounts.toml");
    c.save(&file).expect("save");
    let text = std::fs::read_to_string(&file).expect("read");
    assert!(text.contains("resolved = "), "{text}");
    let loaded = AccountsConfig::load(&file).expect("load");
    assert_eq!(loaded, c);
    assert!(!loaded.bindings[0].is_stale());
    assert_eq!(id(&loaded, &acme).as_deref(), Some("acme"));
}

#[test]
fn load_and_save_never_resnapshot() {
    let (_tmp, root) = real_tempdir();
    let acme = root.join("acme");
    let other = root.join("other");
    std::fs::create_dir(&acme).expect("mkdir");
    std::fs::create_dir(&other).expect("mkdir");
    let file = root.join("accounts.toml");
    config(&acme).save(&file).expect("save");
    std::fs::remove_dir(&acme).expect("rm");
    symlink(&other, &acme).expect("symlink");

    let loaded = AccountsConfig::load(&file).expect("load");
    assert!(loaded.bindings[0].is_stale());
    loaded.save(&file).expect("save again");
    let reloaded = AccountsConfig::load(&file).expect("load");
    assert!(reloaded.bindings[0].is_stale());
    assert_eq!(id(&reloaded, &other), None);
}

#[test]
fn legacy_config_is_stale() {
    let (_tmp, root) = real_tempdir();
    let acme = root.join("acme");
    std::fs::create_dir(&acme).expect("mkdir");
    let file = root.join("accounts.toml");
    let toml = format!(
        "[[account]]\nid = \"acme\"\nlabel = \"Acme\"\nconfig_dir = \"/home/me/.claude-acme\"\n\
         [[binding]]\npath = {:?}\naccount = \"acme\"\n",
        acme.display().to_string()
    );
    std::fs::write(&file, toml).expect("write");
    let mut c = AccountsConfig::load(&file).expect("old configs still load");
    assert!(c.bindings[0].is_stale());
    assert_eq!(c.bindings[0].resolved(), None);
    assert_eq!(id(&c, &acme), None);
    c.reconfirm_binding(&acme, &acme).expect("reconfirm");
    assert_eq!(id(&c, &acme).as_deref(), Some("acme"));
}

#[test]
fn missing_folder_stays_valid_until_it_appears_elsewhere() {
    let (_tmp, root) = real_tempdir();
    let acme = root.join("drive/work/acme"); // nothing of drive/ exists yet
    let c = config(&acme);
    assert!(!c.bindings[0].is_stale());
    assert_eq!(id(&c, &acme.join("api")).as_deref(), Some("acme"));

    // Cloned or plugged in as a real folder: still the confirmed place.
    std::fs::create_dir_all(acme.join("api")).expect("mkdir");
    assert!(!c.bindings[0].is_stale());
    assert_eq!(id(&c, &acme.join("api")).as_deref(), Some("acme"));

    // It shows up as a symlink to somewhere else: stale.
    let elsewhere = root.join("elsewhere");
    std::fs::create_dir(&elsewhere).expect("mkdir");
    std::fs::remove_dir_all(root.join("drive")).expect("rm");
    std::fs::create_dir_all(root.join("drive/work")).expect("mkdir");
    symlink(&elsewhere, &acme).expect("symlink");
    assert!(c.bindings[0].is_stale());
    assert_eq!(id(&c, &elsewhere), None);
}

#[test]
fn stale_link_still_blocks_where_it_pointed_when_confirmed() {
    let (_tmp, root) = real_tempdir();
    let real = root.join("real");
    let other = root.join("other");
    let link = root.join("link");
    std::fs::create_dir_all(real.join("src")).expect("mkdir");
    std::fs::create_dir(&other).expect("mkdir");
    symlink(&real, &link).expect("symlink");
    // acme covers the whole root; personal is bound through the link.
    let mut c = config(&root);
    c.add_account(Account {
        id: "personal".into(),
        label: "Personal".into(),
        config_dir: "/home/me/.claude".into(),
        provider: Default::default(),
        gh_config_dir: None,
        ssh_agent: false,
    })
    .expect("account");
    c.add_binding(Binding::new(link.clone(), "personal".into()))
        .expect("bind");
    assert_eq!(id(&c, &real.join("src")).as_deref(), Some("personal"));

    std::fs::remove_file(&link).expect("rm link");
    symlink(&other, &link).expect("retarget");
    // real/src was personal's; acme must not quietly take it over.
    assert_eq!(id(&c, &real.join("src")), None, "OLD_TARGET_FELL_BACK");
    assert_eq!(id(&c, &other), None);
}
