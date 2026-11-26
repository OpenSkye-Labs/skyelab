//! The Bots center page: every scheduled task and bot (a task GitHub can also start) in one list, with its editor
//! and run history (`scheduled::ScheduledPage`).

use super::SharedStore;
use super::scheduled::ScheduledPage;
use crate::ui::{self, SkyeIcon};
use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, FontWeight, IntoElement, Render,
    Window, div, prelude::*, px,
};
use gpui_component::{ActiveTheme as _, Icon, h_flex, v_flex};
use std::path::PathBuf;

pub struct BotsPage {
    pub(crate) list: Entity<ScheduledPage>,
}

impl BotsPage {
    pub fn new(store: SharedStore, accounts: Option<PathBuf>, cx: &mut Context<Self>) -> Self {
        Self {
            list: cx.new(|cx| ScheduledPage::new(store, accounts, cx)),
        }
    }
}

impl Focusable for BotsPage {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.list.read(cx).focus_handle(cx)
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
        let body = div()
            .id("bots-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .w_full()
                    .max_w(px(860.))
                    .mx_auto()
                    .px_5()
                    .py_6()
                    .child(self.list.clone()),
            );
        v_flex()
            .id("bots-page")
            .debug_selector(|| "bots-page".into())
            .size_full()
            .bg(theme.background)
            .child(header)
            .child(body)
    }
}
