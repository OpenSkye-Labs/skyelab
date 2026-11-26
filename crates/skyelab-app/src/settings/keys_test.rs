//! Settings → Keyboard shortcuts: every action with its keys now, the file's marked custom, and the search.

use super::*;
use gpui::TestAppContext;

fn row<'a>(v: &'a Settings, id: &str) -> Option<&'a keys::KeyRow> {
    v.keys.iter().find(|r| r.id.as_ref() == id)
}

#[gpui::test]
fn lists_actions_with_their_keys_and_follows_the_file(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("keybindings.toml");
    std::fs::write(
        &file,
        "[[binding]]\nkey = \"f9\"\naction = \"BindFolder\"\ncontext = \"Settings\"\n",
    )
    .expect("write");
    cx.update(|cx| {
        gpui_component::init(cx);
        init(cx);
        crate::keymap::init(Some(file.clone()), cx);
    });
    let (view, cx) = cx.add_window_view(|window, cx| {
        Settings::new(None, AccountsConfig::default(), None, window, cx)
    });
    view.read_with(cx, |v, _| {
        let bind = row(v, "skyelab::BindFolder").expect("BindFolder listed");
        assert_eq!(bind.binds.len(), 2, "the file's key and the default");
        assert!(bind.binds[0].custom, "the file's key comes first: it wins");
        assert_eq!(bind.binds[0].context.as_ref(), "Settings");
        assert!(!bind.binds[1].custom);
        let save = row(v, "skyelab::SaveAccount").expect("SaveAccount listed");
        assert!(!save.binds.is_empty() && !save.binds[0].custom);
        assert!(
            row(v, "skyelab::OpenCommandPalette").is_none(),
            "hidden actions stay hidden"
        );
    });

    let search = view.read_with(cx, |v, _| v.keys_search.clone());
    view.update(cx, |v, cx| {
        v.section = Section::Keys; // the search box draws, so it can take keys
        cx.notify();
    });
    cx.run_until_parked();
    search.update_in(cx, |s, window, cx| s.focus(window, cx));
    cx.simulate_input("f9"); // typed, so the input reports the change
    cx.run_until_parked();
    view.read_with(cx, |v, _| {
        let ids: Vec<&str> = v.keys.iter().map(|r| r.id.as_ref()).collect();
        assert_eq!(ids, ["skyelab::BindFolder"], "keys are searchable");
    });

    // The file changes: the list follows without reopening Settings.
    std::fs::write(&file, "").expect("clear");
    cx.simulate_keystrokes("backspace backspace");
    cx.simulate_input("bind folder");
    cx.executor().advance_clock(crate::keymap::POLL * 2);
    cx.run_until_parked();
    view.read_with(cx, |v, _| {
        let bind = row(v, "skyelab::BindFolder").expect("still listed");
        assert!(bind.binds.iter().all(|b| !b.custom));
    });
}

#[test]
fn a_context_reads_as_words_not_as_a_key() {
    assert_eq!(super::keys::context_words("BrowserFind"), "in Browser Find");
    assert_eq!(super::keys::context_words("Terminal"), "in Terminal");
    assert_eq!(
        super::keys::context_words("Thread > Composer"),
        "when Thread > Composer"
    );
}
