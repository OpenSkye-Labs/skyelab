//! Own UI kit, stage 4a (.agent/plans/2026-11-28-own-ui-kit-small-pieces.md), criteria 3 to 5: our spinner, copy
//! button and status bar take the same room as gpui-component's (drawn side by side), and behave as theirs did.

use super::themes::{self, FAMILIES};
use super::{AppAssets, CopyButton, SkyeIcon, Spinner, StatusBar};
use gpui::{
    AnyElement, AssetSource as _, ClipboardItem, Context, InteractiveElement as _, IntoElement,
    Modifiers, ParentElement as _, Render, Styled as _, TestAppContext, VisualTestContext, Window,
    div, px,
};
use gpui_component::Sizable as _;
use skyelab_core::{AppearanceSettings, ColorMode};
use std::time::Duration;

fn sel(name: String) -> &'static str {
    Box::leak(name.into_boxed_str())
}

/// Each case drawn twice, theirs then ours, each in a box named `theirs-<n>` / `ours-<n>`.
struct Pairs(fn() -> Vec<(&'static str, AnyElement, AnyElement)>);

impl Render for Pairs {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The app's rem, as gpui-component's Root sets it.
        window.set_rem_size(super::theme::Theme::global(cx).font_size);
        let mut col = div().flex().flex_col().items_start().w(px(600.));
        for (n, (_, theirs, ours)) in (self.0)().into_iter().enumerate() {
            col = col
                .child(
                    div()
                        .debug_selector(move || sel(format!("theirs-{n}")).into())
                        .child(theirs),
                )
                .child(
                    div()
                        .debug_selector(move || sel(format!("ours-{n}")).into())
                        .child(ours),
                );
        }
        col
    }
}

fn same_bounds(
    cx: &mut TestAppContext,
    cases: fn() -> Vec<(&'static str, AnyElement, AnyElement)>,
) {
    super::button_test::setup(cx);
    let (_, cx) = cx.add_window_view(|_, _| Pairs(cases));
    cx.run_until_parked();
    for (n, (name, ..)) in cases().into_iter().enumerate() {
        let a = cx
            .debug_bounds(sel(format!("theirs-{n}")))
            .expect("theirs drawn");
        let b = cx
            .debug_bounds(sel(format!("ours-{n}")))
            .expect("ours drawn");
        assert_eq!(a.size, b.size, "{name}");
    }
}

fn spinners() -> Vec<(&'static str, AnyElement, AnyElement)> {
    let mut cases: Vec<_> = [12., 14., 16.]
        .into_iter()
        .map(|s| {
            (
                "spinner",
                gpui_component::spinner::Spinner::new()
                    .with_size(px(s))
                    .into_any_element(),
                Spinner::new().size(px(s)).into_any_element(),
            )
        })
        .collect();
    cases.push((
        "xsmall, the devices panel's",
        gpui_component::spinner::Spinner::new()
            .xsmall()
            .into_any_element(),
        Spinner::new().size(gpui::rems(0.75)).into_any_element(),
    ));
    cases
}

#[gpui::test]
fn our_spinner_takes_the_same_room(cx: &mut TestAppContext) {
    same_bounds(cx, spinners);
}

#[test]
fn our_spinner_glyph_is_the_one_it_drew_before() {
    let ours = AppAssets
        .load(&SkyeIcon::Loader.path())
        .expect("load")
        .expect("embedded");
    let before = gpui_kit_assets::Assets
        .load("icons/loader.svg")
        .expect("load")
        .expect("in gpui-kit-assets");
    assert_eq!(ours, before);
    for (icon, theirs) in [
        (SkyeIcon::CopyButton, "icons/copy.svg"),
        (SkyeIcon::CopyButtonDone, "icons/check.svg"),
    ] {
        let ours = AppAssets
            .load(&icon.path())
            .expect("load")
            .expect("embedded");
        let before = gpui_kit_assets::Assets
            .load(theirs)
            .expect("load")
            .expect("there");
        assert_eq!(
            ours, before,
            "{icon:?}: the copy button's glyphs are the ones it drew before"
        );
    }
    assert_eq!(
        super::spinner::GLYPH,
        SkyeIcon::Loader,
        "the glyph Spinner's render draws"
    );
}

struct OneSpinner;
impl Render for OneSpinner {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().child(Spinner::new().size(px(14.)))
    }
}

/// The frames the spinner asks for once it has turned for 5 s: it never stops by itself.
fn spinner_frames(cx: &mut TestAppContext, reduce: bool) -> usize {
    super::button_test::setup(cx);
    cx.update(|cx| cx.set_reduce_motion(reduce));
    let (_, cx) = cx.add_window_view(|_, _| OneSpinner);
    cx.run_until_parked();
    for _ in 0..50 {
        cx.update(|window, cx| window.simulate_next_frame(cx));
        cx.executor().advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
    }
    cx.update(|window, cx| window.simulate_next_frame(cx))
}

#[gpui::test]
fn our_spinner_turns(cx: &mut TestAppContext) {
    assert!(spinner_frames(cx, false) > 0);
}

#[gpui::test]
fn our_spinner_holds_still_under_reduce_motion(cx: &mut TestAppContext) {
    assert_eq!(spinner_frames(cx, true), 0);
}

/// The row under a reply (`timeline_menu.rs`): the copy button, faded, then the time.
fn footer(copy: AnyElement) -> AnyElement {
    super::h_flex()
        .gap_1()
        .text_size(super::META)
        .child(div().opacity(0.6).child(copy))
        .child("00:11")
        .into_any_element()
}

fn copy_buttons() -> Vec<(&'static str, AnyElement, AnyElement)> {
    vec![
        (
            "icon only",
            gpui_component::clipboard::Clipboard::new("t-0")
                .value("x")
                .tooltip("Copy")
                .into_any_element(),
            CopyButton::new("o-0")
                .value("x")
                .tooltip("Copy")
                .into_any_element(),
        ),
        (
            "in a reply's footer row",
            footer(
                gpui_component::clipboard::Clipboard::new("t-1")
                    .value("x")
                    .into_any_element(),
            ),
            footer(CopyButton::new("o-1").value("x").into_any_element()),
        ),
    ]
}

#[gpui::test]
fn our_copy_button_takes_the_same_room(cx: &mut TestAppContext) {
    same_bounds(cx, copy_buttons);
}

struct OneCopy;
impl Render for OneCopy {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // A row, so the marked box shrinks to the button and a click at its center lands on it.
        div().flex().items_start().child(
            div()
                .debug_selector(|| "copy".into())
                .child(CopyButton::new("copy-code").value("cargo run")),
        )
    }
}

