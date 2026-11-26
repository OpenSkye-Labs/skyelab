//! Settings → Machines (T3's Connections settings): whether this machine is shared, every machine you can drive with
//! its status and Connect / Disconnect / Forget, and adding one by its MagicDNS name. It reads the machine menu's
//! list (`EnvPicker`) and dispatches the same actions, so what connects and how a machine is trusted don't change.

use super::env_mode::SwitchEnvironment;
use super::env_picker::{ConnectTyped, EnvPicker, ForgetEnvironment, MachineKind, machine_icon};
use crate::remote::{Remote, ToggleSharing};
use crate::ui;
use crate::ui::{Button, Switch, TextField};
use gpui::{Context, Entity, IntoElement, SharedString, Subscription, Window, div, prelude::*};
use gpui_component::{ActiveTheme as _, Icon, h_flex, v_flex};

pub struct MachinesPage {
    picker: Entity<EnvPicker>,
    /// This machine's sharing; `None` without a tailnet hook.
    remote: Option<Entity<Remote>>,
    _observe: Vec<Subscription>,
}

impl MachinesPage {
    pub fn new(
        picker: Entity<EnvPicker>,
        remote: Option<Entity<Remote>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut observe = vec![cx.observe(&picker, |_, _, cx| cx.notify())];
        observe.extend(remote.iter().map(|r| cx.observe(r, |_, _, cx| cx.notify())));
        picker.update(cx, |p, cx| p.load_peers(cx));
        Self {
            picker,
            remote,
            _observe: observe,
        }
    }
}

