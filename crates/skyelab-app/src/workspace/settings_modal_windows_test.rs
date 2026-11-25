//! Acceptance: Esc and a backdrop click close the Settings modal, it reopens on its last section, and each window has
//! its own modal.

use crate::settings::Section;
use crate::workspace::multi_window_test::in_window;
use crate::workspace::threads_test::{fixture, open_workspace, setup};
use crate::workspace::*;
use gpui::{Modifiers, TestAppContext, VisualTestContext, point, px};

fn open(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> (bool, bool) {
    let drawn = cx.debug_bounds("settings-backdrop").is_some();
    (ws.read_with(cx, |ws, _| ws.settings_open), drawn)
}

#[gpui::test]
fn escape_and_backdrop_close_and_the_section_is_remembered(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    cx.dispatch_action(OpenSettingsSection {
        section: Section::Appearance,
    });
    cx.run_until_parked();
    assert_eq!(open(&ws, cx), (true, true));

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(open(&ws, cx), (false, false), "Esc closes it");

    cx.dispatch_action(OpenSettings);
    cx.run_until_parked();
    let section = ws.read_with(cx, |ws, cx| {
        ws.settings.as_ref().map(|s| s.read(cx).section)
    });
    assert_eq!(
        section,
        Some(Section::Appearance),
        "reopens on the last section"
    );

    // The backdrop: right of the left rail, below the title band, outside the modal's 40 px margin.
    let size = cx.update(|window, _| window.viewport_size());
    cx.simulate_click(
        point(crate::ui::RAIL_W + px(12.), size.height - px(12.)),
        Modifiers::none(),
    );
    cx.run_until_parked();
    assert_eq!(open(&ws, cx), (false, false), "a backdrop click closes it");

    // Esc while adding an account closes the form first, then the modal.
    cx.dispatch_action(OpenSettingsSection {
        section: Section::Accounts,
    });
    cx.run_until_parked();
    let settings = ws.read_with(cx, |ws, _| ws.settings.clone().expect("settings"));
    settings.update(cx, |s, cx| {
        s.adding = true;
        cx.notify();
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(
        !settings.read_with(cx, |s, _| s.adding),
        "Esc closed the add card"
    );
    assert_eq!(open(&ws, cx), (true, true), "the modal stays");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(open(&ws, cx), (false, false));
}

#[gpui::test]
fn each_window_has_its_own_modal(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (a, cx) = open_workspace(&f, cx);
    cx.dispatch_action(NewWindow);
    cx.run_until_parked();
    let handles = cx.update(|_, cx| window_handles(cx));
    assert_eq!(handles.len(), 2);
    let b = cx.update(|_, cx| windows::workspaces(cx)[1].clone());
    let a_window = a.read_with(cx, |ws, _| ws.window_handle);
    in_window(a_window, &a, cx, |ws, window, cx| {
        ws.open_settings_at(Section::Chat, window, cx)
    });
    in_window(handles[1], &b, cx, |ws, window, cx| {
        ws.open_settings_at(Section::Keys, window, cx)
    });
    cx.run_until_parked();
    let sections = cx.update(|_, cx| {
        let of = |ws: &Entity<Workspace>| {
            let ws = ws.read(cx);
            (
                ws.settings_open,
                ws.settings.as_ref().map(|s| s.read(cx).section),
            )
        };
        (of(&a), of(&b))
    });
    assert_eq!(
        sections,
        ((true, Some(Section::Chat)), (true, Some(Section::Keys)))
    );
    let settings_differ = cx.update(|_, cx| a.read(cx).settings != b.read(cx).settings);
    assert!(settings_differ, "each window has its own Settings view");
    // Closing B's leaves A's up.
    in_window(handles[1], &b, cx, |ws, window, cx| {
        ws.close_settings(window, cx)
    });
    cx.run_until_parked();
    let open = cx.update(|_, cx| (a.read(cx).settings_open, b.read(cx).settings_open));
    assert_eq!(open, (true, false), "B's modal closed, A's stays");
}

#[gpui::test]
fn focus_leaving_the_modal_closes_it(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    cx.update(|window, _| window.activate_window()); // GPUI reports focus changes only in the active window
    cx.dispatch_action(OpenSettingsSection {
        section: Section::Appearance,
    });
    cx.run_until_parked();
    assert_eq!(open(&ws, cx), (true, true));
    // F6 moves the focus to an area of the window behind the modal: the modal goes, nothing is left hidden under it.
    cx.simulate_keystrokes("f6");
    cx.run_until_parked();
    assert_eq!(
        open(&ws, cx),
        (false, false),
        "F6 left the modal, so it closed"
    );

    // A field inside it keeps it open.
    cx.dispatch_action(OpenSettingsSection {
        section: Section::General,
    });
    cx.run_until_parked();
    let search = ws.read_with(cx, |ws, cx| {
        ws.settings
            .as_ref()
            .map(|s| s.read(cx).search_input.clone())
    });
    let search = search.expect("settings");
    search.update_in(cx, |s, window, cx| s.focus(window, cx));
    cx.run_until_parked();
    assert_eq!(
        open(&ws, cx),
        (true, true),
        "focus inside the modal keeps it"
    );
}

#[gpui::test]
fn what_took_the_focus_keeps_it_when_the_modal_closes(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    cx.update(|window, _| window.activate_window());
    let before = cx.update(|window, cx| window.focused(cx));
    cx.dispatch_action(OpenSettingsSection {
        section: Section::Appearance,
    });
    cx.run_until_parked();
    cx.simulate_keystrokes(crate::palette::OPEN_PALETTE_KEY);
    cx.run_until_parked();
    assert!(
        !ws.read_with(cx, |ws, _| ws.settings_open),
        "the palette closed it"
    );
    assert!(
        cx.update(gpui_component::WindowExt::has_active_dialog),
        "the palette is open"
    );
    let now = cx.update(|window, cx| window.focused(cx));
    assert!(
        now.is_some() && now != before,
        "the palette lost its focus to the page behind"
    );
}
