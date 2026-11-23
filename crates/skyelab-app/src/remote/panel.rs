//! The Remote tab. Its buttons dispatch actions the Workspace handles, so SKYELAB_VERIFY_ACTIONS can drive them.

use super::{AllowDevice, DenyDevice, DisconnectPeer, Remote, RevokeDevice, ToggleSharing};
use crate::ui;
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
                ui::group_row(true, theme)
                    .child(
                        ui::EmptyState::new("No devices connected")
                            .hint("A device you allowed shows here while it is connected."),
                    )
                    .into_any_element(),
            ]
        } else {
            remote
                .peers
                .iter()
                .enumerate()
                .map(|(n, p)| {
                    let id = p.id;
                    ui::group_row(n == 0, theme)
                        .id(("peer", id))
                        .child(div().flex_1().min_w_0().child(p.device.clone()))
                        .child(
                            div()
                                .text_color(theme.muted_foreground)
                                .child(format!("{} · {}", p.user, p.addr)),
                        )
                        .child(
                            Button::new(("disconnect", id))
                                .small()
                                .ghost()
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
                ui::group_row(n == 0, theme)
                    .id(("request", n))
                    .child(ui::StatusDot::new(theme.warning))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(Remote::request_label(r)),
                    )
                    .child(ui::mono(format!("code {}", r.code), theme))
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
                ui::group_row(true, theme)
                    .child(ui::EmptyState::new("No devices allowed yet").hint(
                        "Turn sharing on, then connect from your other device: its request shows up above for you to allow.",
                    ))
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
                    ui::group_row(n == 0, theme)
                        .id(("approved", n))
                        .child(div().flex_1().min_w_0().truncate().child(d.name.clone()))
                        .child(ui::mono(
                            format!(
                                "{} · code {}",
                                d.user,
                                skyelab_protocol::device_code(&d.key_hash)
                            ),
                            theme,
                        ))
                        .child(
                            Button::new(("revoke", n))
                                .small()
                                .ghost()
                                .label("Revoke")
                                .on_click(move |_, window, cx| {
                                    window.dispatch_action(Box::new(revoke.clone()), cx)
                                }),
                        )
                        .into_any_element()
                })
                .collect()
        };
        let section = |title: &'static str, rows: Vec<gpui::AnyElement>| {
            v_flex()
                .gap_2p5()
                .child(ui::section_title(title, theme))
                .child(ui::group(theme).children(rows))
        };
        div()
            .id("remote")
            .key_context("Remote")
            .track_focus(&self.focus)
            .size_full()
            .overflow_y_scroll()
            .bg(theme.background)
            .child(
                v_flex()
                    .w_full()
                    .max_w(ui::PAGE_MAX_W)
                    .mx_auto()
                    .px_6()
                    .pt_6()
                    .pb_12()
                    .gap_8()
                    .text_size(ui::BODY)
                    .child(
                        v_flex()
                            .gap_1()
                            .child(ui::page_title("Remote access", theme))
                            .child(div().text_color(theme.muted_foreground).child(
                                "Use this machine's projects and agents from your other devices.",
                            )),
                    )
                    .child(
                        ui::group(theme).child(
                            ui::group_row(true, theme)
                                .items_start()
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .min_w_0()
                                        .gap_1()
                                        .child(
                                            h_flex()
                                                .gap_2()
                                                .child(
                                                    div()
                                                        .font_weight(gpui::FontWeight::MEDIUM)
                                                        .child("Share this machine on my tailnet"),
                                                )
                                                .child(ui::Pill::new(status).color(status_color)),
                                        )
                                        .child(div().text_size(ui::META).text_color(theme.muted_foreground).child(
                                            "Only your own untagged devices on your tailnet can connect, over TLS with this \
                                             machine's Tailscale certificate (needs MagicDNS and HTTPS Certificates on in the \
                                             tailnet's admin console), and only after you allow each one here. They can list \
                                             projects and threads and run agents here, under each folder's bound account.",
                                        )),
                                )
                                .child(
                                    Switch::new("share-on-tailnet")
                                        .checked(remote.enabled)
                                        .accessibility_label("Share this machine on my tailnet")
                                        .on_click(|_, window, cx| {
                                            window.dispatch_action(Box::new(ToggleSharing), cx)
                                        }),
                                ),
                        ),
                    )
                    .when(!waiting.is_empty(), |el| {
                        el.child(
                            v_flex()
                                .gap_2p5()
                                .child(ui::section_title("Waiting for approval", theme))
                                .child(div().px_1().text_size(ui::META).text_color(theme.muted_foreground).child(
                                    "Allow a device only if it shows the same code. Another program on one of your \
                                     devices can ask too.",
                                ))
                                .child(ui::group(theme).border_color(theme.warning.opacity(0.4)).children(waiting)),
                        )
                    })
                    .child(
                        section("Allowed devices", approved).children(
                            remote
                                .approval_error
                                .clone()
                                .map(|e| div().px_1().text_color(crate::ui::ink(theme, theme.danger)).child(e)),
                        ),
                    )
                    .child(section("Connected devices", peers)),
            )
    }
}
