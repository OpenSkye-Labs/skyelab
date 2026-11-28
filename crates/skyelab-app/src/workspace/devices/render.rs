//! Drawing the Devices tab: platform sections, device rows, inline URL row and screenshot.

use super::super::device_agent::DeviceClaims;
use super::{DevicesPanel, SectionView, badge};
use crate::ui;
use crate::ui::A11yScrollArea as _;
use crate::ui::ActiveTheme as _;
use crate::ui::Button;
use crate::ui::TextField;
use crate::ui::{h_flex, v_flex};
use gpui::{Context, ObjectFit, SharedString, Window, div, img, prelude::*, px};
use skyelab_devices::{Device, DeviceKind, DeviceState, Holder, Platform};

impl DevicesPanel {
    fn render_device(
        &self,
        device: &Device,
        first: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let holder = self.holder_row(device, cx);
        let theme = cx.theme();
        let ops = self.ops.get(&device.id);
        let busy = ops.and_then(|o| o.busy);
        let state = ui::Pill::new(badge(device.state));
        let state = match device.state {
            DeviceState::Booted => state.color(theme.success),
            DeviceState::Booting => state.color(theme.info),
            DeviceState::Shutdown => state,
            DeviceState::Offline => state.color(theme.warning),
        };
        let emulated = device.kind != DeviceKind::Physical;
        let booted = device.state == DeviceState::Booted;
        let button = |name: &str, label: &'static str| {
            Button::new(SharedString::from(format!("{name}-{}", device.id)))
                .xsmall()
                .ghost()
                .label(label)
                .disabled(busy.is_some())
        };
        let on = |f: fn(&mut Self, &Device, &mut Window, &mut Context<Self>)| {
            let device = device.clone();
            cx.listener(move |this, _, window, cx| f(this, &device, window, cx))
        };
        // On a line of their own under the name, so a narrow panel never cuts the device's name.
        let mut actions: Vec<gpui::AnyElement> = Vec::new();
        if emulated && device.state == DeviceState::Shutdown {
            actions.push(
                button("boot", "Boot")
                    .on_click(on(|t, d, _, cx| t.boot(d, cx)))
                    .into_any_element(),
            );
        }
        if emulated && matches!(device.state, DeviceState::Booted | DeviceState::Booting) {
            actions.push(
                button("shutdown", "Shutdown")
                    .on_click(on(|t, d, _, cx| t.shutdown(d, cx)))
                    .into_any_element(),
            );
        }
        if booted {
            actions.extend([
                button("shot", "Screenshot")
                    .on_click(on(|t, d, _, cx| t.screenshot(d, cx)))
                    .into_any_element(),
                button("url", "Open URL…")
                    .on_click(on(|t, d, w, cx| t.show_url_row(&d.id, w, cx)))
                    .into_any_element(),
                button("install", "Install app…")
                    .on_click(on(|t, d, w, cx| t.pick_app(d, w, cx)))
                    .into_any_element(),
            ]);
        }
        let url_row = (self.url_for.as_deref() == Some(device.id.as_str())).then(|| {
            h_flex()
                .gap_2()
                .child(
                    div().flex_1().child(
                        TextField::new(&self.url_input)
                            .small()
                            .accessibility_label("Deep link URL"),
                    ),
                )
                .child(
                    Button::new(SharedString::from(format!("url-open-{}", device.id)))
                        .small()
                        .primary()
                        .label("Open")
                        .disabled(busy.is_some())
                        .on_click(cx.listener(|this, _, _, cx| this.submit_url(cx))),
                )
                .child(
                    Button::new(SharedString::from(format!("url-cancel-{}", device.id)))
                        .small()
                        .ghost()
                        .label("Cancel")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.url_for = None;
                            cx.notify();
                        })),
                )
        });
        let theme = theme.clone();
        v_flex()
            .id(SharedString::from(format!("device-{}", device.id)))
            .gap_2()
            .px_4()
            .py_2p5()
            .when(!first, |d| d.border_t_1().border_color(theme.border))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(ui::BODY)
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(theme.foreground)
                            .child(device.name.clone()),
                    )
                    .children(device.os_version.clone().map(|v| {
                        div()
                            .flex_shrink_0()
                            .text_size(ui::META)
                            .text_color(theme.muted_foreground)
                            .child(v)
                    }))
                    .child(state)
                    .child(div().flex_1())
                    .children(busy.map(|label| {
                        h_flex()
                            .gap_1()
                            .text_size(ui::META)
                            .text_color(theme.muted_foreground)
                            .child(crate::ui::Spinner::new().size(px(12.)))
                            .child(label)
                    })),
            )
            .when(!actions.is_empty(), |d| {
                d.child(h_flex().flex_wrap().gap_1().children(actions))
            })
            .children(holder)
            .children(url_row)
            .children(ops.and_then(|o| o.error.clone()).map(|e| {
                div()
                    .text_size(ui::META)
                    .text_color(crate::ui::ink(&theme, theme.danger))
                    .child(e)
            }))
            .children(ops.and_then(|o| o.screenshot.clone()).map(|shot| {
                img(shot)
                    .w_full()
                    .h(px(420.))
                    .object_fit(ObjectFit::Contain)
            }))
    }

    /// "Used by <thread>" with Take back, or "Taken back" with Let agents use, when an agent claimed the device.
    fn holder_row(
        &self,
        device: &Device,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let claims = cx.try_global::<DeviceClaims>()?;
        let (text, label, back): (SharedString, &'static str, bool) =
            match claims.claims.holder(&device.id)? {
                Holder::Thread(t) => {
                    let title = claims.titles.get(&t).cloned();
                    let title = title.unwrap_or_else(|| format!("thread {t}").into());
                    (format!("Used by {title}").into(), "Take back", true)
                }
                Holder::User => ("Taken back from agents".into(), "Let agents use", false),
            };
        let theme = cx.theme();
        let id = device.id.clone();
        Some(
            h_flex()
                .gap_2()
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(ui::META)
                        .text_color(if back {
                            ui::ink(theme, theme.info)
                        } else {
                            theme.muted_foreground
                        })
                        .child(text),
                )
                .child(
                    Button::new(SharedString::from(format!("take-back-{}", device.id)))
                        .xsmall()
                        .ghost()
                        .label(label)
                        .on_click(
                            cx.listener(move |_, _, _, cx| DeviceClaims::take_back(&id, back, cx)),
                        ),
                ),
        )
    }

    fn render_section(&self, platform: Platform, cx: &mut Context<Self>) -> impl IntoElement {
        // Copied out: the device rows below need `cx` mutably.
        let (muted, danger) = (cx.theme().muted_foreground, cx.theme().danger);
        let note = |text: SharedString, color| {
            div()
                .px_4()
                .py_3()
                .text_size(ui::BODY)
                .text_color(color)
                .child(text)
        };
        let body = match self.section(platform) {
            SectionView::Loading => note("Looking for devices…".into(), muted).into_any_element(),
            SectionView::Missing(text) => note(text.into(), muted).into_any_element(),
            SectionView::Failed(e) => note(e.into(), danger).into_any_element(),
            SectionView::Devices([]) => note("No devices".into(), muted).into_any_element(),
            SectionView::Devices(devices) => v_flex()
                .children(
                    devices
                        .iter()
                        .enumerate()
                        .map(|(i, d)| self.render_device(d, i == 0, cx).into_any_element())
                        .collect::<Vec<_>>(),
                )
                .into_any_element(),
        };
        let theme = cx.theme();
        v_flex()
            .gap_2p5()
            .child(ui::section_title(platform.to_string(), theme))
            .child(ui::group(theme).child(body))
    }
}

impl Render for DevicesPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _render = crate::debug_renders::count("DevicesPanel");
        self.rendered = true;
        self.ensure_polling(cx);
        let theme = cx.theme();
        let refresh = Button::new("devices-refresh")
            .small()
            .ghost()
            .label("Refresh")
            .loading(self.refreshing.is_some())
            .on_click(cx.listener(|this, _, _, cx| this.refresh(cx)));
        div()
            .id("devices")
            .key_context(super::CONTEXT)
            .track_focus(&self.focus)
            // Esc in the Open URL field closes that row; only Esc elsewhere hides the tab.
            .on_action(cx.listener(|this, _: &super::HideDevices, window, cx| {
                if this.url_for.take().is_some() {
                    this.focus.focus(window, cx);
                    cx.notify();
                } else {
                    cx.propagate();
                }
            }))
            .size_full()
            .overflow_y_scroll()
            .a11y_scroll_area()
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
                    .child(
                        h_flex()
                            .justify_between()
                            .child(ui::page_title("Devices", theme))
                            .child(refresh.icon(crate::ui::SkyeIcon::RefreshCw)),
                    )
                    .child(self.render_section(Platform::Ios, cx))
                    .child(self.render_section(Platform::Android, cx)),
            )
    }
}
