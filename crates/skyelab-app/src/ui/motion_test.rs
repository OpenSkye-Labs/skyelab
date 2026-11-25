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

/// The one-shot spring follows Rex's sidebar spring (38 / 75 / 92 / 98 % at 33 / 67 / 100 / 133 ms), never overshoots
/// and ends at 1; the shared ease-out quad starts fast and ends at 1.
#[test]
fn shared_curves_match_rex() {
    let at = |ms: f32| motion::spring_ease(ms / motion::SETTLE.as_millis() as f32);
    for (ms, want) in [(33., 0.38), (67., 0.75), (100., 0.92), (133., 0.98)] {
        assert!((at(ms) - want).abs() < 0.03, "{ms} ms: {}", at(ms));
    }
    assert_eq!(motion::spring_ease(0.), 0.);
    assert!((motion::spring_ease(1.) - 1.).abs() < 1e-6);
    let mut last = 0.;
    for i in 0..=100 {
        let v = motion::spring_ease(i as f32 / 100.);
        assert!(v >= last && v <= 1. + 1e-6, "monotone, no overshoot");
        last = v;
    }
    assert_eq!(motion::ease_out_quad(0.), 0.);
    assert_eq!(motion::ease_out_quad(1.), 1.);
    assert!(motion::ease_out_quad(0.5) > 0.7);
    assert!(motion::linux_animations_disabled("false\n"));
    assert!(!motion::linux_animations_disabled("true\n"));
}

struct SwitchRig {
    on: bool,
    loading: bool,
}

impl gpui::Render for SwitchRig {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        use gpui::ParentElement as _;
        gpui::div()
            .child(
                super::Switch::new("motion-switch")
                    .accessibility_label("Motion switch")
                    .checked(self.on),
            )
            .child(
                super::Button::new("motion-loading")
                    .label("Working")
                    .loading(self.loading),
            )
    }
}

/// Flipping a `Switch` rides `SPRING_PX` and then idles at 0 frames; under `reduce_motion` it snaps at once, and a
/// loading `Button` stops spinning.
#[gpui::test]
fn switch_springs_and_respects_reduce_motion(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        super::install_theme(cx).expect("every theme file parses");
        cx.set_reduce_motion(false);
    });
    let (view, cx) = cx.add_window_view(|_, _| SwitchRig {
        on: false,
        loading: false,
    });
    cx.run_until_parked();
    assert_eq!(
        moving_for(cx),
        std::time::Duration::ZERO,
        "initial settled switch asks for no frames"
    );

    view.update(cx, |v, cx| {
        v.on = true;
        cx.notify();
    });
    cx.run_until_parked();
    let travel = moving_for(cx);
    assert!(
        travel > std::time::Duration::ZERO && travel <= std::time::Duration::from_millis(350),
        "switch thumb springs to rest, ran {travel:?}"
    );
    assert_eq!(
        moving_for(cx),
        std::time::Duration::ZERO,
        "settled switch idles"
    );

    // Settings -> Appearance -> Motion: Reduced flips GPUI's reduce_motion on immediately.
    cx.update(|_, cx| {
        let s = skyelab_core::AppearanceSettings {
            motion: skyelab_core::MotionMode::Reduced,
            ..Default::default()
        };
        super::themes::set(s, cx).detach();
        assert!(
            cx.reduce_motion(),
            "Reduced mode enables GPUI reduce_motion"
        );
    });
    view.update(cx, |v, cx| {
        v.on = false;
        v.loading = true;
        cx.notify();
    });
    cx.run_until_parked();
    assert!(
        moving_for(cx) <= std::time::Duration::from_millis(10),
        "reduce_motion snaps switch and stops loading spinner"
    );
}
