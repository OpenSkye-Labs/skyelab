//! Settings → Appearance: theme cards with a live preview, mode, accent, density and text sizes. Every change goes
//! through `ui::themes::set`, which applies it to every window and saves it.

use super::widgets::{heading, section, setting};
use super::*;
use crate::ui::SkyeIcon;
use crate::ui::{self, themes};
use gpui::Hsla;
use gpui_component::{Disableable as _, Icon, h_flex, try_parse_color, v_flex};
use skyelab_core::{AppearanceSettings, ColorMode};

/// A theme config color by its key, for the previews.
fn color(
    config: &gpui_component::ThemeConfig,
    pick: impl Fn(&gpui_component::ThemeConfigColors) -> Option<SharedString>,
) -> Option<Hsla> {
    pick(&config.colors).and_then(|c| try_parse_color(&c).ok())
}

impl Settings {
    fn set_appearance(
        &mut self,
        change: impl FnOnce(&mut AppearanceSettings),
        cx: &mut Context<Self>,
    ) {
        let mut s = themes::get(cx);
        change(&mut s);
        let save = themes::set(s, cx);
        self.report(save, "Appearance", cx);
        cx.notify();
    }

    pub(super) fn render_appearance(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let s = themes::get(cx);
        let picked = themes::family(&s.theme).key;
        let cards: Vec<_> = themes::FAMILIES
            .iter()
            .map(|f| self.theme_card(f, f.key == picked, cx))
            .collect();
        let mode = self.segmented(
            "mode",
            &[
                (
                    ColorMode::System,
                    "System",
                    Icon::new(crate::ui::SkyeIcon::Monitor),
                ),
                (ColorMode::Light, "Light", Icon::new(SkyeIcon::Sun)),
                (ColorMode::Dark, "Dark", Icon::new(SkyeIcon::Moon)),
            ],
            s.mode,
            |s, m| s.mode = m,
            cx,
        );
        let density = self.segmented(
            "density",
            &[
                (false, "Comfortable", Icon::new(crate::ui::SkyeIcon::Rows3)),
                (true, "Compact", Icon::new(SkyeIcon::Minus)),
            ],
            s.compact,
            |s, c| s.compact = c,
            cx,
        );
        let display = [
            setting(true, "Mode", "Light, dark, or follow the system.", mode, cx),
            setting(
                false,
                "Accent color",
                "Buttons, switches, links and focus rings.",
                self.accents(&s, cx),
                cx,
            ),
            setting(
                false,
                "Density",
                "Compact tightens rows in the sidebar, lists and settings.",
                density,
                cx,
            ),
        ];
        let text = [
            setting(
                true,
                "Text size",
                "The interface: sidebar, chat, menus and settings.",
                self.stepper(
                    "ui-size",
                    s.ui_font_size,
                    AppearanceSettings::UI_SIZES,
                    |s, v| s.ui_font_size = v,
                    cx,
                ),
                cx,
            ),
            setting(
                false,
                "Code text size",
                "Code blocks, diffs and editors.",
                self.stepper(
                    "code-size",
                    s.code_font_size,
                    AppearanceSettings::CODE_SIZES,
                    |s, v| s.code_font_size = v,
                    cx,
                ),
                cx,
            ),
        ];
        v_flex()
            .gap_8()
            .child(
                v_flex()
                    .gap_3()
                    .child(heading(
                        "Theme",
                        "Each theme has a light and a dark side; Mode picks which shows.",
                        cx,
                    ))
                    .child(h_flex().items_start().gap_2().children(cards)),
            )
            .child(section(
                "Display",
                "Light or dark, the accent color, and how dense lists are.",
                display,
                cx,
            ))
            .child(section(
                "Text",
                "Sizes in points; every window changes at once.",
                text,
                cx,
            ))
            .into_any_element()
    }

