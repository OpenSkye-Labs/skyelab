//! Onboarding's frame and the pages that need no detection: progress dots and Skip on top, one page in the middle,
//! Back and the next action at the bottom (one page per step); Welcome, Theme with a live miniature of the
//! app, and Done.

use super::{Onboarding, OnboardingNext, Step, motion};
use crate::ui;
use crate::ui::{Button, Kbd, SegmentedControl, SkyeIcon};
use gpui::{
    AnyElement, Context, Div, FontWeight, ParentElement as _, SharedString, Window, div,
    prelude::*, px,
};
use gpui_component::{ActiveTheme as _, Icon, h_flex, v_flex};
use skyelab_core::ColorMode;

/// Width of a dot, and of the current step's.
const DOT: f32 = 6.;
const DOT_ON: f32 = 22.;

impl Onboarding {
    /// The row of progress dots: the current step's is wide, and morphs as the step changes.
    fn dots(&self, cx: &Context<Self>) -> Div {
        let theme = cx.theme();
        let (now, before) = (self.step as usize, self.from as usize);
        h_flex()
            .debug_selector(|| "onboarding-dots".into())
            .gap_1p5()
            .children(Step::ALL.iter().enumerate().map(|(i, _)| {
                let on = i == now;
                let dot = div()
                    .h(px(DOT))
                    .w(px(if on { DOT_ON } else { DOT }))
                    .rounded_full()
                    .bg(if on {
                        theme.foreground
                    } else {
                        theme.foreground.opacity(0.18)
                    });
                let (from, to) = match (on, i == before && before != now) {
                    (true, _) => (DOT, DOT_ON),
                    (false, true) => (DOT_ON, DOT),
                    (false, false) => (DOT, DOT),
                };
                // Keyed by the step shown, so each change morphs once and then stays still.
                motion::dot(dot, ("onboarding-dot", i * 16 + now), from, to).into_any_element()
            }))
    }

