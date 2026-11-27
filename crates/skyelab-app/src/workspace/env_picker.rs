//! The machine menu (T3's environment selector): this machine, your online tailnet devices and recent ones. It opens
//! from the composer footer's machine chip, the status bar's Remote badge and the palette, as a floating card at the
//! point clicked. Adding a machine by name and forgetting one live in Settings → Machines (`machines_page.rs`).
//! Picking a row dispatches `SwitchEnvironment`; the workspace does the switching.

use super::env_mode::SwitchEnvironment;
use crate::environment::RemoteEnv;
use crate::ui;
use crate::ui::A11yScrollArea as _;
use crate::ui::ActiveTheme as _;
use crate::ui::SkyeIcon;
use crate::ui::Tooltip;
use gpui::{
    Action, AppContext as _, Context, Entity, FocusHandle, FontWeight, Hsla, IntoElement, Pixels,
    Point, SharedString, Task, Window, actions, div, prelude::*, px,
};
use gpui_component::{
    Icon,
    input::{InputEvent, InputState},
    v_flex,
};
use skyelab_daemon::SharedTailnet;
use skyelab_tailnet::own_online_peers;

actions!(skyelab, [ToggleEnvironments, ConnectTyped]);

/// Opens the machine menu just above and left of a window point (where the chip was clicked).
#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct ShowEnvironments {
    pub x: f32,
    pub y: f32,
}

impl ShowEnvironments {
    pub fn at(p: Point<Pixels>) -> Self {
        Self {
            x: f32::from(p.x),
            y: f32::from(p.y),
        }
    }
}

/// Drops a remembered name from the recents (Settings → Machines).
#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct ForgetEnvironment {
    pub host: String,
}

pub const THIS_MAC: &str = "This Mac";

/// The menu's width: roomy enough for a full MagicDNS name.
pub(super) const MENU_W: f32 = 340.;

/// Which list a machine row belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MachineKind {
    This,
    Device,
    Recent,
}

/// One row of the machine list, shared by the menu and Settings → Machines.
pub struct Machine {
    /// `None` is this machine.
    pub host: Option<String>,
    pub label: SharedString,
    pub kind: MachineKind,
    /// The window shows this machine now.
    pub current: bool,
    /// The status dot and what it means.
    pub status: (Hsla, SharedString),
}

pub struct EnvPicker {
    pub open: bool,
    /// Where the menu's bottom-left corner sits; `None` (the palette) puts it over the bottom of the window.
    pub anchor: Option<Point<Pixels>>,
    /// The remote host in use; `None` is this machine.
    pub current: Option<String>,
    /// The connection to `current`, for its status dot.
    pub(super) remote: Option<Entity<RemoteEnv>>,
    /// MagicDNS names of your online devices.
    pub peers: Vec<String>,
    pub recents: Vec<String>,
    /// Why the device list is empty (Tailscale off, not installed).
    pub peers_error: Option<SharedString>,
    /// Why the last pick, connection start or recents save failed.
    pub error: Option<SharedString>,
    typed: Entity<InputState>,
    tailnet: SharedTailnet,
    pub(super) focus: FocusHandle,
    /// Where the focus was before the menu opened; it goes back there on close.
    return_focus: Option<FocusHandle>,
    _load: Option<Task<()>>,
    _typed: gpui::Subscription,
}