fn press(cx: &mut VisualTestContext) {
    let at = cx.debug_bounds("copy").expect("drawn").center();
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
}

fn clipboard(cx: &mut VisualTestContext) -> Option<String> {
    cx.read_from_clipboard().and_then(|c| c.text())
}

/// The copy button's description: "Copied" while it shows the check (the same flag picks the icon), else none.
fn description(cx: &mut VisualTestContext) -> Option<String> {
    let snaps = cx.update(|window, _| gpui_base::test_support::snapshots(window));
    let node = snaps
        .iter()
        .find(|s| s.label() == Some("Copy"))
        .expect("a node named Copy");
    node.description().map(str::to_owned)
}

#[gpui::test]
fn a_click_copies_once_until_it_resets(cx: &mut TestAppContext) {
    super::button_test::setup(cx);
    let (_, cx) = cx.add_window_view(|_, _| OneCopy);
    cx.run_until_parked();
    assert_eq!(description(cx), None);
    press(cx);
    assert_eq!(clipboard(cx).as_deref(), Some("cargo run"));
    assert_eq!(
        description(cx).as_deref(),
        Some("Copied"),
        "the check shows"
    );

    cx.write_to_clipboard(ClipboardItem::new_string("other".into()));
    press(cx);
    assert_eq!(
        clipboard(cx).as_deref(),
        Some("other"),
        "while it shows the check, a click writes nothing"
    );

    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
    assert_eq!(description(cx), None, "the copy icon is back after 2 s");
    press(cx);
    assert_eq!(
        clipboard(cx).as_deref(),
        Some("cargo run"),
        "and copies again"
    );
}

#[gpui::test]
fn it_is_a_button_named_copy_unless_named(cx: &mut TestAppContext) {
    struct Two;
    impl Render for Two {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().child(CopyButton::new("a").value("x")).child(
                CopyButton::new("b")
                    .value("y")
                    .accessibility_label("Copy reply: hi"),
            )
        }
    }
    super::button_test::setup(cx);
    let (_, cx) = cx.add_window_view(|_, _| Two);
    cx.run_until_parked();
    let snaps = cx.update(|window, _| gpui_base::test_support::snapshots(window));
    for name in ["Copy", "Copy reply: hi"] {
        let s = snaps
            .iter()
            .find(|s| s.label() == Some(name))
            .unwrap_or_else(|| panic!("no node named {name}"));
        assert_eq!(s.role(), Some(gpui::accesskit::Role::Button), "{name}");
    }
}

fn block(w: f32) -> gpui::Div {
    div().w(px(w)).h(px(10.))
}

