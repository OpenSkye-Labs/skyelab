//! The workspace's bottom status bar.

use super::{OpenDevices, OpenSettings, Workspace};
use crate::ui;
use gpui::{Context, IntoElement, div, prelude::*};
use gpui_component::{
    ActiveTheme as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    status_bar::StatusBar,
};

impl Workspace {
    pub(super) fn render_status(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let badge = match (&self.account_id, &self.account) {
            (Some(id), Some(label)) => ui::AccountBadge::new(id.clone(), label.clone()),
            _ => ui::AccountBadge::unbound(),
        };
        let account = Button::new("status-account")
            .ghost()
            .xsmall()
            .tooltip("Accounts and folder bindings")
            .child(badge.chip())
            .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenSettings), cx));
        let remote = self.is_remote();
        // Debug mode on: a chip that shows or hides the overlay (docs/debugging.md).
        let debug = skyelab_core::debug::enabled().then(|| {
            Button::new("status-debug")
                .ghost()
                .xsmall()
                .label("Debug")
                .tooltip("Debug mode is on: show or hide the overlay")
                .on_click(|_, window, cx| {
                    window.dispatch_action(Box::new(super::ToggleDebugOverlay), cx)
                })
        });
        StatusBar::new()
            .left(
                h_flex()
                    .min_w_0()
                    .gap_3()
                    .text_size(ui::META)
                    .children(self.render_env_badge(cx))
                    .when(!remote, |el| {
                        el.child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_color(theme.muted_foreground)
                                .child(ui::short_path(&self.folder, dirs::home_dir().as_deref())),
                        )
                    })
                    .children(self.store_error.clone().map(|e| {
                        div()
                            .text_color(crate::ui::ink(theme, theme.danger))
                            .child(format!("Store error: {e}"))
                    })),
            )
            .right(h_flex().children(debug))
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
