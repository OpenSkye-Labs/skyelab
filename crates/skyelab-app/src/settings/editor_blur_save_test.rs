//! Own UI kit, stage 6a (.agent/plans/2026-11-29-own-ui-kit-text-editing-6a.md), criterion 14: Settings → General saves
//! the editor command when its field loses focus (the field's Blur event), not only on Enter. A guard: it passes
//! with gpui-component's field and must keep passing with ours.

use super::*;
use crate::workspace::thread_extras::prefs;
use gpui::{Focusable as _, TestAppContext};

#[gpui::test]
fn the_editor_command_saves_when_its_field_blurs(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        init(cx);
    });
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.toml");
    std::fs::write(&path, "").expect("write");
    cx.update(|cx| prefs::install(Some(path.clone()), cx));
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut s = Settings::new(None, AccountsConfig::default(), None, window, cx);
        s.show(Section::General, window, cx);
        s
    });
    // GPUI reports focus changes only in the active window (docs/gpui/elements.md).
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    let input = view.read_with(cx, |s, _| s.editor_input.clone());
    cx.update(|window, cx| {
        let f = input.focus_handle(cx);
        window.focus(&f, cx);
    });
    cx.run_until_parked();
    cx.simulate_input("zed");
    assert_eq!(input.read_with(cx, |i, _| i.value().to_string()), "zed", "typed into the field");
    let saved = || skyelab_core::AppSettings::load(&path).expect("load").editor;
    assert_eq!(saved(), None, "typing alone doesn't save");

    view.update_in(cx, |s, window, cx| s.focus.focus(window, cx));
    cx.run_until_parked();
    assert_eq!(saved().as_deref(), Some("zed"), "saved on blur");
}