    /// A theme's card: a small picture of the app in its colors (this mode's side), its name and a line about it.
    fn theme_card(
        &self,
        f: &'static themes::Family,
        on: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let theme = cx.theme();
        let side = if theme.is_dark() { "Dark" } else { "Light" };
        let registry = gpui_component::ThemeRegistry::global(cx);
        let config = registry
            .themes()
            .get(format!("{} {side}", f.label).as_str())
            .cloned();
        let pick = |p: fn(&gpui_component::ThemeConfigColors) -> Option<SharedString>, or: Hsla| {
            config.as_ref().and_then(|c| color(c, p)).unwrap_or(or)
        };
        let bg = pick(|c| c.background.clone(), theme.background);
        let side_bg = pick(|c| c.sidebar.clone(), theme.sidebar);
        let fg = pick(|c| c.foreground.clone(), theme.foreground);
        let muted = pick(|c| c.muted_foreground.clone(), theme.muted_foreground);
        let primary = pick(|c| c.primary.clone(), theme.primary);
        let border = pick(|c| c.border.clone(), theme.border);
        let bar = |w: f32, c: Hsla| div().h(gpui::px(4.)).w(gpui::px(w)).rounded_full().bg(c);
        let key = f.key;
        v_flex()
            .id(SharedString::from(format!("theme-{key}")))
            .debug_selector(move || format!("theme-{key}"))
            .flex_1()
            .min_w_0()
            .gap_2()
            .p_1p5()
            .rounded(ui::RADIUS_LG)
            .border_1()
            .border_color(if on {
                theme.primary
            } else {
                gpui::transparent_black()
            })
            .when(on, |d| d.bg(theme.primary.opacity(0.06)))
            .cursor_pointer()
            .hover(|s| s.bg(theme.accent))
            .child(
                h_flex()
                    .h(gpui::px(88.))
                    .rounded(ui::RADIUS)
                    .border_1()
                    .border_color(border)
                    .bg(bg)
                    .overflow_hidden()
                    .child(
                        v_flex()
                            .w(gpui::px(44.))
                            .h_full()
                            .p_2()
                            .gap_1p5()
                            .bg(side_bg)
                            .child(bar(24., muted.opacity(0.7)))
                            .child(bar(30., fg.opacity(0.8)))
                            .child(bar(20., muted.opacity(0.7))),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .h_full()
                            .p_2p5()
                            .gap_1p5()
                            .child(bar(48., fg))
                            .child(bar(64., muted))
                            .child(bar(40., muted))
                            .child(div().flex_1())
                            .child(
                                h_flex().justify_end().child(
                                    div()
                                        .h(gpui::px(12.))
                                        .w(gpui::px(28.))
                                        .rounded(gpui::px(4.))
                                        .bg(primary),
                                ),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .px_1()
                    .pb_0p5()
                    .child(
                        h_flex()
                            .gap_1p5()
                            .text_size(ui::BODY)
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child(f.label)
                            .when(on, |d| {
                                d.child(
                                    Icon::new(SkyeIcon::Check)
                                        .size_3p5()
                                        .text_color(theme.primary),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_size(ui::META)
                            .text_color(theme.muted_foreground)
                            .line_clamp(2)
                            .child(f.about),
                    ),
            )
            .on_click(
                cx.listener(move |this, _, _, cx| {
                    this.set_appearance(|s| s.theme = key.into(), cx)
                }),
            )
    }

    /// The accent swatches: the theme's own, then each preset; the picked one is ringed.
    fn accents(&self, s: &AppearanceSettings, cx: &mut Context<Self>) -> gpui::Div {
        let theme = cx.theme();
        let dark = theme.is_dark();
        let own = (None, "Theme", themes::family(&s.theme).label);
        let presets = themes::ACCENTS.iter().map(|a| {
            (
                Some(a.key),
                a.label,
                if dark { a.dark.0 } else { a.light.0 },
            )
        });
        let ring = theme.foreground;
        let current = s.accent.clone();
        h_flex().gap_1p5().children(
            std::iter::once(own)
                .map(|(k, l, _)| (k, l, None))
                .chain(presets.map(|(k, l, hex)| (k, l, try_parse_color(hex).ok())))
                .map(|(key, label, fill)| {
                    let on = current.as_deref() == key;
                    let theme = cx.theme();
                    // The theme's own accent (no preset): an outlined, lightly filled swatch, so it reads apart from the presets.
                    let swatch = match fill {
                        Some(c) => div().size_full().rounded_full().bg(c),
                        None => div()
                            .size_full()
                            .rounded_full()
                            .border_2()
                            .border_color(theme.primary)
                            .child(
                                div()
                                    .size_full()
                                    .rounded_full()
                                    .bg(theme.primary.opacity(0.35)),
                            ),
                    };
                    div()
                        .id(SharedString::from(format!(
                            "accent-{}",
                            key.unwrap_or("theme")
                        )))
                        .size(gpui::px(24.))
                        .p(gpui::px(3.))
                        .rounded_full()
                        .border_1()
                        .border_color(if on { ring } else { gpui::transparent_black() })
                        .cursor_pointer()
                        .child(swatch)
                        .tooltip(move |window, cx| {
                            gpui_component::tooltip::Tooltip::new(label).build(window, cx)
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.set_appearance(|s| s.accent = key.map(str::to_owned), cx)
                        }))
                }),
        )
    }

    /// A segmented control (T3's toggle group): one choice of `options`, the picked one lifted.
    fn segmented<T: Copy + PartialEq + 'static>(
        &self,
        id: &'static str,
        options: &[(T, &'static str, Icon)],
        current: T,
        set: fn(&mut AppearanceSettings, T),
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let theme = cx.theme();
        h_flex()
            .p_0p5()
            .gap_0p5()
            .rounded(ui::RADIUS)
            .bg(theme.secondary)
            .children(options.iter().map(|(value, label, icon)| {
                let (value, on) = (*value, *value == current);
                let theme = cx.theme();
                h_flex()
                    .id(SharedString::from(format!("{id}-{label}")))
                    .h_7()
                    .px_2p5()
                    .gap_1p5()
                    .rounded(ui::RADIUS_MD)
                    .text_size(ui::META)
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .cursor_pointer()
                    .text_color(if on {
                        theme.foreground
                    } else {
                        theme.muted_foreground
                    })
                    .when(on, |d| {
                        d.bg(theme.background).border_1().border_color(theme.border)
                    })
                    .child(icon.clone().size_3p5())
                    .child(*label)
                    .on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.set_appearance(|s| set(s, value), cx)
                        }),
                    )
            }))
    }

    /// − size + within `range`.
    fn stepper(
        &self,
        id: &'static str,
        value: u8,
        range: std::ops::RangeInclusive<u8>,
        set: fn(&mut AppearanceSettings, u8),
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let step = |dir: i8, icon: SkyeIcon, cx: &mut Context<Self>| {
            let next = value
                .saturating_add_signed(dir)
                .clamp(*range.start(), *range.end());
            Button::new(SharedString::from(format!("{id}-{dir}")))
                .outline()
                .small()
                .icon(icon)
                .disabled(next == value)
                .on_click(
                    cx.listener(move |this, _, _, cx| this.set_appearance(|s| set(s, next), cx)),
                )
        };
        h_flex()
            .gap_2()
            .child(step(-1, SkyeIcon::Minus, cx))
            .child(
                div()
                    .w(gpui::px(44.))
                    .text_center()
                    .text_size(ui::BODY)
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(format!("{value} pt")),
            )
            .child(step(1, SkyeIcon::Plus, cx))
    }
}
