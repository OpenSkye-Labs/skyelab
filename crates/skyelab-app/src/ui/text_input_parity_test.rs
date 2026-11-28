//! Own UI kit, stage 6a (.agent/plans/2026-11-29-own-ui-kit-text-editing-6a.md), criterion 11: our `TextField` lays out
//! its box, its text, a caret position and a selection exactly where today's did (gpui-component's `Input` in the
//! same chrome, copied here from `text_field.rs` before the move), at the app's 14 px rem.

use super::{ActiveTheme as _, Icon, SkyeIcon, Size, TextField, TextInput, h_flex};
use gpui::{
    AnyElement, AppContext as _, Bounds, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, Render, Styled as _, TestAppContext, Window, div, prelude::*, px,
};
use gpui_component::input::{Input, InputState};

const SIZES: [Size; 3] = [Size::Xs, Size::Sm, Size::Md];

fn sel(name: String) -> &'static str {
    Box::leak(name.into_boxed_str())
}

/// Today's `ui::TextField` (0c4278749), drawn over gpui-component's state.
fn theirs(state: &Entity<InputState>, size: Size, prefix: bool, cx: &gpui::App) -> AnyElement {
    let theme = cx.theme();
    let input = Input::new(state)
        .appearance(false)
        .when(prefix, |i| i.prefix(Icon::new(SkyeIcon::Search).size_3p5()))
        .px_0()
        .py_0()
        .h_full()
        .text_size(size.text());
    h_flex()
        .w_full()
        .min_w_0()
        .h(size.height())
        .px(size.pad_x() - px(1.))
        .rounded(size.radius())
        .border_1()
        .border_color(theme.input)
        .bg(if theme.is_dark() {
            theme.input.opacity(0.32)
        } else {
            theme.background
        })
        .text_size(size.text())
        .child(input)
        .into_any_element()
}

struct Pairs {
    theirs: Vec<Entity<InputState>>,
    ours: Vec<Entity<TextInput>>,
}

fn cases() -> impl Iterator<Item = (usize, Size, bool)> {
    SIZES
        .into_iter()
        .flat_map(|s| [(s, false), (s, true)])
        .enumerate()
        .map(|(n, (s, p))| (n, s, p))
}

impl Render for Pairs {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.set_rem_size(px(14.));
        let mut col = div().flex().flex_col().w(px(240.));
        for (n, size, prefix) in cases().take(self.ours.len()) {
            let ours = TextField::new(&self.ours[n])
                .with_size(size)
                .accessibility_label("Ours")
                .when(prefix, |f| f.prefix(Icon::new(SkyeIcon::Search).size_3p5()));
            if let Some(state) = self.theirs.get(n) {
                col = col.child(
                    div()
                        .debug_selector(move || sel(format!("theirs-{n}")).into())
                        .child(theirs(state, size, prefix, cx)),
                );
            }
            col = col.child(
                    div()
                        .debug_selector(move || sel(format!("ours-{n}")).into())
                    .child(ours),
            );
        }
        col
    }
}

/// `b` relative to `origin`, so the two columns (one under the other) compare.
fn rel(b: Bounds<Pixels>, origin: gpui::Point<Pixels>) -> Bounds<Pixels> {
    Bounds::new(b.origin - origin, b.size)
}

#[gpui::test]
fn our_field_lays_out_like_theirs(cx: &mut TestAppContext) {
    super::button_test::setup(cx);
    let (view, vcx) = cx.add_window_view(|window, cx| {
        let mut theirs = Vec::new();
        let mut ours = Vec::new();
        for _ in cases() {
            theirs.push(cx.new(|cx| InputState::new(window, cx).default_value("abc")));
            let o = cx.new(|cx| TextInput::new(window, cx));
            o.update(cx, |i, cx| i.set_value("abc", window, cx));
            ours.push(o);
        }
        Pairs { theirs, ours }
    });
    vcx.run_until_parked();
    let mut bad = Vec::new();
    for (n, size, prefix) in cases() {
        let what = format!("{size:?} prefix {prefix}");
        let a = vcx.debug_bounds(sel(format!("theirs-{n}"))).expect("theirs drawn");
        let b = vcx.debug_bounds(sel(format!("ours-{n}"))).expect("ours drawn");
        if a.size != b.size {
            bad.push(format!("{what} box: theirs {:?}, ours {:?}", a.size, b.size));
        }
        let (t, o) = view.read_with(vcx, |p, _| (p.theirs[n].clone(), p.ours[n].clone()));
        let theirs = t.read_with(vcx, |s, _| {
            (
                s.text_bounds(),
                s.range_to_bounds(&(3..3)),
                s.range_to_bounds(&(1..3)),
            )
        });
        let ours = o.read_with(vcx, |s, _| {
            (s.text_bounds(), s.range_bounds(3..3), s.range_bounds(1..3))
        });
        for (part, x, y) in [
            ("text", theirs.0, ours.0),
            ("caret at 3", theirs.1, ours.1),
            ("selection 1..3", theirs.2, ours.2),
        ] {
            let x = x.map(|x| rel(x, a.origin));
            let y = y.map(|y| rel(y, b.origin));
            if x != y {
                bad.push(format!("{what} {part}: theirs {x:?}, ours {y:?}"));
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[gpui::test]
fn our_caret_is_one_pixel_wide_at_the_caret_position(cx: &mut TestAppContext) {
    super::button_test::setup(cx);
    let (view, vcx) = cx.add_window_view(|window, cx| {
        let o = cx.new(|cx| TextInput::new(window, cx));
        o.update(cx, |i, cx| i.set_value("abc", window, cx));
        Pairs {
            theirs: Vec::new(),
            ours: vec![o],
        }
    });
    let o = view.read_with(vcx, |p, _| p.ours[0].clone());
    vcx.update(|window, cx| {
        let f = gpui::Focusable::focus_handle(o.read(cx), cx);
        window.focus(&f, cx);
    });
    vcx.run_until_parked();
    let (caret, at) = o.read_with(vcx, |s, _| (s.caret_bounds(), s.range_bounds(3..3)));
    let (caret, at) = (caret.expect("a caret"), at.expect("laid out"));
    assert_eq!(caret.size.width, px(1.), "theirs is CURSOR_WIDTH, 1 px");
    assert_eq!(caret.origin.x, at.origin.x);
}
