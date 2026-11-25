//! Rex's motion reaches gpui-component's theme and stays through a theme or appearance change.

use super::motion;
use gpui::TestAppContext;
use gpui_component::{ActiveTheme as _, Theme, ThemeMode};

#[gpui::test]
fn theme_carries_rex_motion(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        super::install_theme(cx).expect("every theme file parses");
    });
    let check = |cx: &mut TestAppContext, when: &str| {
        cx.update(|cx| {
            let m = &cx.theme().motion;
            assert_eq!(m.duration_normal, motion::FADE, "{when}");
            assert_eq!(m.duration_fast, motion::FADE_FAST, "{when}");
            assert_eq!(
                format!("{:?}", m.spring_control),
                format!("{:?}", motion::SPRING),
                "{when}"
            );
        })
    };
    check(cx, "after install");
    cx.update(|cx| Theme::change(ThemeMode::Dark, None, cx));
    check(cx, "after a mode change");
    cx.update(|cx| {
        let s = skyelab_core::AppearanceSettings {
            theme: "dawn".into(),
            ..Default::default()
        };
        super::themes::set(s, cx).detach();
    });
    check(cx, "after a family change");
}
