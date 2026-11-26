//! Settings → Scheduled tasks: a pointer to the Bots page, which holds the one list of scheduled tasks and bots.

use super::{Settings, widgets};
use crate::ui::{Button, SkyeIcon};
use gpui::{AnyElement, Context, prelude::*};

impl Settings {
    pub(super) fn render_scheduled(&self, cx: &mut Context<Self>) -> AnyElement {
        let open = widgets::setting(
            true,
            "Scheduled tasks are bots now",
            "Every scheduled task, and every bot GitHub starts, is listed and edited on the Bots page.",
            Button::new("settings-open-bots")
                .debug_selector(|| "settings-open-bots".into())
                .outline()
                .small()
                .icon(SkyeIcon::Bot)
                .label("Open Bots")
                .on_click(|_, window, cx| {
                    crate::workspace::dispatch(Box::new(crate::workspace::OpenBots), window, cx)
                }),
            cx,
        );
        widgets::section(
            "Scheduled tasks",
            "Saved prompts that run on a schedule or when GitHub has news.",
            [open],
            cx,
        )
        .into_any_element()
    }
}
