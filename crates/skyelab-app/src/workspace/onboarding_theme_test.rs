//! Acceptance: the Theme page applies a family and a mode to the whole app at once and saves them where Settings →
//! Appearance does (`[appearance]` in settings.toml).

use super::onboarding::Step;
use super::onboarding_test::rig;
use super::threads_test::wait_until;
use gpui::TestAppContext;
use gpui_component::ActiveTheme as _;

#[gpui::test]
fn picking_a_family_and_dark_applies_and_saves_both(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx);
    cx.update(|_, cx| crate::ui::install_theme(cx).expect("themes load"));
    let path = rig.home.join("settings.toml");
    cx.update(|_, cx| crate::ui::themes::load(Some(path.clone()), cx));
    rig.setup.update(cx, |o, cx| o.go(Step::Theme, cx));
    cx.run_until_parked();
    assert!(cx.debug_bounds("onboarding-preview").is_some(), "the miniature is drawn");
    let at = cx.debug_bounds("onboarding-family-grove").expect("grove row").center();
    cx.simulate_click(at, gpui::Modifiers::default());
    cx.run_until_parked();
    rig.setup.update(cx, |o, cx| o.pick_mode(2, cx));
    cx.run_until_parked();
    let (family, dark) = cx.update(|_, cx| (crate::ui::themes::get(cx).theme, cx.theme().mode.is_dark()));
    assert_eq!(family, "grove");
    assert!(dark, "dark applied now");
    wait_until(cx, "both saved", |_| {
        skyelab_core::AppSettings::load(&path).is_ok_and(|s| {
            s.appearance.theme == "grove" && s.appearance.mode == skyelab_core::ColorMode::Dark
        })
    });
}
