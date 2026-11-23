//! The environment switcher at the top of the sidebar: "This Mac", your online tailnet devices, recent ones,
//! and a name typed by hand. Picking one dispatches `SwitchEnvironment`; the workspace does the switching.

use super::env_mode::SwitchEnvironment;
use crate::ui;
use gpui::{
    Action as _, AppContext as _, Context, Entity, FontWeight, IntoElement, SharedString, Task,
    Window, actions, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
    tooltip::Tooltip,
    v_flex,
};
use skyelab_daemon::SharedTailnet;
use skyelab_tailnet::own_online_peers;

actions!(skyelab, [ToggleEnvironments, ConnectTyped]);

pub const THIS_MAC: &str = "This Mac";

pub struct EnvPicker {
    pub open: bool,
    /// The remote host in use; `None` is this machine.
    pub current: Option<String>,
    /// MagicDNS names of your online devices.
    pub peers: Vec<String>,
    pub recents: Vec<String>,
    /// Why the device list is empty (Tailscale off, not installed).
    pub peers_error: Option<SharedString>,
    /// Why the last pick, connection start or recents save failed.
    pub error: Option<SharedString>,
    typed: Entity<InputState>,
    tailnet: SharedTailnet,
    _load: Option<Task<()>>,
    _typed: gpui::Subscription,
}

impl EnvPicker {
    pub fn new(tailnet: SharedTailnet, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let typed = cx.new(|cx| InputState::new(window, cx).placeholder("machine.tailnet.ts.net"));
        let _typed = cx.subscribe_in(&typed, window, |_, _, e: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { .. } = e {
                window.dispatch_action(Box::new(ConnectTyped), cx);
            }
        });
        Self {
            open: false,
            current: None,
            peers: Vec::new(),
            recents: Vec::new(),
            peers_error: None,
            error: None,
            typed,
            tailnet,
            _load: None,
            _typed,
        }
    }

    #[cfg(test)]
    /// The rows the list offers, in order: this machine, your devices, then recent names not already listed.
    pub fn entries(&self) -> Vec<String> {
        let mut rows = vec![THIS_MAC.to_owned()];
        rows.extend(self.peers.iter().cloned());
        for r in &self.recents {
            if !rows.iter().any(|x| x.eq_ignore_ascii_case(r)) {
                rows.push(r.clone());
            }
        }
        rows
    }

    pub fn toggle(&mut self, cx: &mut Context<Self>) {
        self.open = !self.open;
        if self.open {
            self.load_peers(cx);
        }
        cx.notify();
    }

    /// `tailscale status` shells out and can take seconds, so it runs off the UI thread.
    fn load_peers(&mut self, cx: &mut Context<Self>) {
        let tailnet = self.tailnet.clone();
        let task = cx.background_spawn(async move {
            let me = tailnet.self_node()?;
            let peers = own_online_peers(&me, tailnet.peers()?);
            anyhow::Ok(
                peers
                    .iter()
                    .map(|p| p.magic_dns_name().to_owned())
                    .collect(),
            )
        });
        self._load = Some(cx.spawn(async move |this, cx| {
            let loaded: anyhow::Result<Vec<String>> = task.await;
            let _ = this.update(cx, |this, cx| {
                match loaded {
                    Ok(peers) => (this.peers, this.peers_error) = (peers, None),
                    Err(e) => this.peers_error = Some(format!("{e:#}").into()),
                }
                cx.notify();
            });
        }));
    }

    pub fn typed(&self, cx: &gpui::App) -> String {
        self.typed.read(cx).value().trim().to_owned()
    }

    pub fn set_typed(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.typed
            .update(cx, |t, cx| t.set_value(text.to_owned(), window, cx));
    }

    pub fn clear_typed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_typed("", window, cx);
    }
}

impl Render for EnvPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let current: SharedString = self.current.clone().unwrap_or(THIS_MAC.into()).into();
        // One line, so it fits the window's top row beside the traffic lights.
        let header = h_flex()
            .id("env-picker")
            .h(px(26.))
            .justify_between()
            .gap_1p5()
            .px_2()
            .rounded(ui::RADIUS)
            .cursor_pointer()
            .tooltip(|window, cx| {
                Tooltip::new("SkyeLab on this machine or another; click to switch")
                    .build(window, cx)
            })
            .hover(|s| s.bg(theme.sidebar_accent.opacity(0.7)))
            .on_click(|_, window, cx| window.dispatch_action(Box::new(ToggleEnvironments), cx))
            .child(
                h_flex()
                    .min_w_0()
                    .gap_1p5()
                    .child(ui::brand_mark(px(16.), theme.sidebar_foreground))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(ui::BODY)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(current),
                    ),
            )
            .child(
                Icon::new(IconName::ChevronsUpDown)
                    .size_3p5()
                    .text_color(theme.muted_foreground),
            );
        let row = |host: Option<String>, label: String, selected: bool| {
            let action = SwitchEnvironment { host };
            ui::row(SharedString::from(format!("env-{label}")), selected, theme)
                .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx))
                .child(div().min_w_0().truncate().child(label))
        };
        let caption = |text: &'static str| {
            div()
                .px_2p5()
                .pt_2()
                .pb_0p5()
                .text_size(ui::META)
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.muted_foreground)
                .child(text)
        };
        let is = |h: &str| {
            self.current
                .as_deref()
                .is_some_and(|c| c.eq_ignore_ascii_case(h))
        };
        let recents: Vec<_> = self
            .recents
            .iter()
            .filter(|r| !self.peers.iter().any(|p| p.eq_ignore_ascii_case(r)))
            .collect();
        let list = v_flex()
            .gap_0p5()
            .p_1()
            .rounded(ui::RADIUS)
            .bg(theme.popover)
            .border_1()
            .border_color(theme.border)
            .child(row(None, THIS_MAC.into(), self.current.is_none()))
            .child(caption("Your devices"))
            .children(
                self.peers
                    .iter()
                    .map(|p| row(Some(p.clone()), p.clone(), is(p))),
            )
            .when(self.peers.is_empty(), |l| {
                l.child(
                    div()
                        .px_2p5()
                        .text_size(ui::META)
                        .text_color(theme.muted_foreground)
                        .child(
                            self.peers_error
                                .clone()
                                .unwrap_or("No other devices online".into()),
                        ),
                )
            })
            .when(!recents.is_empty(), |l| {
                l.child(caption("Recent")).children(
                    recents
                        .into_iter()
                        .map(|r| row(Some(r.clone()), r.clone(), is(r))),
                )
            })
            .child(caption("Add by MagicDNS name"))
            .child(
                h_flex()
                    .gap_1()
                    .px_1()
                    .child(div().flex_1().child(Input::new(&self.typed).small()))
                    .child(
                        Button::new("env-connect")
                            .small()
                            .primary()
                            .label("Connect")
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(ConnectTyped), cx)
                            }),
                    ),
            )
            .children(self.error.clone().map(|e| {
                div()
                    .px_2p5()
                    .text_size(ui::META)
                    .text_color(crate::ui::ink(theme, theme.danger))
                    .child(e)
            }));
        v_flex()
            .gap_1()
            .child(header)
            .when(self.open, |this| this.child(list))
    }
}
