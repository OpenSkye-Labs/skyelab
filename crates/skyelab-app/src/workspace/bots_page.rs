//! The Bots center page: a "Coming soon" placeholder that points back to Home and to Scheduled tasks.

use super::{OpenHome, OpenSettingsSection};
use crate::settings::Section;
use crate::ui::{self, Button, Pill, SkyeIcon};
use gpui::{
    App, Context, FocusHandle, Focusable, FontWeight, IntoElement, Render, Window, div, prelude::*,
    px,
};
use gpui_component::{ActiveTheme as _, Icon, h_flex, v_flex};

pub struct BotsPage {
    focus: FocusHandle,
}

impl BotsPage {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
        }
    }
}

impl Focusable for BotsPage {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for BotsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _render = crate::debug_renders::count("BotsPage");
        let theme = cx.theme();
        let header = super::layout::page_header(window, cx)
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_2()
                    .items_center()
                    .child(
                        Icon::new(SkyeIcon::Bot)
                            .size_4()
                            .text_color(theme.muted_foreground),
                    )
                    .child(
                        div()
                            .text_size(ui::BODY)
                            .font_weight(FontWeight::MEDIUM)
                            .child("Bots"),
                    ),
            )
            .child(super::layout::panel_toggles(window, cx));

        let hero = v_flex()
            .gap_3()
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(
                        div()
                            .size(px(36.))
                            .flex_shrink_0()
                            .rounded(ui::RADIUS)
                            .border_1()
                            .border_color(theme.border)
                            .bg(theme.muted)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                Icon::new(SkyeIcon::Bot)
                                    .size(px(18.))
                                    .text_color(theme.foreground),
                            ),
                    )
                    .child(ui::page_title("Bots", theme))
                    .child(Pill::new("Coming soon")),
            )
            .child(
                div()
                    .max_w(px(640.))
                    .text_size(ui::BODY)
                    .text_color(theme.muted_foreground)
                    .child(
                        "Bots will live here.",
                    ),
            )
            .child(
                h_flex()
                    .pt_1()
                    .gap_2()
                    .items_center()
                    .child(
                        Button::new("bots-back-home")
                            .debug_selector(|| "bots-back-home".into())
                            .primary()
                            .medium()
                            .icon(SkyeIcon::Home)
                            .label("Back to Home")
                            .on_click(|_, window, cx| {
                                crate::workspace::dispatch(Box::new(OpenHome), window, cx)
                            }),
                    )
                    .child(
                        Button::new("bots-open-scheduled")
                            .debug_selector(|| "bots-open-scheduled".into())
                            .outline()
                            .medium()
                            .icon(SkyeIcon::Clock)
                            .label("Open Scheduled tasks")
                            .on_click(|_, window, cx| {
                                crate::workspace::dispatch(
                                    Box::new(OpenSettingsSection {
                                        section: Section::Scheduled,
                                    }),
                                    window, cx,
                                )
                            }),
                    ),
            )
            .child(
                div()
                    .text_size(ui::META)
                    .text_color(theme.muted_foreground)
                    .child(
                        "Need recurring runs today? Scheduled tasks already run prompts on a schedule in any bound folder.",
                    ),
            );

        let body = div()
            .id("bots-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .w_full()
                    .max_w(px(760.))
                    .mx_auto()
                    .px_5()
                    .py_8()
                    .gap_6()
                    .child(hero),
            );

        v_flex()
            .id("bots-page")
            .debug_selector(|| "bots-page".into())
            .track_focus(&self.focus)
            .size_full()
            .bg(theme.background)
            .child(header)
            .child(body)
    }
}
