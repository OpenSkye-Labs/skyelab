//! The Remote tab: the sharing toggle, its status, and who is connected.
//! Its buttons dispatch actions that the Workspace handles, so they work from the keyboard and verify runs too.

use super::{DisconnectPeer, Remote, Status, ToggleSharing};
use gpui::{App, Context, Entity, FocusHandle, Focusable, Subscription, Window, div, prelude::*};
use gpui_component::{
    ActiveTheme as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    switch::Switch,
    v_flex,
};

pub struct RemotePanel {
    remote: Entity<Remote>,
    focus: FocusHandle,
    _observe: Subscription,
}

impl RemotePanel {
    pub fn new(remote: Entity<Remote>, cx: &mut Context<Self>) -> Self {
        Self {
            _observe: cx.observe(&remote, |_, _, cx| cx.notify()),
            remote,
            focus: cx.focus_handle(),
        }
    }
}

impl Focusable for RemotePanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for RemotePanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let remote = self.remote.read(cx);
        let status_color = match remote.status {
            Status::Failed(_) => theme.danger,
            Status::Sharing { .. } => theme.success,
            _ => theme.muted_foreground,
        };
        let status = remote
            .status
            .label()
            .unwrap_or_else(|| "Not sharing.".into());
        let peers = if remote.peers.is_empty() {
            vec![
                div()
                    .text_color(theme.muted_foreground)
                    .child("No devices connected.")
                    .into_any_element(),
            ]
        } else {
            remote
                .peers
                .iter()
                .map(|p| {
                    let id = p.id;
                    h_flex()
                        .id(("peer", id))
                        .gap_2()
                        .py_1()
                        .border_b_1()
                        .border_color(theme.border)
                        .child(div().flex_1().min_w_0().child(p.device.clone()))
                        .child(
                            div()
                                .text_color(theme.muted_foreground)
                                .child(format!("{} · {}", p.user, p.addr)),
                        )
                        .child(
                            Button::new(("disconnect", id))
                                .small()
                                .danger()
                                .label("Disconnect")
                                .on_click(move |_, window, cx| {
                                    window.dispatch_action(Box::new(DisconnectPeer { id }), cx)
                                }),
                        )
                        .into_any_element()
                })
                .collect()
        };
        v_flex()
            .id("remote")
            .key_context("Remote")
            .track_focus(&self.focus)
            .size_full()
            .p_4()
            .gap_4()
            .text_sm()
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        Switch::new("share-on-tailnet")
                            .checked(remote.enabled)
                            .label("Share this machine on my tailnet")
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(ToggleSharing), cx)
                            }),
                    )
                    .child(div().text_color(status_color).child(status))
                    .child(div().text_xs().text_color(theme.muted_foreground).child(
                        "Only your own untagged devices on your tailnet can connect. They can list projects \
                         and threads and run agents here, under each folder's bound account.",
                    )),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(div().text_lg().child("Connected devices"))
                    .children(peers),
            )
    }
}
