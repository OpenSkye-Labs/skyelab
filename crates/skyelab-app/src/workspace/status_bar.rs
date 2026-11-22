//! The workspace's bottom status bar.

use super::{OpenDevices, OpenSettings, Workspace};
use gpui::{Context, IntoElement, div, prelude::*, px};
use gpui_component::{
    ActiveTheme as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    status_bar::StatusBar,
};

impl Workspace {
    pub(super) fn render_status(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (dot, label) = match &self.account {
            Some(label) => (theme.success, label.clone()),
            None => (theme.warning, "No account bound".into()),
        };
        let account = Button::new("status-account")
            .ghost()
            .xsmall()
            .tooltip("Accounts and folder bindings")
            .child(
                h_flex()
                    .gap_1p5()
                    .child(div().size(px(6.)).rounded_full().bg(dot))
                    .child(label),
            )
            .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenSettings), cx));
        let remote = self.is_remote();
        StatusBar::new()
            .left(
                h_flex()
                    .gap_3()
                    .text_xs()
                    .children(self.render_env_badge(cx))
                    .when(!remote, |el| {
                        el.child(
                            div()
                                .text_color(theme.muted_foreground)
                                .child(self.folder_label.clone()),
                        )
                    })
                    .children(self.store_error.clone().map(|e| {
                        div()
                            .text_color(theme.danger)
                            .child(format!("Store error: {e}"))
                    })),
            )
            .right(
                Button::new("status-devices")
                    .ghost()
                    .xsmall()
                    .label("Devices")
                    .tooltip("iOS Simulators and Android emulators")
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenDevices), cx)),
            )
            .right(
                h_flex()
                    .gap_2()
                    .children(self.render_remote_status(cx))
                    .when(!remote, |el| el.child(account)),
            )
    }
}