impl Render for MachinesPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _render = crate::debug_renders::count("MachinesPage");
        let picker = self.picker.read(cx);
        let machines = picker.machines(cx);
        let recents = picker.recents.clone();
        let (typed, error, peers_error) = (
            picker.typed_input().clone(),
            picker.error.clone(),
            picker.peers_error.clone(),
        );
        let theme = cx.theme();
        let muted = |text: SharedString| {
            div()
                .text_size(ui::META)
                .text_color(theme.muted_foreground)
                .child(text)
        };
        let sharing = self.remote.as_ref().map(|r| {
            let r = r.read(cx);
            ui::group_row(true, theme)
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .child(
                            h_flex()
                                .gap_2()
                                .child(div().font_weight(gpui::FontWeight::MEDIUM).child(
                                    format!("Share {} on my tailnet", EnvPicker::this_machine()),
                                ))
                                .child(ui::Pill::new(r.status.label()).color(r.status.color(theme))),
                        )
                        .child(muted(
                            "Lets your other devices drive this machine. Who may connect is in Sharing & access."
                                .into(),
                        )),
                )
                .child(
                    Switch::new("machines-share")
                        .checked(r.enabled)
                        .label(if r.enabled { "On" } else { "Off" })
                        .accessibility_label("Share this machine on my tailnet")
                        .on_click(|_, window, cx| {
                            crate::workspace::dispatch(Box::new(ToggleSharing), window, cx)
                        }),
                )
        });
        let rows = machines.into_iter().enumerate().map(|(n, m)| {
            let id = SharedString::from(format!("machine-{}", m.label));
            let status: SharedString = match m.kind {
                MachineKind::This if m.current => "This machine · shown now".into(),
                MachineKind::This => "This machine".into(),
                _ if m.current => m.status.1.clone(),
                MachineKind::Device => m.status.1.clone(),
                MachineKind::Recent => "Recent · not among your online devices".into(),
            };
            let host = m.host.clone();
            let button = |id: &str, label: &'static str| {
                Button::new(SharedString::from(format!("{id}-{}", m.label)))
                    .small()
                    .ghost()
                    .label(label)
            };
            let switch_to = |target: Option<String>| {
                move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut gpui::App| {
                    crate::workspace::dispatch(
                        Box::new(SwitchEnvironment {
                            host: target.clone(),
                        }),
                        window,
                        cx,
                    )
                }
            };
            ui::group_row(n == 0, theme)
                .id(id)
                .child(
                    Icon::new(machine_icon(m.kind))
                        .size_4()
                        .text_color(theme.muted_foreground),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_0p5()
                        .child(div().min_w_0().truncate().child(m.label.clone()))
                        .child(
                            h_flex()
                                .gap_1p5()
                                .child(ui::StatusDot::new(m.status.0))
                                .child(muted(status).min_w_0().truncate()),
                        ),
                )
                .map(|row| match (&host, m.current) {
                    // On another machine now: Disconnect goes back to this one.
                    (Some(_), true) => row.child(
                        button("machine-disconnect", "Disconnect").on_click(switch_to(None)),
                    ),
                    (Some(_), false) => row.child(
                        button("machine-connect", "Connect")
                            .outline()
                            .on_click(switch_to(host.clone())),
                    ),
                    (None, false) => row
                        .child(button("machine-connect", "Switch back").on_click(switch_to(None))),
                    (None, true) => row,
                })
                // A recent name can also be online now, listed among your devices.
                .when(
                    !m.current
                        && host
                            .as_ref()
                            .is_some_and(|h| recents.iter().any(|r| r.eq_ignore_ascii_case(h))),
                    |row| {
                        let host = host.clone().unwrap_or_default();
                        row.child(button("machine-forget", "Forget").on_click(
                            move |_, window, cx| {
                                crate::workspace::dispatch(
                                    Box::new(ForgetEnvironment { host: host.clone() }),
                                    window,
                                    cx,
                                )
                            },
                        ))
                    },
                )
        });
        let add = ui::group_row(true, theme)
            .items_start()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_2()
                    .child(muted(
                        "A machine of yours on this tailnet that shares SkyeLab, such as laptop.tail1234.ts.net."
                            .into(),
                    ))
                    .child(
                        h_flex()
                            .debug_selector(|| "machines-add".into())
                            .w_full()
                            .gap_2()
                            .child(div().flex_1().min_w_0().child(TextField::new(&typed)))
                            .child(
                                Button::new("machines-connect-typed")
                                    .primary()
                                    .label("Connect")
                                    .on_click(|_, window, cx| {
                                        crate::workspace::dispatch(Box::new(ConnectTyped), window, cx)
                                    }),
                            ),
                    )
                    .children(error.map(|e| {
                        div()
                            .text_size(ui::META)
                            .text_color(ui::ink(theme, theme.danger))
                            .child(e)
                    })),
            );
        let section = |title: &'static str| ui::section_title(title, theme);
        div()
            .id("machines")
            .size_full()
            .overflow_y_scroll()
            .bg(theme.background)
            .child(
                v_flex()
                    .w_full()
                    .max_w(ui::SETTINGS_MAX_W)
                    .mx_auto()
                    .px_8()
                    .pt_7()
                    .pb_12()
                    .gap_8()
                    .text_size(ui::BODY)
                    .child(
                        v_flex()
                            .gap_1()
                            .child(ui::page_title("Machines", theme))
                            .child(div().text_color(theme.muted_foreground).child(
                                "Drive your other machines from this window. The composer's machine chip switches \
                                 between them.",
                            )),
                    )
                    .children(sharing.map(|row| {
                        v_flex()
                            .gap_2p5()
                            .child(section("This machine"))
                            .child(ui::group(theme).child(row))
                    }))
                    .child(
                        v_flex()
                            .gap_2p5()
                            .child(
                                h_flex()
                                    .justify_between()
                                    .child(section("Your machines"))
                                    .child(
                                        Button::new("machines-refresh")
                                            .xsmall()
                                            .ghost()
                                            .label("Refresh")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.picker.update(cx, |p, cx| p.load_peers(cx))
                                            })),
                                    ),
                            )
                            .child(ui::group(theme).children(rows))
                            .children(peers_error.map(|e| muted(e).px_1())),
                    )
                    .child(
                        v_flex()
                            .gap_2p5()
                            .child(section("Add by MagicDNS name"))
                            .child(ui::group(theme).child(add)),
                    ),
            )
    }
}
