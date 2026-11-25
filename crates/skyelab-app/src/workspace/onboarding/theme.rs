//! Onboarding's Theme page, after Rex's appearance step: mode and family as picker tiles (each painted in the colors
//! it stands for; the picked one gets a ring and a bold label) and a live miniature of the app.

use super::Onboarding;
use super::pages::caption;
use crate::ui;
use crate::ui::Divider;
use gpui::{AnyElement, Context, Div, FontWeight, Hsla, ParentElement as _, div, prelude::*, px};
use gpui_component::{ActiveTheme as _, h_flex, v_flex};
use skyelab_core::ColorMode;

/// A picker tile's preview.
const TILE_W: f32 = 92.;
const TILE_H: f32 = 64.;

impl Onboarding {
    pub(super) fn theme_page(&self, cx: &mut Context<Self>) -> Div {
        let current = ui::themes::get(cx);
        let dark = cx.theme().mode.is_dark();
        let mode = match current.mode {
            ColorMode::System => 0,
            ColorMode::Light => 1,
            ColorMode::Dark => 2,
        };
        let colors = |key: &str, dark: bool, cx: &Context<Self>| {
            ui::themes::swatch(key, dark, cx).unwrap_or_else(|| {
                let t = cx.theme();
                [t.background, t.foreground, t.primary]
            })
        };
        let (light_c, dark_c) = (
            colors(&current.theme, false, cx),
            colors(&current.theme, true, cx),
        );
        let modes = [
            ("System", mini_split(light_c, dark_c)),
            ("Light", mini(light_c).into_any_element()),
            ("Dark", mini(dark_c).into_any_element()),
        ];
        let mut mode_row = h_flex().gap_3();
        for (ix, (label, art)) in modes.into_iter().enumerate() {
            mode_row = mode_row.child(
                tile(("onboarding-mode-tile", ix), label, ix == mode, art, cx)
                    .on_click(cx.listener(move |this, _, _, cx| this.pick_mode(ix, cx))),
            );
        }
        let mut families = h_flex().flex_wrap().gap_3();
        let mut about = "";
        for (i, f) in ui::themes::FAMILIES.iter().enumerate() {
            let on = current.theme == f.key;
            if on {
                about = f.about;
            }
            let key = f.key;
            families = families.child(
                tile(
                    ("onboarding-family", i),
                    f.label,
                    on,
                    mini(colors(key, dark, cx)).into_any_element(),
                    cx,
                )
                .debug_selector(move || format!("onboarding-family-{key}"))
                .on_click(cx.listener(move |this, _, _, cx| this.pick_family(key, cx))),
            );
        }
        let theme = cx.theme();
        v_flex()
            .gap_3()
            .child(
                h_flex()
                    .items_end()
                    .justify_between()
                    .gap_4()
                    .child(
                        v_flex().gap_2().child(caption("Appearance", cx)).child(
                            div()
                                .debug_selector(|| "onboarding-mode".into())
                                .child(mode_row),
                        ),
                    )
                    .child(preview(cx)),
            )
            .child(Divider::horizontal())
            .child(caption("Theme", cx))
            .child(families)
            .child(
                div()
                    .text_size(ui::META)
                    .text_color(theme.muted_foreground)
                    .child(about),
            )
    }
}

/// A picker tile: a preview with its label under it; the picked one gets a ring and a bold label.
fn tile(
    id: impl Into<gpui::ElementId>,
    label: &'static str,
    on: bool,
    art: AnyElement,
    cx: &gpui::App,
) -> gpui::Stateful<Div> {
    let theme = cx.theme();
    let ring = theme.ring;
    v_flex()
        .id(id)
        .items_center()
        .gap_1p5()
        .cursor_pointer()
        .child(
            div()
                .p(px(2.))
                .rounded(ui::RADIUS_LG + px(3.))
                .border_2()
                .border_color(if on { ring } else { gpui::transparent_black() })
                .child(
                    div()
                        .w(px(TILE_W))
                        .h(px(TILE_H))
                        .rounded(ui::RADIUS_LG)
                        .border_1()
                        .border_color(theme.border)
                        .overflow_hidden()
                        .child(art),
                ),
        )
        .child(
            div()
                .text_size(ui::META)
                .font_weight(if on {
                    FontWeight::BOLD
                } else {
                    FontWeight::NORMAL
                })
                .text_color(if on {
                    theme.foreground
                } else {
                    theme.muted_foreground
                })
                .child(label),
        )
}

