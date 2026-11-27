//! Settings → Appearance: theme cards with a live preview, mode, accent, density and text sizes. Every change goes
//! through `ui::themes::set`, which applies it to every window and saves it.

use super::widgets::{heading, section, setting};
use super::*;
use crate::ui::SkyeIcon;
use crate::ui::{self, SegmentedControl, Size, themes};
use gpui::{Hsla, accesskit};
use gpui_component::{Icon, h_flex, v_flex};
use skyelab_core::{AppearanceSettings, ColorMode, MotionMode};

/// One theme card in the wrapping grid.
const THEME_CARD_W: f32 = 148.;

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
        let mode = self
            .segmented(
                "mode",
                &[
                    (ColorMode::System, "System", SkyeIcon::Monitor),
                    (ColorMode::Light, "Light", SkyeIcon::Sun),
                    (ColorMode::Dark, "Dark", SkyeIcon::Moon),
                ],
                s.mode,
                |s, m| s.mode = m,
                cx,
            )
            .accessibility_label("Appearance mode")
            .accessibility_id("appearance-mode");
        let density = self
            .segmented(
                "density",
                &[
                    (false, "Comfortable", SkyeIcon::Rows3),
                    (true, "Compact", SkyeIcon::Minus),
                ],
                s.compact,
                |s, c| s.compact = c,
                cx,
            )
            .accessibility_label("Density");
        let motion = self
            .segmented(
                "motion",
                &[
                    (MotionMode::System, "System", SkyeIcon::Monitor),
                    (MotionMode::Reduced, "Reduced", SkyeIcon::Minus),
                ],
                s.motion,
                |s, m| s.motion = m,
                cx,
            )
            .accessibility_label("Motion");
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
            setting(
                false,
                "Motion",
                "Reduced turns off transitions and spinners across the app.",
                motion,
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
                    "text size",
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
                    "code text size",
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
                    .child(
                        h_flex()
                            .id("theme-group")
                            .role(accesskit::Role::RadioGroup)
                            .aria_label("Theme")
                            .flex_wrap()
                            .items_start()
                            .gap_2()
                            .children(cards),
                    ),
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
    ) -> super::scroll::TrackedButton {
        let theme = cx.theme();
        // The family's own colors in this mode, without the accent preset.
        let own = crate::ui::theme_load::resolve(f.json, theme.is_dark(), &Default::default()).ok();
        let own = own.as_ref().unwrap_or(theme);
        let (bg, side_bg, fg) = (own.background, own.sidebar, own.foreground);
        let (muted, primary, border) = (own.muted_foreground, own.primary, own.border);
        let ring = theme.ring;
        let accent_hover = theme.accent;
        let bar = |w: f32, c: Hsla| div().h(gpui::px(4.)).w(gpui::px(w)).rounded_full().bg(c);
        let key = f.key;
        let id = SharedString::from(format!("theme-{key}"));
        let button =
            gpui_base::Button::new(id.clone())
                .debug_selector(move || format!("theme-{key}"))
                .role(accesskit::Role::RadioButton)
                .selected(on)
                .aria_toggled(if on {
                    accesskit::Toggled::True
                } else {
                    accesskit::Toggled::False
                })
                .accessibility_label(f.label)
                .on_any_mouse_down(|_, window, _| window.prevent_default())
                .flex_col()
                .items_stretch()
                // A fixed width so the families wrap into rows instead of squeezing into one.
                .w(gpui::px(THEME_CARD_W))
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
                .hover(move |s| s.bg(accent_hover))
                .focus_visible(move |s| s.border_2().border_color(ring))
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
                                            .rounded(ui::RADIUS_SM)
                                            .bg(primary),
                                    ),
                                ),
                        ),
                )
                .child(
                    v_flex()
                        // gpui_base::Button sets a line height of 1 for one-line labels; the name and blurb wrap.
                        .line_height(gpui::phi())
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
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.set_appearance(|s| s.theme = key.into(), cx)
                }));
        super::scroll::track_button(id, button)
    }

    /// The accent swatches: the theme's own, then each preset; the picked one is ringed.
    fn accents(&self, s: &AppearanceSettings, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
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
        let focus_ring = theme.ring;
        let current = s.accent.clone();
        h_flex()
            .id("accent-group")
            .role(accesskit::Role::RadioGroup)
            .aria_label("Accent color")
            .gap_1p5()
            .children(
                std::iter::once(own)
                    .map(|(k, l, _)| (k, l, None))
                    .chain(
                        presets.map(|(k, l, hex)| (k, l, crate::ui::theme_load::parse_color(hex))),
                    )
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
                        let id = SharedString::from(format!("accent-{}", key.unwrap_or("theme")));
                        let button = gpui_base::Button::new(id.clone())
                            .role(accesskit::Role::RadioButton)
                            .selected(on)
                            .aria_toggled(if on {
                                accesskit::Toggled::True
                            } else {
                                accesskit::Toggled::False
                            })
                            .accessibility_label(format!("{label} accent"))
                            .on_any_mouse_down(|_, window, _| window.prevent_default())
                            .size(gpui::px(24.))
                            .p(gpui::px(3.))
                            .rounded_full()
                            .border_1()
                            .border_color(if on { ring } else { gpui::transparent_black() })
                            .focus_visible(move |s| s.border_2().border_color(focus_ring))
                            .cursor_pointer()
                            .child(swatch)
                            .tooltip(move |window, cx| {
                                crate::ui::Tooltip::new(label).build(window, cx)
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set_appearance(|s| s.accent = key.map(str::to_owned), cx)
                            }));
                        super::scroll::track_button(id, button)
                    }),
            )
    }

    /// A segmented control backed by [`ui::SegmentedControl`].
    fn segmented<T: Copy + PartialEq + 'static>(
        &self,
        id: &'static str,
        options: &[(T, &'static str, SkyeIcon)],
        current: T,
        set: fn(&mut AppearanceSettings, T),
        cx: &mut Context<Self>,
    ) -> SegmentedControl {
        let values: Vec<T> = options.iter().map(|&(v, ..)| v).collect();
        let selected = values.iter().position(|&v| v == current);
        let mut seg = SegmentedControl::new(id)
            .with_size(Size::Sm)
            .selected(selected);
        for &(_, label, icon) in options {
            seg = seg.icon_item(icon, label);
        }
        seg.on_click(cx.listener(move |this, &ix: &usize, _, cx| {
            if let Some(&value) = values.get(ix) {
                this.set_appearance(|s| set(s, value), cx);
            }
        }))
    }

    /// − size + within `range`.
    fn stepper(
        &self,
        id: &'static str,
        what: &'static str,
        value: u8,
        range: std::ops::RangeInclusive<u8>,
        set: fn(&mut AppearanceSettings, u8),
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let step = |dir: i8, icon: SkyeIcon, tip: String, cx: &mut Context<Self>| {
            let next = value
                .saturating_add_signed(dir)
                .clamp(*range.start(), *range.end());
            Button::new(SharedString::from(format!("{id}-{dir}")))
                .outline()
                .small()
                .icon(icon)
                .tooltip(tip)
                .disabled(next == value)
                .on_click(
                    cx.listener(move |this, _, _, cx| this.set_appearance(|s| set(s, next), cx)),
                )
        };
        h_flex()
            .gap_2()
            .child(step(-1, SkyeIcon::Minus, format!("Decrease {what}"), cx))
            .child(
                div()
                    .w(gpui::px(44.))
                    .text_center()
                    .text_size(ui::BODY)
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(format!("{value} pt")),
            )
            .child(step(1, SkyeIcon::Plus, format!("Increase {what}"), cx))
    }
}
