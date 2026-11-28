use super::{AppAssets, Icon, SkyeIcon, h_flex, v_flex};
use gpui::{
    AnyElement, AssetSource as _, Context, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, Styled as _, TestAppContext, Window, div, px,
};
use gpui_component::Sizable as _;

const TEXT: [f32; 2] = [13., 16.];

fn theirs() -> gpui_component::Icon {
    gpui_component::Icon::new(SkyeIcon::Check)
}

fn ours() -> Icon {
    Icon::new(SkyeIcon::Check)
}

fn cases() -> Vec<(&'static str, AnyElement, AnyElement)> {
    vec![
        (
            "default",
            theirs().into_any_element(),
            ours().into_any_element(),
        ),
        (
            "size_3",
            theirs().size_3().into_any_element(),
            ours().size_3().into_any_element(),
        ),
        (
            "xsmall",
            theirs().xsmall().into_any_element(),
            ours().size_3().into_any_element(),
        ),
        (
            "small",
            theirs().small().into_any_element(),
            ours().size_3p5().into_any_element(),
        ),
        (
            "size_4",
            theirs().size_4().into_any_element(),
            ours().size_4().into_any_element(),
        ),
        (
            "px18",
            theirs().with_size(px(18.)).into_any_element(),
            ours().size(px(18.)).into_any_element(),
        ),
        (
            "width",
            theirs().w(px(10.)).into_any_element(),
            ours().w(px(10.)).into_any_element(),
        ),
        (
            "margin",
            theirs().size_4().mr_1().into_any_element(),
            ours().size_4().mr_1().into_any_element(),
        ),
    ]
}

fn selector(side: &str, text: usize, case: usize) -> &'static str {
    Box::leak(format!("{side}-{text}-{case}").into_boxed_str())
}

struct Pairs;

impl Render for Pairs {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let mut col = div().flex().flex_col().items_start();
        for (t, size) in TEXT.into_iter().enumerate() {
            for (c, (_, a, b)) in cases().into_iter().enumerate() {
                col = col.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .text_size(px(size))
                        .children([
                            div()
                                .debug_selector(move || selector("theirs", t, c).into())
                                .child(a),
                            div()
                                .debug_selector(move || selector("ours", t, c).into())
                                .child(b),
                        ]),
                );
            }
            for (side, icon) in [
                ("theirs-squeezed", theirs().into_any_element()),
                ("ours-squeezed", ours().into_any_element()),
            ] {
                col = col.child(
                    div()
                        .flex()
                        .flex_row()
                        .w(px(20.))
                        .text_size(px(size))
                        .child(icon)
                        .child(
                            div()
                                .w(px(100.))
                                .debug_selector(move || selector(side, t, 0).into()),
                        ),
                );
            }
        }
        col
    }
}

#[gpui::test]
fn our_icon_takes_the_same_bounds_as_gpui_components(cx: &mut TestAppContext) {
    super::button_test::setup(cx);
    let (_view, cx) = cx.add_window_view(|_, _| Pairs);
    cx.run_until_parked();
    let names: Vec<_> = cases().into_iter().map(|(n, ..)| n).collect();
    for (t, size) in TEXT.into_iter().enumerate() {
        for (c, name) in names.iter().enumerate() {
            let a = cx
                .debug_bounds(selector("theirs", t, c))
                .expect("theirs drawn");
            let b = cx.debug_bounds(selector("ours", t, c)).expect("ours drawn");
            assert_eq!(a.size, b.size, "{name} at {size} px text");
        }
        let a = cx
            .debug_bounds(selector("theirs-squeezed", t, 0))
            .expect("drawn");
        let b = cx
            .debug_bounds(selector("ours-squeezed", t, 0))
            .expect("drawn");
        assert_eq!(
            a.size, b.size,
            "an icon's neighbor in a crowded row at {size} px text"
        );
        assert_eq!(b.size.width, px(20. - size), "the icon doesn't shrink");
    }
}

#[test]
fn h_flex_and_v_flex_match_gpui_components() {
    let (mut ours, mut theirs) = (h_flex(), gpui_component::h_flex());
    assert!(ours.style() == theirs.style(), "h_flex");
    let (mut ours, mut theirs) = (v_flex(), gpui_component::v_flex());
    assert!(ours.style() == theirs.style(), "v_flex");
}

#[test]
fn caption_buttons_draw_our_icons() {
    for icon in [
        SkyeIcon::WindowMinimize,
        SkyeIcon::WindowMaximize,
        SkyeIcon::WindowRestore,
        SkyeIcon::WindowClose,
    ] {
        let path = icon.path();
        assert!(
            path.starts_with("icons/skyelab/"),
            "{icon:?} is ours: {path}"
        );
        let svg = AppAssets
            .load(&path)
            .expect("asset load")
            .expect("embedded");
        assert!(
            std::str::from_utf8(&svg)
                .expect("utf8")
                .contains("currentColor"),
            "{icon:?} is an SVG drawn in the text color"
        );
    }
    use super::window_controls::caption_icon;
    use gpui::WindowControlArea::{Close, Max, Min};
    assert_eq!(caption_icon(Min, false), SkyeIcon::WindowMinimize);
    assert_eq!(caption_icon(Max, false), SkyeIcon::WindowMaximize);
    assert_eq!(caption_icon(Max, true), SkyeIcon::WindowRestore);
    assert_eq!(caption_icon(Close, false), SkyeIcon::WindowClose);
}