    /// The whole page: dots and Skip, the step's page sliding in, Back and the next action.
    pub(super) fn frame(
        &mut self,
        page: AnyElement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = cx.theme();
        let step = self.step;
        let enter = Kbd::for_action_in(&OnboardingNext, &self.focus, window);
        let next = self.next_label();
        let forward = self.forward;
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .h(ui::HEADER_H)
                    .px_5()
                    .justify_between()
                    .child(div().w(px(120.)))
                    .child(self.dots(cx))
                    .child(
                        h_flex()
                            .w(px(120.))
                            .justify_end()
                            .when(step != Step::Done, |d| {
                                d.child(
                                    div().debug_selector(|| "onboarding-skip".into()).child(
                                        Button::new("onboarding-skip")
                                            .ghost()
                                            .small()
                                            .label("Skip")
                                            .on_click(cx.listener(|this, _, _, cx| this.skip(cx))),
                                    ),
                                )
                            }),
                    ),
            )
            .child(
                div()
                    .id("onboarding-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .w_full()
                            .max_w(px(640.))
                            .mx_auto()
                            .px_6()
                            .pt_8()
                            .pb_8()
                            .child(motion::page(
                                div().relative().child(page),
                                ("onboarding-page", step as usize),
                                forward,
                            )),
                    ),
            )
            .children(self.error.clone().map(|e| {
                div()
                    .w_full()
                    .max_w(px(640.))
                    .mx_auto()
                    .px_6()
                    .pb_2()
                    .text_size(ui::META)
                    .text_color(ui::ink(theme, theme.danger))
                    .child(e)
            }))
            .child(
                h_flex()
                    .w_full()
                    .max_w(px(640.))
                    .mx_auto()
                    .px_6()
                    .pb_6()
                    .pt_3()
                    .gap_3()
                    .child(
                        div()
                            .debug_selector(|| "onboarding-back".into())
                            .when(step == Step::Welcome, |d| d.invisible())
                            .child(
                                Button::new("onboarding-back")
                                    .ghost()
                                    .label("Back")
                                    .icon(SkyeIcon::ChevronLeft)
                                    .disabled(step == Step::Welcome)
                                    .on_click(cx.listener(|this, _, _, cx| this.back(cx))),
                            ),
                    )
                    .child(div().flex_1())
                    .child(
                        h_flex()
                            .gap_1p5()
                            .text_size(ui::META)
                            .text_color(theme.muted_foreground)
                            .children(enter.map(|k| k.with_label("Return")))
                            .child("to continue"),
                    )
                    .child(
                        div().debug_selector(|| "onboarding-next".into()).child(
                            Button::new("onboarding-next")
                                .primary()
                                .label(next)
                                .on_click(cx.listener(|this, _, _, cx| this.next(cx))),
                        ),
                    ),
            )
    }

    /// A page's heading: a title and one muted line.
    pub(super) fn heading(title: &'static str, about: &'static str, cx: &Context<Self>) -> Div {
        v_flex()
            .gap_1p5()
            .mb_6()
            .child(
                div()
                    .text_size(px(24.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(
                div()
                    .text_size(ui::BODY)
                    .text_color(cx.theme().muted_foreground)
                    .child(about),
            )
    }

    pub(super) fn welcome_page(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        v_flex()
            .pt_10()
            .gap_6()
            .child(
                div()
                    .size(px(64.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(ui::RADIUS_XL)
                    .bg(theme.secondary)
                    .child(ui::grip_mark(px(40.), theme.foreground)),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(div().text_size(px(30.)).font_weight(FontWeight::SEMIBOLD).child("Welcome to SkyeLab"))
                    .child(div().text_size(ui::SECTION).text_color(theme.muted_foreground).child(
                        "Every folder runs on the account it is bound to, and nothing ever falls back to another.",
                    )),
            )
            .child(self.idea(cx))
            .child(
                div()
                    .text_size(ui::BODY)
                    .text_color(theme.muted_foreground)
                    .child("A few steps: pick a look, check your agents and accounts, and bring in your projects."),
            )
    }

    pub(super) fn theme_page(&self, cx: &mut Context<Self>) -> Div {
        let current = ui::themes::get(cx);
        let mut list = v_flex().gap_1p5();
        for (i, f) in ui::themes::FAMILIES.iter().enumerate() {
            let on = current.theme == f.key;
            let theme = cx.theme();
            let key = f.key;
            list = list.child(motion::row(
                h_flex()
                    .relative()
                    .id(("onboarding-family", i))
                    .debug_selector(move || format!("onboarding-family-{key}"))
                    .px_3()
                    .py_2p5()
                    .gap_3()
                    .rounded(ui::RADIUS)
                    .border_1()
                    .border_color(if on { theme.ring } else { theme.border })
                    .bg(if on { theme.accent } else { theme.background })
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.accent))
                    .on_click(cx.listener(move |this, _, _, cx| this.pick_family(key, cx)))
                    .child(
                        v_flex()
                            .flex_1()
                            .child(
                                div()
                                    .text_size(ui::BODY)
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(f.label),
                            )
                            .child(
                                div()
                                    .text_size(ui::META)
                                    .text_color(theme.muted_foreground)
                                    .child(f.about),
                            ),
                    )
                    .when(on, |d| {
                        d.child(
                            Icon::new(SkyeIcon::Check)
                                .size_4()
                                .text_color(theme.foreground),
                        )
                    }),
                ("onboarding-family-row", i),
                i,
            ));
        }
        let mode = match current.mode {
            ColorMode::System => 0,
            ColorMode::Light => 1,
            ColorMode::Dark => 2,
        };
        v_flex()
            .child(Self::heading(
                "Make it yours",
                "Pick a theme. The whole app changes as you click.",
                cx,
            ))
            .child(
                h_flex()
                    .items_start()
                    .gap_6()
                    .child(
                        v_flex().flex_1().gap_4().child(list).child(
                            h_flex().child(
                                div().debug_selector(|| "onboarding-mode".into()).child(
                                    SegmentedControl::new("onboarding-mode")
                                        .icon_item(SkyeIcon::Monitor, "System")
                                        .icon_item(SkyeIcon::Sun, "Light")
                                        .icon_item(SkyeIcon::Moon, "Dark")
                                        .selected(Some(mode))
                                        .on_click(cx.listener(|this, ix: &usize, _, cx| {
                                            this.pick_mode(*ix, cx)
                                        })),
                                ),
                            ),
                        ),
                    )
                    .child(preview(cx)),
            )
    }

    pub(super) fn done_page(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let tip = |i: usize, key: Option<Kbd>, text: &'static str| {
            motion::row(
                h_flex()
                    .relative()
                    .gap_3()
                    .h(px(36.))
                    // A fixed key column, so the lines start together with or without a key.
                    .child(h_flex().w(px(56.)).children(key))
                    .child(div().text_size(ui::BODY).child(text)),
                ("onboarding-tip", i),
                i,
            )
        };
        let accounts = self.config.accounts.len();
        let projects = self.projects.len();
        v_flex()
            .pt_6()
            .child(Self::heading("You're set", "Here is how to get going.", cx))
            .child(
                div()
                    .mb_4()
                    .text_size(ui::BODY)
                    .text_color(theme.muted_foreground)
                    .child(format!(
                        "{accounts} account{} and {projects} project{} ready.",
                        if accounts == 1 { "" } else { "s" },
                        if projects == 1 { "" } else { "s" }
                    )),
            )
            .child(tip(
                0,
                Kbd::for_action(
                    &crate::palette::OpenCommandPalette,
                    Some("Workspace"),
                    window,
                ),
                "Find any action, project or thread",
            ))
            .child(tip(
                1,
                Kbd::for_action(&super::super::NewThread, Some("Workspace"), window),
                "Start a thread in the current project",
            ))
            .child(tip(
                2,
                None,
                "Add project in the palette adds a folder, a new repo or a clone",
            ))
            .child(tip(
                3,
                None,
                "Open Onboarding in the palette brings this back",
            ))
    }

    /// What Next says on this step.
    fn next_label(&self) -> SharedString {
        match self.step {
            Step::Welcome => "Get started".into(),
            Step::Theme | Step::Agents => "Continue".into(),
            Step::Import => match self.picked().len() {
                0 => "Continue".into(),
                1 => "Import 1 project".into(),
                n => format!("Import {n} projects").into(),
            },
            Step::Done => "Start building".into(),
        }
    }
}

/// A miniature of the app in the theme now applied: sidebar with a selected row, a thread, the composer.
fn preview(cx: &Context<Onboarding>) -> impl IntoElement {
    let theme = cx.theme();
    let bar = |w: f32, color: gpui::Hsla| div().h(px(6.)).w(px(w)).rounded_full().bg(color);
    let line = theme.muted_foreground.opacity(0.5);
    v_flex()
        .debug_selector(|| "onboarding-preview".into())
        .flex_shrink_0()
        .w(px(240.))
        .h(px(176.))
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
