//! The Remote tab. Its buttons dispatch actions the Workspace handles, so SKYELAB_VERIFY_ACTIONS can drive them.

use super::{AllowDevice, DenyDevice, DisconnectPeer, Remote, RevokeDevice, ToggleSharing};
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
        let (status, status_color) = (remote.status.label(), remote.status.color(theme));
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
        let waiting: Vec<_> = remote
            .requests
            .iter()
            .enumerate()
            .map(|(n, r)| {
                let (allow, deny) = (
                    AllowDevice {
                        node_id: r.node_id.clone(),
                        key_hash: r.key_hash.clone(),
                    },
                    DenyDevice {
                        node_id: r.node_id.clone(),
                        key_hash: r.key_hash.clone(),
                    },
                );
                h_flex()
                    .id(("request", n))
                    .gap_2()
                    .py_1()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(div().flex_1().min_w_0().child(Remote::request_label(r)))
                    .child(
                        div()
                            .text_color(theme.muted_foreground)
                            .child(format!("code {}", r.code)),
                    )
                    .child(
                        Button::new(("allow", n))
                            .small()
                            .primary()
                            .label("Allow")
                            .on_click(move |_, window, cx| {
                                window.dispatch_action(Box::new(allow.clone()), cx)
                            }),
                    )
                    .child(Button::new(("deny", n)).small().label("Deny").on_click(
                        move |_, window, cx| window.dispatch_action(Box::new(deny.clone()), cx),
                    ))
                    .into_any_element()
            })
            .collect();
        let approved: Vec<_> = if remote.approved.is_empty() {
            vec![
                div()
                    .text_color(theme.muted_foreground)
                    .child("No devices allowed yet.")
                    .into_any_element(),
            ]
        } else {
            remote
                .approved
                .iter()
                .enumerate()
                .map(|(n, d)| {
                    let revoke = RevokeDevice {
                        node_id: d.node_id.clone(),
                        key_hash: d.key_hash.clone(),
                    };
                    h_flex()
                        .id(("approved", n))
                        .gap_2()
                        .py_1()
                        .border_b_1()
                        .border_color(theme.border)
                        .child(div().flex_1().min_w_0().child(d.name.clone()))
                        .child(div().text_color(theme.muted_foreground).child(format!(
                            "{} · code {}",
                            d.user,
                            skyelab_protocol::device_code(&d.key_hash)
                        )))
                        .child(
                            Button::new(("revoke", n))
                                .small()
                                .danger()
                                .label("Revoke")
                                .on_click(move |_, window, cx| {
                                    window.dispatch_action(Box::new(revoke.clone()), cx)
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
                        "Only your own untagged devices on your tailnet can connect, over TLS with this \
                         machine's Tailscale certificate (needs MagicDNS and HTTPS Certificates on in the \
                         tailnet's admin console), and only after you allow each one here. They can list \
                         projects and threads and run agents here, under each folder's bound account.",
                    )),
            )
            .when(!waiting.is_empty(), |el| {
                el.child(
                    v_flex()
                        .gap_1()
                        .child(div().text_lg().child("Waiting for approval"))
                        .child(div().text_xs().text_color(theme.muted_foreground).child(
                            "Allow a device only if it shows the same code. Another program on one of your \
                             devices can ask too.",
                        ))
                        .children(waiting),
                )
            })
            .child(
                v_flex()
                    .gap_1()
                    .child(div().text_lg().child("Allowed devices"))
                    .children(
                        remote
                            .approval_error
                            .clone()
                            .map(|e| div().text_color(theme.danger).child(e)),
                    )
                    .children(approved),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(div().text_lg().child("Connected devices"))
                    .children(peers),
            )
    }
}
