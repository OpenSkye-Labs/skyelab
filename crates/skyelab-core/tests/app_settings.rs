use skyelab_core::{AccountsConfig, AppSettings};

#[test]
fn sharing_is_off_without_a_file_and_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("skyelab").join("settings.toml");
    let off = AppSettings::load(&path).unwrap();
    assert!(!off.share_on_tailnet, "off by default");
    let on = AppSettings {
        share_on_tailnet: true,
        ..AppSettings::default()
    };
    on.save(&path).unwrap();
    assert_eq!(AppSettings::load(&path).unwrap(), on);
    // An empty file is the defaults too.
    std::fs::write(&path, "").unwrap();
    assert!(!AppSettings::load(&path).unwrap().share_on_tailnet);
}

#[test]
fn lives_next_to_accounts_toml_not_in_it() {
    let (Some(settings), Some(accounts)) =
        (AppSettings::default_path(), AccountsConfig::default_path())
    else {
        return; // no config dir on this OS
    };
    assert_eq!(settings.parent(), accounts.parent());
    assert_ne!(settings, accounts);
}

#[test]
fn a_typo_or_wrong_type_is_a_load_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    std::fs::write(&path, "share_on_tailnt = true\n").unwrap();
    assert!(AppSettings::load(&path).is_err());
    std::fs::write(&path, "share_on_tailnet = \"yes\"\n").unwrap();
    assert!(AppSettings::load(&path).is_err());
}

#[test]
fn thread_defaults_round_trip_and_titles_default_on() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.toml");
    assert!(
        skyelab_core::AppSettings::load(&path)
            .expect("load")
            .threads
            .generate_titles
    );
    std::fs::write(
        &path,
        "[threads]\npermission_mode = \"plan\"\nnew_worktree = true\n",
    )
    .expect("write");
    let s = skyelab_core::AppSettings::load(&path).expect("load");
    assert!(s.threads.generate_titles, "a missing key keeps the default");
    assert_eq!(s.threads.permission_mode.as_deref(), Some("plan"));
    assert!(s.threads.new_worktree);
    skyelab_core::AppSettings::update(&path, |s| s.threads.claude_model = Some("haiku".into()))
        .expect("save");
    let s = skyelab_core::AppSettings::load(&path).expect("load");
    assert_eq!(
        s.threads.model_for(skyelab_core::Provider::Claude),
        (Some("haiku".into()), None)
    );
}
