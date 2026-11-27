//! Acceptance: every way into Settings opens it as a modal over the window; the page under it never changes.

#[cfg(unix)] // unix-only as it was before it moved under this module
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

/// QA: Tab past the last control of a Settings page cycles inside the modal, never to the sidebar behind it.
#[gpui::test]
fn tab_stays_inside_the_settings_modal(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    cx.update(|window, cx| {
        window.dispatch_action(
            Box::new(OpenSettingsSection {
                section: Section::General,
            }),
            cx,
        )
    });
    cx.run_until_parked();
    let settings = ws.read_with(cx, |ws, _| ws.settings.clone().expect("settings"));
    let inside = |cx: &mut VisualTestContext| {
        let focus = settings.read_with(cx, gpui::Focusable::focus_handle);
        cx.update(|window, cx| focus.contains_focused(window, cx))
    };
    assert!(inside(cx), "opening Settings focuses it");
    for i in 0..40 {
        cx.simulate_keystrokes("tab");
        assert!(inside(cx), "TAB_LEFT_THE_MODAL after {} tabs", i + 1);
    }
    for i in 0..40 {
        cx.simulate_keystrokes("shift-tab");
        assert!(inside(cx), "SHIFT_TAB_LEFT_THE_MODAL after {} tabs", i + 1);
    }
}

/// QA: Tabbing to below-fold controls in Settings → General scrolls `settings-body` down to keep the focused control
/// visible; Shift-Tab back up scrolls back toward the top, and switching sections resets scroll to 0.
#[gpui::test]
fn tab_scrolls_below_fold_controls_into_view_and_shift_tab_returns(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    // Short window so General's Updates section sits well below the fold.
    cx.simulate_resize(gpui::size(gpui::px(1100.), gpui::px(420.)));
    cx.update(|window, cx| {
        window.dispatch_action(
            Box::new(OpenSettingsSection {
                section: Section::General,
            }),
            cx,
        )
    });
    cx.run_until_parked();
    let settings = ws.read_with(cx, |ws, _| ws.settings.clone().expect("settings"));
    let inside = |cx: &mut VisualTestContext| {
        let focus = settings.read_with(cx, gpui::Focusable::focus_handle);
        cx.update(|window, cx| focus.contains_focused(window, cx))
    };
    let scroll_y = |cx: &mut VisualTestContext| {
        settings.read_with(cx, |s, _| s.body_scroll.handle().offset().y)
    };
    let max_y = settings.read_with(cx, |s, _| s.body_scroll.handle().max_offset().y);
    assert!(
        max_y > gpui::px(100.),
        "General must overflow vertically in a short window; max_y={max_y:?}"
    );
    assert_eq!(scroll_y(cx), gpui::px(0.), "starts scrolled to top");

    // Tab forward down through Editor, Terminal, Quitting, Advanced, and into Updates.
    let mut lowest_y = gpui::px(0.);
    let mut lowest_step = 0;
    for step in 1..=14 {
        cx.simulate_keystrokes("tab");
        assert!(inside(cx), "TAB_LEFT_THE_MODAL at step {step}");
        let y = scroll_y(cx);
        if y < lowest_y {
            lowest_y = y;
            lowest_step = step;
        }
    }
    assert!(
        lowest_y < gpui::px(-80.),
        "Tabbing into below-fold Updates did not scroll settings-body down: lowest_y={lowest_y:?}"
    );

    // Stop at the scrolled-down position (re-tab if the last step wrapped to top) and Shift-Tab back up.
    if scroll_y(cx) > lowest_y + gpui::px(20.) {
        for _ in 0..lowest_step {
            cx.simulate_keystrokes("tab");
            assert!(inside(cx), "TAB_LEFT_THE_MODAL re-reaching Updates");
            if scroll_y(cx) <= lowest_y + gpui::px(20.) {
                break;
            }
        }
    }
    let scrolled_down = scroll_y(cx);
    assert!(
        scrolled_down < gpui::px(-80.),
        "expected scrolled-down offset before Shift-Tab; got {scrolled_down:?}"
    );

    let mut highest_after_back = scrolled_down;
    for step in 1..=12 {
        cx.simulate_keystrokes("shift-tab");
        assert!(inside(cx), "SHIFT_TAB_LEFT_THE_MODAL at step {step}");
        let y = scroll_y(cx);
        if y > highest_after_back {
            highest_after_back = y;
        }
    }
    assert!(
        highest_after_back > scrolled_down + gpui::px(60.),
        "Shift-Tab did not scroll settings-body back toward the top: down={scrolled_down:?}, back={highest_after_back:?}"
    );

    // Switching sections resets scroll offset to the top.
    settings.update(cx, |s, _| {
        s.body_scroll
            .handle()
            .set_offset(gpui::point(gpui::px(0.), gpui::px(-120.)));
    });
    settings.update_in(cx, |s, window, cx| s.show(Section::Appearance, window, cx));
    cx.run_until_parked();
    assert_eq!(
        scroll_y(cx),
        gpui::px(0.),
        "switching section must reset body scroll to top"
    );
}
