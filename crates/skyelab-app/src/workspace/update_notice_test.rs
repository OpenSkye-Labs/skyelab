//! The update pill's label, icon and inline action for every update state, and where it collapses.

use super::update_notice::{Fit, Inline, expanded, fit, look};
use crate::ui::SkyeIcon;
use crate::updates::{Step, UpdateStatus};
use gpui::px;

fn updating(step: Step) -> UpdateStatus {
    UpdateStatus::Updating {
        tag: "v9.9.9".into(),
        url: String::new(),
        step,
    }
}

#[test]
fn each_state_has_a_short_label_and_only_a_failure_has_an_inline_action() {
    let available = UpdateStatus::Available {
        tag: "v9.9.9".into(),
        url: String::new(),
    };
    let label = |s: &UpdateStatus, folder| look(s, folder).expect("shown").label;
    assert_eq!(label(&available, false), "Update available");
    let half = updating(Step::Downloading {
        done: 42,
        total: Some(100),
    });
    assert_eq!(label(&half, false), "Downloading… 42%");
    let unknown = updating(Step::Downloading {
        done: 42,
        total: None,
    });
    assert_eq!(label(&unknown, false), "Downloading…");
    let all_in = updating(Step::Downloading {
        done: 100,
        total: Some(100),
    });
    assert_eq!(label(&all_in, false), "Checking…");
    assert_eq!(label(&updating(Step::Ready), false), "Ready to restart");
    assert_eq!(label(&updating(Step::Ready), true), "Ready to install");
    let failed = look(&updating(Step::Failed("no network".into())), false).expect("shown");
    assert_eq!(failed.label, "Update failed");
    assert!(failed.failed);
    assert_eq!(failed.icon, SkyeIcon::CircleAlert);
    assert_eq!(failed.inline, Some(Inline::Retry));
    for s in [&available, &half, &updating(Step::Ready)] {
        let l = look(s, false).expect("shown");
        assert!(!l.failed && l.inline.is_none(), "{l:?}");
    }
    for s in [
        UpdateStatus::Idle,
        UpdateStatus::UpToDate,
        UpdateStatus::Failed("x".into()),
    ] {
        assert_eq!(look(&s, false), None);
    }
}

#[test]
fn the_pill_collapses_to_a_circle_under_80_px_of_room() {
    assert!(!expanded(px(0.)));
    assert!(!expanded(px(79.)));
    assert!(expanded(px(80.)));
}

#[test]
fn the_open_pill_drops_the_icon_then_the_full_label_then_the_action() {
    let f = |full, inline, icon| Fit { full, inline, icon };
    // Labels 90 and 40 wide, no action: 24 of padding, the icon adds 20.
    assert_eq!(fit(px(90.), px(40.), None, px(134.)), f(true, false, true));
    assert_eq!(fit(px(90.), px(40.), None, px(114.)), f(true, false, false));
    assert_eq!(fit(px(90.), px(40.), None, px(84.)), f(false, false, true));
    assert_eq!(fit(px(90.), px(40.), None, px(64.)), f(false, false, false));
    // With a 30 wide Retry (plus its 6 of gap).
    assert_eq!(
        fit(px(90.), px(40.), Some(px(30.)), px(150.)),
        f(true, true, false)
    );
    assert_eq!(
        fit(px(90.), px(40.), Some(px(30.)), px(100.)),
        f(false, true, false)
    );
    assert_eq!(
        fit(px(90.), px(40.), Some(px(30.)), px(80.)),
        f(false, false, false)
    );
}
