use crate::ui::{self, Button, Popover};
use gpui::{
    AppContext as _, Context, IntoElement, Modifiers, ParentElement as _, Pixels, Render,
    Styled as _, TestAppContext, VisualTestContext, Window, div, px,
};
use gpui_base::test_support::{ElementSnapshot, snapshots};

struct Harness {
    shift: Pixels,
}

impl Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .p_4()
            .flex()
            .flex_col()
            .child(div().h(self.shift))
            .child(
                Popover::new("pop")
                    .trigger(|open| Button::new("pop-trigger").label("Open").selected(open))
                    .content(|_, _| Button::new("inside").label("Inside").on_click(|_, _, _| {})),
            )
    }
}

fn node(vcx: &mut VisualTestContext, label: &str) -> ElementSnapshot {
    vcx.update(|window, _| snapshots(window))
        .into_iter()
        .find(|s| s.label() == Some(label) && s.visible())
        .unwrap_or_else(|| panic!("{label} drawn"))
}

fn gap(vcx: &mut VisualTestContext) -> Pixels {
    node(vcx, "Inside").bounds().top() - node(vcx, "Open").bounds().bottom()
}

#[gpui::test]
fn an_open_popover_follows_its_trigger_when_it_moves(cx: &mut TestAppContext) {
    ui::button_test::setup(cx);
    let view = cx.update(|cx| cx.new(|_| Harness { shift: px(0.) }));
    let root = view.clone();
    let (_, vcx) =
        cx.add_window_view(move |window, cx| ui::window_root(root.clone().into(), window, cx));
    vcx.run_until_parked();
    vcx.update(|window, cx| {
        window.draw(cx).clear(cx);
        window.draw(cx).clear(cx);
    });
    let at = node(vcx, "Open").bounds().center();
    vcx.simulate_mouse_move(at, None, Modifiers::none());
    vcx.simulate_click(at, Modifiers::none());
    vcx.run_until_parked();
    let before = gap(vcx);
    assert!(
        before >= px(0.) && before < px(40.),
        "opens under its trigger: {before:?}"
    );
    vcx.update(|_, cx| {
        view.update(cx, |h, cx| {
            h.shift = px(120.);
            cx.notify();
        })
    });
    vcx.run_until_parked();
    assert_eq!(
        gap(vcx),
        before,
        "the open popover stays under its trigger after the trigger moves"
    );
}