fn status_bars() -> Vec<(&'static str, AnyElement, AnyElement)> {
    use gpui_component::status_bar::StatusBar as Theirs;
    vec![
        (
            "left, center, right",
            Theirs::new()
                .left(block(40.))
                .right(block(30.))
                .right(block(20.))
                .child(block(50.))
                .into_any_element(),
            StatusBar::new()
                .left(block(40.))
                .right(block(30.))
                .right(block(20.))
                .child(block(50.))
                .into_any_element(),
        ),
        (
            "left and center",
            Theirs::new()
                .left(block(40.))
                .child(block(50.))
                .into_any_element(),
            StatusBar::new()
                .left(block(40.))
                .child(block(50.))
                .into_any_element(),
        ),
        (
            "center only, styled",
            Theirs::new().w_full().child(block(50.)).into_any_element(),
            StatusBar::new()
                .w_full()
                .child(block(50.))
                .into_any_element(),
        ),
    ]
}

#[gpui::test]
fn our_status_bar_takes_the_same_room(cx: &mut TestAppContext) {
    same_bounds(cx, status_bars);
}

/// Where each child lands, in each layout: a marked block per region, at a fixed bar width.
#[gpui::test]
fn our_status_bar_places_each_region_alike(cx: &mut TestAppContext) {
    fn mark(side: &str, case: usize, what: &str, w: f32) -> gpui::Div {
        let name = sel(format!("{side}-{case}-{what}"));
        block(w).debug_selector(move || name.into())
    }
    /// Layout `case`: 0 left + center + right, 1 left + center, 2 center only.
    fn theirs(case: usize) -> AnyElement {
        let bar = gpui_component::status_bar::StatusBar::new().w(px(500.));
        let bar = if case < 2 {
            bar.left(mark("theirs", case, "left", 40.))
        } else {
            bar
        };
        let bar = if case == 0 {
            bar.right(mark("theirs", case, "right", 30.))
        } else {
            bar
        };
        bar.child(mark("theirs", case, "center", 50.))
            .into_any_element()
    }
    fn ours(case: usize) -> AnyElement {
        let bar = StatusBar::new().w(px(500.));
        let bar = if case < 2 {
            bar.left(mark("ours", case, "left", 40.))
        } else {
            bar
        };
        let bar = if case == 0 {
            bar.right(mark("ours", case, "right", 30.))
        } else {
            bar
        };
        bar.child(mark("ours", case, "center", 50.))
            .into_any_element()
    }
    fn bars() -> Vec<(&'static str, AnyElement, AnyElement)> {
        (0..3).map(|c| ("marked", theirs(c), ours(c))).collect()
    }
    super::button_test::setup(cx);
    let (_, cx) = cx.add_window_view(|_, _| Pairs(bars));
    cx.run_until_parked();
    for (case, regions) in [
        (0, &["left", "center", "right"][..]),
        (1, &["left", "center"][..]),
        (2, &["center"][..]),
    ] {
        let t0 = cx
            .debug_bounds(sel(format!("theirs-{case}")))
            .expect("drawn")
            .origin;
        let o0 = cx
            .debug_bounds(sel(format!("ours-{case}")))
            .expect("drawn")
            .origin;
        for what in regions {
            let t = cx
                .debug_bounds(sel(format!("theirs-{case}-{what}")))
                .expect("drawn");
            let o = cx
                .debug_bounds(sel(format!("ours-{case}-{what}")))
                .expect("drawn");
            assert_eq!(t.origin - t0, o.origin - o0, "layout {case}, {what}");
        }
    }
}

#[gpui::test]
fn our_status_bar_colors_come_from_the_same_keys(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        super::install_theme(cx).expect("every theme file loads");
    });
    let mut bad = Vec::new();
    for f in &FAMILIES {
        for mode in [ColorMode::Light, ColorMode::Dark] {
            cx.update(|cx| {
                let s = AppearanceSettings {
                    mode,
                    theme: f.key.into(),
                    ..Default::default()
                };
                themes::set(s.clone(), cx).detach();
                let dark = mode == ColorMode::Dark;
                let ours = super::theme::Theme::global(cx);
                let config: gpui_component::ThemeConfig =
                    serde_json::from_value(themes::adjusted(f.key, &s, dark))
                        .expect("gpui-component reads the adjusted theme");
                let mut theirs = gpui_component::Theme::global(cx).clone();
                theirs.apply_config(&std::rc::Rc::new(config));
                if ours.status_bar != *theirs.tokens.status_bar
                    || ours.status_bar_border != theirs.status_bar_border
                {
                    bad.push(format!("{} {mode:?}", f.key));
                }
            });
        }
    }
    assert!(bad.is_empty(), "status bar colors differ: {bad:?}");
}