impl EnvPicker {
    pub fn new(tailnet: SharedTailnet, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let typed = cx.new(|cx| InputState::new(window, cx).placeholder("machine.tailnet.ts.net"));
        let _typed = cx.subscribe_in(&typed, window, |_, _, e: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { .. } = e {
                crate::workspace::dispatch(Box::new(ConnectTyped), window, cx);
            }
        });
        Self {
            open: false,
            anchor: None,
            current: None,
            remote: None,
            peers: Vec::new(),
            recents: Vec::new(),
            peers_error: None,
            error: None,
            typed,
            tailnet,
            focus: cx.focus_handle(),
            return_focus: None,
            _load: None,
            _typed,
        }
    }

    /// This machine's name as people know it, or "This Mac".
    pub fn this_machine() -> SharedString {
        super::thread::composer::machine_name()
            .unwrap_or(THIS_MAC.into())
            .into()
    }

    /// Every row, in order: this machine, your online devices, then recent names not already listed.
    pub fn machines(&self, cx: &gpui::App) -> Vec<Machine> {
        let theme = cx.theme();
        let is = |h: &str| {
            self.current
                .as_deref()
                .is_some_and(|c| c.eq_ignore_ascii_case(h))
        };
        let status = |host: &str, online: bool| -> (Hsla, SharedString) {
            match &self.remote {
                Some(env) if is(host) => {
                    let state = &env.read(cx).state;
                    (state.color(theme), state.label())
                }
                _ if online => (theme.success, "Online".into()),
                _ => (
                    theme.muted_foreground.opacity(0.5),
                    "Not among your online devices".into(),
                ),
            }
        };
        let mut rows = vec![Machine {
            host: None,
            label: Self::this_machine(),
            kind: MachineKind::This,
            current: self.current.is_none(),
            status: (theme.success, "This machine".into()),
        }];
        rows.extend(self.peers.iter().map(|p| Machine {
            host: Some(p.clone()),
            label: p.clone().into(),
            kind: MachineKind::Device,
            current: is(p),
            status: status(p, true),
        }));
        for r in &self.recents {
            if !self.peers.iter().any(|p| p.eq_ignore_ascii_case(r)) {
                rows.push(Machine {
                    host: Some(r.clone()),
                    label: r.clone().into(),
                    kind: MachineKind::Recent,
                    current: is(r),
                    status: status(r, false),
                });
            }
        }
        rows
    }

    #[cfg(test)]
    /// The hosts the list offers, in order, this machine as "This Mac".
    pub fn entries(&self, cx: &gpui::App) -> Vec<String> {
        self.machines(cx)
            .into_iter()
            .map(|m| m.host.unwrap_or(THIS_MAC.into()))
            .collect()
    }

    /// Opens the menu at `anchor` (closing it when it is already open), and moves the focus into it so Escape closes it.
    pub fn toggle(
        &mut self,
        anchor: Option<Point<Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.open {
            return self.close(window, cx);
        }
        self.open = true;
        self.anchor = anchor;
        self.return_focus = window.focused(cx);
        self.focus.focus(window, cx);
        self.load_peers(cx);
        cx.notify();
    }

    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        self.open = false;
        if self.focus.contains_focused(window, cx)
            && let Some(back) = self.return_focus.take()
        {
            back.focus(window, cx);
        }
        cx.notify();
    }

    /// `tailscale status` shells out and can take seconds, so it runs off the UI thread.
    pub(super) fn load_peers(&mut self, cx: &mut Context<Self>) {
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

    pub(super) fn typed_input(&self) -> &Entity<InputState> {
        &self.typed
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

/// A list caption (T3's select group label).
fn caption(text: &'static str, cx: &gpui::App) -> gpui::Div {
    div()
        .px_2p5()
        .pt_2()
        .pb_0p5()
        .text_size(ui::META)
        .font_weight(FontWeight::MEDIUM)
        .text_color(cx.theme().muted_foreground)
        .child(text)
}

/// The icon a machine row leads with.
pub(super) fn machine_icon(kind: MachineKind) -> SkyeIcon {
    match kind {
        MachineKind::This => SkyeIcon::Monitor,
        MachineKind::Device | MachineKind::Recent => SkyeIcon::Remote,
    }
}

impl Render for EnvPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _render = crate::debug_renders::count("EnvPicker");
        let machines = self.machines(cx);
        let theme = cx.theme();
        let row = |m: Machine| {
            let action = SwitchEnvironment {
                host: m.host.clone(),
            };
            let tip: SharedString = match &m.host {
                Some(h) => format!("{h} · {}", m.status.1).into(),
                None => format!("{} · this machine", m.label).into(),
            };
            let row_label = format!("{} · {}", m.label, m.status.1);
            ui::row(
                SharedString::from(format!("env-{}", m.label)),
                m.current,
                theme,
            )
            .aria_label(row_label)
            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            .on_click(move |_, window, cx| {
                crate::workspace::dispatch(action.boxed_clone(), window, cx)
            })
            .child(
                Icon::new(machine_icon(m.kind))
                    .size_3p5()
                    .text_color(theme.muted_foreground),
            )
            .child(div().flex_1().min_w_0().truncate().child(m.label))
            .when(m.kind != MachineKind::This, |r| {
                r.child(ui::StatusDot::new(m.status.0))
            })
            .child(div().w(px(14.)).flex_shrink_0().when(m.current, |d| {
                d.child(Icon::new(SkyeIcon::Check).size_3p5())
            }))
        };
        let mut this = None;
        let (mut devices, mut recents) = (Vec::new(), Vec::new());
        for m in machines {
            match m.kind {
                MachineKind::This => this = Some(m),
                MachineKind::Device => devices.push(m),
                MachineKind::Recent => recents.push(m),
            }
        }
        let note = |text: SharedString| {
            div()
                .px_2p5()
                .py_1()
                .text_size(ui::META)
                .text_color(theme.muted_foreground)
                .child(text)
        };
        v_flex()
            .id("env-menu")
            .debug_selector(|| "env-menu".into())
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, window, cx| {
                if e.keystroke.key == "escape" {
                    this.close(window, cx);
                    cx.stop_propagation();
                }
            }))
            .w(px(MENU_W))
            .max_h(px(480.))
            .overflow_y_scroll()
            .a11y_scroll_area()
            .p_1()
            .gap_0p5()
            .rounded(ui::RADIUS_LG)
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .text_size(ui::BODY)
            .shadow_lg()
            // Presses inside never reach the backdrop that closes the menu.
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(caption("This machine", cx))
            .children(this.map(row))
            .child(caption("Your devices", cx))
            .when(devices.is_empty(), |l| {
                l.child(note(
                    self.peers_error
                        .clone()
                        .unwrap_or("No other devices online".into()),
                ))
            })
            .children(devices.into_iter().map(row))
            .when(!recents.is_empty(), |l| {
                l.child(caption("Recent", cx))
                    .children(recents.into_iter().map(row))
            })
            .children(
                self.error
                    .clone()
                    .map(|e| note(e).text_color(ui::ink(theme, theme.danger))),
            )
            .child(div().my_1().h(px(1.)).bg(theme.border))
            .child(
                ui::row("env-manage", false, theme)
                    .aria_label("Manage machines…")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.close(window, cx);
                        crate::workspace::dispatch(
                            Box::new(super::keys::OpenSettingsSection {
                                section: crate::settings::Section::Machines,
                            }),
                            window,
                            cx,
                        )
                    }))
                    .child(
                        Icon::new(SkyeIcon::Settings2)
                            .size_3p5()
                            .text_color(theme.muted_foreground),
                    )
                    .child("Manage machines…"),
            )
    }
}
