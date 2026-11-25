use super::*;

#[test]
fn send_shortcut_defaults_to_enter_and_round_trips_through_settings_toml() {
    assert_eq!(
        AppSettings::default().threads.send_shortcut,
        SendShortcut::Enter
    );
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.toml");
    for value in [
        SendShortcut::ModEnterMultiline,
        SendShortcut::ModEnter,
        SendShortcut::Enter,
    ] {
        AppSettings::update(&path, |s| s.threads.send_shortcut = value).expect("save");
        assert_eq!(
            AppSettings::load(&path)
                .expect("load")
                .threads
                .send_shortcut,
            value
        );
    }
    AppSettings::update(&path, |s| {
        s.threads.send_shortcut = SendShortcut::ModEnterMultiline
    })
    .expect("save");
    let text = std::fs::read_to_string(&path).expect("read");
    assert!(
        text.contains("send_shortcut = \"mod-enter-multiline\""),
        "{text}"
    );
}

#[test]
fn enter_sends_follows_each_setting() {
    assert!(SendShortcut::Enter.enter_sends("a\nb"));
    assert!(SendShortcut::ModEnterMultiline.enter_sends("one line"));
    assert!(!SendShortcut::ModEnterMultiline.enter_sends("two\nlines"));
    assert!(!SendShortcut::ModEnterMultiline.enter_sends("cr\rline"));
    assert!(!SendShortcut::ModEnter.enter_sends("hi"));
}
