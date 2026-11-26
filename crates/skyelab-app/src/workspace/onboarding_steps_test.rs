//! Acceptance: the onboarding's pages. It opens by itself on a fresh install at Welcome with its progress dots;
//! Return and Space go next and Esc goes back only while the page itself has focus (a field keeps its spaces); the
//! Back button goes back; Skip and Done each save `onboarded = true`; Open Onboarding brings it back at Welcome.

use super::onboarding::{Step, first_run};
use super::onboarding_test::{Rig, rig};
use super::threads_test::wait_until;
use super::*;
use gpui::{TestAppContext, VisualTestContext};

fn step(rig: &Rig, cx: &mut VisualTestContext) -> Step {
    rig.setup.read_with(cx, |o, _| o.step)
}

fn keys(k: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(k);
    cx.run_until_parked();
}

fn click(selector: &'static str, cx: &mut VisualTestContext) {
    let at = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} drawn")).center();
    cx.simulate_click(at, gpui::Modifiers::default());
    cx.run_until_parked();
}

/// settings.toml in the rig's temp dir, read at startup like the app does.
fn settings(rig: &Rig, cx: &mut VisualTestContext) -> std::path::PathBuf {
    let path = rig.home.join("settings.toml");
    cx.update(|_, cx| first_run::install(Some(path.clone()), cx));
    path
}

fn saved_onboarded(path: &std::path::Path) -> bool {
    skyelab_core::AppSettings::load(path).is_ok_and(|s| s.onboarded)
}

#[gpui::test]
fn keys_move_between_pages_only_while_the_page_has_focus(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx);
    assert!(rig.shown(cx));
    assert_eq!(step(&rig, cx), Step::Welcome);
    assert!(cx.debug_bounds("onboarding-dots").is_some(), "progress dots drawn");
    keys("enter", cx);
    assert_eq!(step(&rig, cx), Step::Theme);
    keys("space", cx);
    assert_eq!(step(&rig, cx), Step::Agents);
    keys("escape", cx);
    assert_eq!(step(&rig, cx), Step::Theme);
    click("onboarding-back", cx);
    assert_eq!(step(&rig, cx), Step::Welcome, "the Back button");

    // A focused field keeps Space (and its text) for itself.
    rig.setup.update(cx, |o, cx| o.go(Step::Agents, cx));
    cx.run_until_parked();
    rig.rename("claude-work", "", cx);
    keys("a space b", cx);
    assert_eq!(step(&rig, cx), Step::Agents, "Space typed, not Next");
    let typed = rig.setup.read_with(cx, |o, cx| {
        let row = o.found.iter().find(|f| f.found.id == "claude-work").expect("row");
        row.label.as_ref().expect("field").read(cx).value().to_string()
    });
    assert_eq!(typed, "a b");
}

#[gpui::test]
fn skip_saves_onboarded_and_it_stays_closed(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx);
    let path = settings(&rig, cx);
    click("onboarding-skip", cx);
    assert!(!rig.shown(cx));
    wait_until(cx, "onboarded saved", |_| saved_onboarded(&path));
    rig.ws.update(cx, |ws, cx| ws.sync_setup(cx));
    cx.run_until_parked();
    assert!(!rig.shown(cx), "a later sync doesn't bring it back");
}

#[gpui::test]
fn done_saves_onboarded_and_open_onboarding_reopens_at_welcome(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx);
    let path = settings(&rig, cx);
    for _ in 0..4 {
        keys("enter", cx);
    }
    assert_eq!(step(&rig, cx), Step::Done, "Import with nothing ticked continues");
    keys("enter", cx);
    assert!(!rig.shown(cx), "Start building closes it");
    wait_until(cx, "onboarded saved", |_| saved_onboarded(&path));

    let focus = rig.ws.read_with(cx, |ws, _| ws.focus.clone());
    cx.update(|window, cx| {
        window.focus(&focus, cx);
        window.dispatch_action(Box::new(OpenOnboarding), cx)
    });
    cx.run_until_parked();
    assert!(rig.shown(cx));
    assert_eq!(step(&rig, cx), Step::Welcome);
    assert!(cx.update(|_, cx| cx.all_action_names().contains(&"skyelab::OpenOnboarding")));
    assert_eq!(crate::palette::humanize("skyelab::OpenOnboarding"), "Open Onboarding");
}

#[gpui::test]
fn each_step_opens_at_its_top(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx);
    keys("enter", cx);
    rig.setup.update(cx, |o, _| o.scroll.set_offset(gpui::point(gpui::px(0.), gpui::px(-300.))));
    keys("enter", cx);
    assert_eq!(step(&rig, cx), Step::Agents);
    let y = rig.setup.read_with(cx, |o, _| o.scroll.offset().y);
    assert_eq!(y, gpui::px(0.), "the new step starts scrolled to its top");
}

#[gpui::test]
fn the_actions_stay_in_a_short_window(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx);
    cx.simulate_resize(gpui::size(gpui::px(1280.), gpui::px(560.)));
    rig.setup.update(cx, |o, cx| o.go(Step::Agents, cx));
    cx.run_until_parked();
    let next = cx.debug_bounds("onboarding-next").expect("Continue drawn");
    let skip = cx.debug_bounds("onboarding-skip").expect("Skip drawn");
    let h = cx.update(|window, _| window.viewport_size().height);
    assert!(next.bottom() <= h && skip.bottom() <= h, "{next:?} {skip:?} inside {h:?}");
}
