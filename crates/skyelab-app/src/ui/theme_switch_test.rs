//! Own UI kit, stage 2a (.agent/plans/2026-11-27-own-ui-kit-theme.md), criterion 3: a view painting from our theme
//! draws again with the new colors when the appearance changes.

use super::theme::ActiveTheme as _;
use super::themes;
use gpui::{
    Context, Hsla, IntoElement, Render, TestAppContext, VisualTestContext, Window, div, prelude::*,
};
use skyelab_core::{AppearanceSettings, ColorMode};
use std::cell::Cell;
use std::rc::Rc;

struct Painted(Rc<Cell<Option<Hsla>>>);

impl Render for Painted {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let background = cx.theme().background;
        self.0.set(Some(background));
        div().size_full().bg(background)
    }
}

fn pick(theme: &str, mode: ColorMode) -> AppearanceSettings {
    AppearanceSettings {
        mode,
        theme: theme.into(),
        ..Default::default()
    }
}

#[gpui::test]
fn a_view_draws_again_in_the_new_theme(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        super::install_theme(cx).expect("every theme file loads");
        themes::set(pick("skye", ColorMode::Light), cx).detach();
    });
    let seen = Rc::new(Cell::new(None));
    let s = seen.clone();
    let window = cx.add_window(move |window, cx| {
        let view = cx.new(|_| Painted(s));
        super::window_root(view.into(), window, cx)
    });
    let vcx = &mut VisualTestContext::from_window(window.into(), cx);
    vcx.run_until_parked();
    let light = vcx.update(|_, cx| super::theme::Theme::global(cx).background);
    assert_eq!(seen.get(), Some(light), "drawn in Skye Light first");

    vcx.update(|_, cx| themes::set(pick("dawn", ColorMode::Dark), cx).detach());
    vcx.run_until_parked();

    let dawn_dark = vcx.update(|_, cx| {
        let t = super::theme::Theme::global(cx);
        assert!(t.is_dark(), "Dawn Dark is dark");
        t.background
    });
    assert_ne!(dawn_dark, light, "the family changed the canvas");
    assert_eq!(
        seen.get(),
        Some(dawn_dark),
        "the view drew again in Dawn Dark"
    );
}

/// Mode System: when the window's appearance turns dark, ours turns dark with gpui-component's, on the same canvas.
#[gpui::test]
fn system_mode_follows_the_window_appearance(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        super::install_theme(cx).expect("every theme file loads");
        themes::set(pick("skye", ColorMode::System), cx).detach();
    });
    let seen = Rc::new(Cell::new(None));
    let s = seen.clone();
    let window = cx.add_window(move |window, cx| {
        let view = cx.new(|_| Painted(s));
        super::window_root(view.into(), window, cx)
    });
    let vcx = &mut VisualTestContext::from_window(window.into(), cx);
    for (appearance, dark) in [
        (gpui::WindowAppearance::Dark, true),
        (gpui::WindowAppearance::Light, false),
    ] {
        vcx.update(|window, cx| themes::follow(appearance, window, cx));
        vcx.run_until_parked();
        vcx.update(|_, cx| {
            let ours = super::theme::Theme::global(cx);
            let theirs = gpui_component::Theme::global(cx);
            assert_eq!(ours.is_dark(), dark, "ours follows {appearance:?}");
            assert_eq!(
                theirs.is_dark(),
                dark,
                "gpui-component follows {appearance:?}"
            );
            assert_eq!(
                ours.background, theirs.background,
                "{appearance:?}: one canvas"
            );
        });
        let ours = vcx.update(|_, cx| super::theme::Theme::global(cx).background);
        assert_eq!(
            seen.get(),
            Some(ours),
            "the view drew again for {appearance:?}"
        );
    }
}
