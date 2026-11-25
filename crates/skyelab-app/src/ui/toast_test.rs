//! SkyeLab's toasts: Rex's 150 ms fade then no frames, autohide, actions that stay, keys that replace, hover holds.

use super::motion;
use super::toast::{self, AUTOHIDE, Toast};
use gpui::{
    Context, IntoElement, ParentElement as _, Render, Styled as _, TestAppContext,
    VisualTestContext, Window, div,
};
use std::time::Duration;

struct Host;
impl Render for Host {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().children(toast::layer(window, cx))
    }
}

fn host(cx: &mut TestAppContext) -> &mut VisualTestContext {
    cx.update(|cx| {
        gpui_component::init(cx);
        super::install_theme(cx).expect("every theme file parses");
        cx.set_reduce_motion(false);
    });
    let (_, cx) = cx.add_window_view(|_, _| Host);
    cx
}

fn push(cx: &mut VisualTestContext, t: Toast) {
    cx.update(|window, cx| toast::push(window, t, cx));
    cx.run_until_parked();
}

fn shown(cx: &mut VisualTestContext) -> Vec<String> {
    cx.update(|window, cx| {
        toast::shown(window, cx)
            .iter()
            .map(|s| s.to_string())
            .collect()
    })
}

/// How long the window keeps asking for frames, stepping 10 ms a frame.
fn moving_for(cx: &mut VisualTestContext) -> Duration {
    let step = Duration::from_millis(10);
    let mut t = Duration::ZERO;
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

#[gpui::test]
fn fades_in_over_rex_150ms_then_idles(cx: &mut TestAppContext) {
    let cx = host(cx);
    assert_eq!(moving_for(cx), Duration::ZERO, "no toast, no frames");
    push(cx, Toast::info("hello"));
    let t = moving_for(cx);
    // gpui-component's slide ran 400 ms; Rex's fade is 150.
    assert!(
        t >= motion::FADE - Duration::from_millis(20)
            && t <= motion::FADE + Duration::from_millis(30),
        "fade ran {t:?}"
    );
    assert_eq!(
        moving_for(cx),
        Duration::ZERO,
        "a shown toast draws no frames"
    );
}

#[gpui::test]
fn reduce_motion_shows_it_at_once(cx: &mut TestAppContext) {
    let cx = host(cx);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    moving_for(cx);
    push(cx, Toast::info("hello"));
    assert!(moving_for(cx) <= Duration::from_millis(10));
}

#[gpui::test]
fn autohides_unless_it_has_an_action(cx: &mut TestAppContext) {
    let cx = host(cx);
    push(cx, Toast::new("plain"));
    push(cx, Toast::new("restart").action("Restart", |_, _| {}));
    push(
        cx,
        Toast::new("undo").action("Undo", |_, _| {}).autohide(true),
    );
    assert_eq!(shown(cx), ["plain", "restart", "undo"]);
    cx.executor()
        .advance_clock(AUTOHIDE - Duration::from_millis(100));
    cx.run_until_parked();
    assert_eq!(shown(cx).len(), 3, "still up just before the timeout");
    cx.executor().advance_clock(Duration::from_millis(200));
    cx.run_until_parked();
    assert_eq!(shown(cx), ["restart"]);
}

#[gpui::test]
fn a_key_replaces_in_place(cx: &mut TestAppContext) {
    let cx = host(cx);
    push(cx, Toast::new("one").key("thread-1"));
    push(cx, Toast::new("other"));
    push(cx, Toast::new("two").key("thread-1"));
    assert_eq!(shown(cx), ["other", "two"]);
}

#[gpui::test]
fn the_pointer_on_it_holds_it(cx: &mut TestAppContext) {
    let cx = host(cx);
    push(cx, Toast::new("held"));
    let bounds = cx.debug_bounds("toast-1").expect("toast drawn");
    cx.simulate_mouse_move(bounds.center(), None, gpui::Modifiers::none());
    cx.executor().advance_clock(AUTOHIDE * 2);
    cx.run_until_parked();
    assert_eq!(shown(cx), ["held"], "kept while hovered");
    cx.simulate_mouse_move(
        gpui::point(gpui::px(1.), gpui::px(1.)),
        None,
        gpui::Modifiers::none(),
    );
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
    assert!(shown(cx).is_empty(), "goes once the pointer leaves");
}
