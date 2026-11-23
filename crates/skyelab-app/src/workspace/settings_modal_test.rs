//! Acceptance: every way into Settings opens it as a modal over the window; the page under it never changes.
#![cfg(unix)]

#[path = "settings_reopen_stale_test.rs"]
mod reopen_stale;
#[path = "settings_modal_windows_test.rs"]
mod windows;

use super::center::Page;
use super::threads_test::{fixture, open_workspace, setup};
use super::*;
use crate::settings::Section;
use gpui::{TestAppContext, VisualTestContext};

fn state(
    ws: &Entity<Workspace>,
    cx: &mut VisualTestContext,
) -> (bool, bool, Page, Option<Section>) {
    let drawn = cx.debug_bounds("settings-backdrop").is_some();
    ws.read_with(cx, |ws, cx| {
        (
            ws.settings_open,
            drawn,
            ws.page.clone(),
            ws.settings.as_ref().map(|s| s.read(cx).section),
        )
    })
}

fn close(ws: &Entity<Workspace>, cx: &mut VisualTestContext) {
    ws.update_in(cx, |ws, window, cx| ws.close_settings(window, cx));
    cx.run_until_parked();
}

#[gpui::test]
fn every_open_path_shows_the_modal_and_keeps_the_page(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let opens: [(Box<dyn gpui::Action>, Section); 4] = [
        (Box::new(OpenSettings), Section::Accounts),
        (Box::new(OpenUsage), Section::Usage),
        (Box::new(crate::remote::OpenRemote), Section::Sharing),
        (
            Box::new(OpenSettingsSection {
                section: Section::Diffs,
            }),
            Section::Diffs,
        ),
    ];
    for (action, section) in opens {
        cx.update(|window, cx| window.dispatch_action(action.boxed_clone(), cx));
        cx.run_until_parked();
        assert_eq!(
            state(&ws, cx),
            (true, true, Page::Welcome, Some(section)),
            "{} opens the modal on {section:?}",
            action.name()
        );
        close(&ws, cx);
        let (open, drawn, ..) = state(&ws, cx);
        assert!(!open && !drawn, "closed after {}", action.name());
    }
    ws.read_with(cx, |ws, cx| {
        let settings = ws.settings.clone().expect("made on first open");
        assert!(
            ws.dock
                .read(cx)
                .panel(settings.entity_id().into())
                .is_none(),
            "Settings is not a tab"
        );
    });
}

#[gpui::test]
fn a_deep_link_moves_an_open_modal(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    cx.dispatch_action(OpenSettings);
    cx.run_until_parked();
    ws.update_in(cx, |ws, window, cx| {
        ws.open_settings_section(
            &OpenSettingsSection {
                section: Section::SourceControl,
            },
            window,
            cx,
        )
    });
    cx.run_until_parked();
    assert_eq!(
        state(&ws, cx),
        (true, true, Page::Welcome, Some(Section::SourceControl))
    );
}
