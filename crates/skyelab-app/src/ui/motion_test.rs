//! Rex's motion reaches gpui-component's theme and stays through a theme or appearance change.

use super::motion;
use gpui::{AppContext as _, TestAppContext};
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

struct Blank;
impl gpui::Render for Blank {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        gpui::Empty
    }
}

/// How long the window keeps asking for frames, stepping 10 ms a frame.
fn moving_for(cx: &mut gpui::VisualTestContext) -> std::time::Duration {
    let step = std::time::Duration::from_millis(10);
    let mut t = std::time::Duration::ZERO;
    for _ in 0..100 {
        let n = cx.update(|window, cx| window.simulate_next_frame(cx));
        cx.run_until_parked();
        if n == 0 {
            return t;
        }
        cx.executor().advance_clock(step);
        t += step;
    }
    panic!("never settled");
}

/// A dialog fades over Rex's 150 ms (gpui-component's default slide runs 250 ms); one that sets
/// `DialogEntrance::None` (the palette) draws no frames.
#[gpui::test]
fn dialogs_fade_for_rex_duration_and_none_draws_at_once(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        super::install_theme(cx).expect("every theme file parses");
        cx.set_reduce_motion(false);
    });
    let window = cx.add_window(|window, cx| {
        let view = cx.new(|_| Blank);
        gpui_component::Root::new(view, window, cx)
    });
    let cx = gpui::VisualTestContext::from_window(window.into(), cx).into_mut();
    moving_for(cx);
    cx.update(|window, cx| super::open_dialog(window, cx, |d, _, _| d.title("Fade")));
    cx.run_until_parked();
    let fade = moving_for(cx);
    assert!(
        fade >= std::time::Duration::from_millis(100)
            && fade <= motion::FADE + std::time::Duration::from_millis(30),
        "the fade runs about 150 ms, ran {fade:?}"
    );
    cx.update(gpui_component::WindowExt::close_dialog);
    cx.run_until_parked();
    moving_for(cx);
    cx.update(|window, cx| {
        super::open_dialog(window, cx, |d, _, _| {
            d.entrance(gpui_component::dialog::DialogEntrance::None)
        })
    });
    cx.run_until_parked();
    assert_eq!(
        moving_for(cx),
        std::time::Duration::ZERO,
        "no entrance, no frames"
    );
}