/// A tiny window in a theme's canvas, text and accent colors.
fn mini([bg, fg, accent]: [Hsla; 3]) -> Div {
    let bar = |w: f32, c: Hsla| div().h(px(5.)).w(px(w)).rounded_full().bg(c);
    v_flex()
        .size_full()
        .bg(bg)
        .p_2()
        .gap_1p5()
        .child(
            h_flex()
                .gap_1()
                .children((0..3).map(|_| div().size(px(4.)).rounded_full().bg(fg.opacity(0.3)))),
        )
        .child(bar(44., accent))
        .child(bar(60., fg.opacity(0.25)))
        .child(bar(36., fg.opacity(0.25)))
}

/// System's tile: light on the left half, dark on the right.
fn mini_split(light: [Hsla; 3], dark: [Hsla; 3]) -> AnyElement {
    h_flex()
        .size_full()
        .child(div().w_1_2().h_full().overflow_hidden().child(mini(light)))
        .child(div().w_1_2().h_full().overflow_hidden().child(mini(dark)))
        .into_any_element()
}

/// A miniature of the app in the theme now applied: sidebar with a selected row, a thread, the composer.
fn preview(cx: &Context<Onboarding>) -> impl IntoElement {
    let theme = cx.theme();
    let bar = |w: f32, color: gpui::Hsla| div().h(px(6.)).w(px(w)).rounded_full().bg(color);
    let line = theme.muted_foreground.opacity(0.5);
    v_flex()
        .debug_selector(|| "onboarding-preview".into())
        .flex_shrink_0()
        .w(px(200.))
        .h(px(TILE_H + 30.))
        .rounded(ui::RADIUS_LG)
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .overflow_hidden()
        .child(
            h_flex()
                .size_full()
                .child(
                    v_flex()
                        .w(px(72.))
                        .h_full()
                        .p_2()
                        .gap_1p5()
                        .bg(theme.sidebar)
                        .border_r_1()
                        .border_color(theme.sidebar_border)
                        .child(bar(36., theme.foreground.opacity(0.6)))
                        .child(
                            div()
                                .mt_1()
                                .px_1()
                                .py_1()
                                .rounded(px(4.))
                                .bg(theme.sidebar_accent)
                                .child(bar(40., theme.foreground.opacity(0.7))),
                        )
                        .child(div().px_1().child(bar(34., line)))
                        .child(div().px_1().child(bar(44., line))),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .h_full()
                        .p_3()
                        .gap_2()
                        .child(bar(70., theme.foreground.opacity(0.7)))
                        .child(
                            h_flex().justify_end().child(
                                div()
                                    .px_2()
                                    .py_1p5()
                                    .rounded(px(6.))
                                    .bg(theme.secondary)
                                    .child(bar(56., line)),
                            ),
                        )
                        .child(
                            v_flex()
                                .gap_1()
                                .child(bar(110., line))
                                .child(bar(90., line))
                                .child(bar(100., line)),
                        )
                        .child(div().flex_1())
                        .child(
                            h_flex()
                                .h(px(28.))
                                .px_2()
                                .rounded(px(8.))
                                .border_1()
                                .border_color(theme.input)
                                .bg(theme.popover)
                                .child(bar(60., line))
                                .child(div().flex_1())
                                .child(div().size(px(14.)).rounded_full().bg(theme.primary)),
                        ),
                ),
        )
}
