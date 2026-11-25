//! Onboarding's frame and the pages that need no detection, laid out after Rex's: progress dots on top, then one
//! centered column per step (hero tile, bold title, muted line, one rounded panel holding the step's controls, a
//! round Back and a pill Continue, and a text link: Skip, or Back on Done). A step change crossfades the old column
//! out over the new one. Welcome and Done live here; Theme is in `theme.rs`.

use super::{Onboarding, Step, motion};
use crate::ui;
use crate::ui::{Button, Card, Divider, Kbd, SkyeIcon};
use gpui::{
    AnyElement, Context, Div, FontWeight, Hsla, ParentElement as _, Pixels, SharedString, Window,
    div, linear_color_stop, linear_gradient, prelude::*, px,
};
use gpui_component::{ActiveTheme as _, Icon, h_flex, v_flex};

/// Width of a dot, and of the current step's.
const DOT: f32 = 6.;
const DOT_ON: f32 = 22.;
/// The hero tile's edge.
const HERO: f32 = 72.;
/// The column's width.
const COLUMN: f32 = 600.;

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

    /// The whole page: the dots, then the step's column fading in, over the old one fading out after a change. The
    /// old column sits under a layer that takes the mouse, so a click during the fade never reaches it.
    pub(super) fn frame(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let step = self.step;
        let page = self.column(step, window, cx);
        let leaving = self.leaving.map(|s| (s, self.column(s, window, cx)));
        let centered = || {
            v_flex()
                .w_full()
                .items_center()
                .justify_center()
                .px_6()
                .py_8()
        };
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .h(ui::HEADER_H)
                    .flex_shrink_0()
                    .justify_center()
                    .child(self.dots(cx)),
            )
            .child(
                div()
                    .id("onboarding-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .min_h_full()
                            .flex()
                            .flex_col()
                            .children(leaving.map(|(s, old)| {
                                motion::fade_out(
                                    centered()
                                        .absolute()
                                        .inset_0()
                                        .debug_selector(|| "onboarding-leaving".into())
                                        .child(old),
                                    ("onboarding-leaving", s as usize),
                                )
                            }))
                            .children(self.leaving.map(|_| div().absolute().inset_0().occlude()))
                            .child(motion::fade_in(
                                centered()
                                    .flex_1()
                                    .debug_selector(|| "onboarding-page".into())
                                    .child(page),
                                ("onboarding-page", step as usize),
                            )),
                    ),
            )
    }

    /// One step's column: hero, title and line, the panel, then the actions.
    fn column(&self, step: Step, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let (title, about): (&'static str, SharedString) = match step {
            Step::Welcome => (
                "Welcome to SkyeLab",
                "Every folder runs on the account it is bound to, and nothing ever falls back to another."
                    .into(),
            ),
            Step::Theme => (
                "Make it yours",
                "Pick a theme. The whole app changes as you click.".into(),
            ),
            Step::Agents => (
                "Agents and accounts",
                "Each Claude or Codex login keeps its own config folder. Add the ones you use.".into(),
            ),
            Step::Import => (
                "Bring in your projects",
                "Folders your agents worked in. Each runs on the account shown; change it before importing."
                    .into(),
            ),
            Step::Done => ("You're set", self.ready_line().into()),
        };
        let body = match step {
            Step::Welcome => self.welcome_page(cx).into_any_element(),
            Step::Theme => self.theme_page(cx).into_any_element(),
            Step::Agents => self.agents_page(cx).into_any_element(),
            Step::Import => self.import_page(cx).into_any_element(),
            Step::Done => self.done_page(window, cx).into_any_element(),
        };
        let theme = cx.theme();
        v_flex()
            .w_full()
            .max_w(px(COLUMN))
            .items_center()
            .child(hero(step, cx))
            .child(
                div()
                    .mt_5()
                    .text_size(px(26.))
                    .font_weight(FontWeight::BOLD)
                    .text_center()
                    .child(title),
            )
            .child(
                div()
                    .mt_2()
                    .text_size(ui::SECTION)
                    .text_color(theme.muted_foreground)
                    .text_center()
                    .child(about),
            )
            .child(Card::new().mt_6().w_full().child(body))
            .children(self.error.clone().map(|e| {
                div()
                    .mt_3()
                    .w_full()
                    .text_size(ui::META)
                    .text_color(ui::ink(theme, theme.danger))
                    .child(e)
            }))
            .child(self.actions(step, cx))
            .into_any_element()
    }

    /// The round Back and the pill next action, then a text link: Skip, or on Done the way back.
    fn actions(&self, step: Step, cx: &mut Context<Self>) -> Div {
        let back = Button::new("onboarding-back")
            .secondary()
            .square()
            .rounded_full()
            .icon(SkyeIcon::ChevronLeft)
            .tooltip("Back")
            .on_click(cx.listener(|this, _, _, cx| this.back(cx)));
        let link = match step {
            Step::Done => div().debug_selector(|| "onboarding-back".into()).child(
                Button::new("onboarding-back-link")
                    .ghost()
                    .small()
                    .icon(SkyeIcon::ChevronLeft)
                    .label("Return to projects")
                    .on_click(cx.listener(|this, _, _, cx| this.back(cx))),
            ),
            _ => div().debug_selector(|| "onboarding-skip".into()).child(
                Button::new("onboarding-skip")
                    .ghost()
                    .small()
                    .label(if step == Step::Welcome {
                        "Skip for now"
                    } else {
                        "Skip setup"
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.skip(cx))),
            ),
        };
        v_flex()
            .mt_6()
            .items_center()
            .gap_3()
            .child(
                h_flex()
                    .gap_2()
                    .when(!matches!(step, Step::Welcome | Step::Done), |d| {
                        d.child(
                            div()
                                .debug_selector(|| "onboarding-back".into())
                                .child(back),
                        )
                    })
                    .child(
                        div().debug_selector(|| "onboarding-next".into()).child(
                            Button::new("onboarding-next")
                                .secondary()
                                .rounded_full()
                                .px_4()
                                .label(self.next_label(step))
                                .when(step != Step::Done, |b| b.icon_end(SkyeIcon::ArrowRight))
                                .on_click(cx.listener(|this, _, _, cx| this.next(cx))),
                        ),
                    ),
            )
            .child(link)
    }

    pub(super) fn welcome_page(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        v_flex()
            .items_center()
            .gap_4()
            .py_2()
            .child(self.idea(cx))
            .child(
                div()
                    .text_size(ui::BODY)
                    .text_color(theme.muted_foreground)
                    .text_center()
                    .child("A few steps: pick a look, check your agents and accounts, and bring in your projects."),
            )
    }

    /// The line under Done's title: what is ready.
    fn ready_line(&self) -> String {
        let accounts = self.config.accounts.len();
        let projects = self.projects.len();
        format!(
            "{accounts} account{} and {projects} project{} ready. Here is how to get going.",
            if accounts == 1 { "" } else { "s" },
            if projects == 1 { "" } else { "s" }
        )
    }

    pub(super) fn done_page(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let tips = [
            (
                Kbd::for_action(
                    &crate::palette::OpenCommandPalette,
                    Some("Workspace"),
                    window,
                ),
                "Find any action, project or thread",
            ),
            (
                Kbd::for_action(&super::super::NewThread, Some("Workspace"), window),
                "Start a thread in the current project",
            ),
            (
                None,
                "Add project in the palette adds a folder, a new repo or a clone",
            ),
            (None, "Open Onboarding in the palette brings this back"),
        ];
        let mut list = v_flex();
        for (i, (key, text)) in tips.into_iter().enumerate() {
            if i > 0 {
                list = list.child(Divider::horizontal());
            }
            list = list.child(
                h_flex()
                    .h(px(40.))
                    .gap_3()
                    .justify_between()
                    .child(div().text_size(ui::BODY).child(text))
                    .child(h_flex().flex_shrink_0().children(key)),
            );
        }
        list.text_color(theme.foreground)
    }

    /// What Next says on `step`.
    fn next_label(&self, step: Step) -> SharedString {
        match step {
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

/// A small muted heading inside the panel.
pub(super) fn caption(text: &'static str, cx: &gpui::App) -> Div {
    div()
        .text_size(ui::META)
        .font_weight(FontWeight::MEDIUM)
        .text_color(cx.theme().muted_foreground)
        .child(text)
}

/// The step's hero: an app-icon-style tile with the step's glyph, zooming in as the step appears.
fn hero(step: Step, cx: &gpui::App) -> impl IntoElement {
    let theme = cx.theme();
    let (bg, fg) = (theme.primary, theme.primary_foreground);
    let shade = |by: f32| Hsla {
        l: (bg.l + by).clamp(0., 1.),
        ..bg
    };
    let (top, bottom) = (shade(0.08), shade(-0.06));
    let border = theme.foreground.opacity(0.08);
    motion::hero(
        ("onboarding-hero", step as usize),
        px(HERO),
        move |edge: Pixels| {
            let glyph = edge * 0.5;
            div()
                .debug_selector(|| "onboarding-hero".into())
                .size(edge)
                .flex()
                .items_center()
                .justify_center()
                .rounded(edge * 0.26)
                .border_1()
                .border_color(border)
                .bg(linear_gradient(
                    180.,
                    linear_color_stop(top, 0.),
                    linear_color_stop(bottom, 1.),
                ))
                .shadow_lg()
                .child(match step {
                    Step::Welcome => ui::grip_mark(glyph, fg).into_any_element(),
                    _ => Icon::new(match step {
                        Step::Theme => SkyeIcon::Palette,
                        Step::Agents => SkyeIcon::Bot,
                        Step::Import => SkyeIcon::FolderOpen,
                        _ => SkyeIcon::Sparkles,
                    })
                    .size(glyph)
                    .text_color(fg)
                    .into_any_element(),
                })
        },
    )
}
