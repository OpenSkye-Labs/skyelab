//! The workspace's bottom status bar.

use super::{OpenDevices, OpenSettings, Workspace};
use crate::ui;
use crate::ui::ActiveTheme as _;
use crate::ui::Button;
use crate::ui::h_flex;
use gpui::{Context, IntoElement, div, prelude::*};
use gpui_component::status_bar::StatusBar;

impl Workspace {
    pub(super) fn render_status(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (badge, account_a11y) = match (&self.account_id, &self.account) {
            (Some(id), Some(label)) => (
                ui::AccountBadge::new(id.clone(), label.clone()),
                format!("Account: {label}"),
            ),
            _ => (ui::AccountBadge::unbound(), "Account: No account".into()),
        };
        let account = Button::new("status-account")
            .ghost()
            .xsmall()
            .accessibility_label(account_a11y)
            .accessibility_description("Opens accounts and folder bindings")
            .accessibility_id("status-account")
            .tooltip("Accounts and folder bindings")
            .child(badge.chip())
            .on_click(|_, window, cx| {
                crate::workspace::dispatch(Box::new(OpenSettings), window, cx)
            });
        let remote = self.is_remote();
        let devices_shown = self.devices_shown(cx);
        // Debug mode on: a chip that shows or hides the overlay (docs/debugging.md).
        let debug = skyelab_core::debug::enabled().then(|| {
            Button::new("status-debug")
                .ghost()
                .xsmall()
                .label("Debug")
                .tooltip("Debug mode is on: show or hide the overlay")
                .on_click(|_, window, cx| {
                    crate::workspace::dispatch(Box::new(super::ToggleDebugOverlay), window, cx)
                })
        });
        div()
            .id("status-bar")
            .role(gpui::accesskit::Role::Toolbar)
            .aria_label("Status bar")
            .accessibility_id("status-bar")
            .w_full()
            .flex_shrink_0()
            .child(
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
                                        .child(ui::short_path(
                                            &self.folder,
                                            dirs::home_dir().as_deref(),
                                        )),
                                )
                            })
                            .children(self.store_error.clone().map(|e| {
                                let msg = format!("Store error: {e}");
                                div()
                                    .id("store-error")
                                    .role(gpui::accesskit::Role::Alert)
                                    .aria_label(msg.clone())
                                    .text_color(crate::ui::ink(theme, theme.danger))
                                    .child(msg)
                            })),
                    )
                    .right(h_flex().children(debug))
                    .right(
                        Button::new("status-devices")
                            .ghost()
                            .xsmall()
                            .label("Devices")
                            .toggled(devices_shown)
                            .selected(devices_shown)
                            .tooltip_with_action(
                                "iOS Simulators and Android emulators",
                                &OpenDevices,
                                Some(super::CONTEXT),
                            )
                            .on_click(|_, window, cx| {
                                crate::workspace::dispatch(Box::new(OpenDevices), window, cx)
                            }),
                    )
                    .right(
                        h_flex()
                            .gap_2()
                            .children(self.render_remote_status(cx))
                            .when(!remote, |el| el.child(account)),
                    ),
            )
    }
}
