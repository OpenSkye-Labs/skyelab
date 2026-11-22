//! Drawing the Devices tab: platform sections, device rows, inline URL row and screenshot.

use super::{DevicesPanel, SectionView, badge};
use gpui::{Context, ObjectFit, SharedString, Window, div, img, prelude::*, px};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::Input,
    spinner::Spinner,
    tag::Tag,
    v_flex,
};
use skyelab_devices::{Device, DeviceKind, DeviceState, Platform};

impl DevicesPanel {
    fn render_device(&self, device: &Device, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let ops = self.ops.get(&device.id);
        let busy = ops.and_then(|o| o.busy);
        let tag = match device.state {
            DeviceState::Booted => Tag::success(),
            DeviceState::Booting => Tag::info(),
            DeviceState::Shutdown => Tag::secondary(),
            DeviceState::Offline => Tag::warning(),
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
        let mut actions = h_flex().gap_1();
        if emulated && device.state == DeviceState::Shutdown {
            actions =
                actions.child(button("boot", "Boot").on_click(on(|t, d, _, cx| t.boot(d, cx))));
        }
        if emulated && matches!(device.state, DeviceState::Booted | DeviceState::Booting) {
            actions = actions.child(
                button("shutdown", "Shutdown").on_click(on(|t, d, _, cx| t.shutdown(d, cx))),
            );
        }
        if booted {
            actions = actions
                .child(button("shot", "Screenshot").on_click(on(|t, d, _, cx| t.screenshot(d, cx))))
                .child(
                    button("url", "Open URL…")
                        .on_click(on(|t, d, w, cx| t.show_url_row(&d.id, w, cx))),
                )
                .child(
                    button("install", "Install app…")
                        .on_click(on(|t, d, w, cx| t.pick_app(d, w, cx))),
                );
        }
        let url_row = (self.url_for.as_deref() == Some(device.id.as_str())).then(|| {
            h_flex()
                .gap_2()
                .child(div().flex_1().child(Input::new(&self.url_input).small()))
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
            .px_3()
            .py_2()
            .rounded_md()
            .border_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.foreground)
                            .child(device.name.clone()),
                    )
                    .children(
                        device
                            .os_version
                            .clone()
                            .map(|v| div().text_xs().text_color(theme.muted_foreground).child(v)),
                    )
                    .child(tag.xsmall().child(badge(device.state)))
                    .child(div().flex_1())
                    .children(busy.map(|label| {
                        h_flex()
                            .gap_1()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(Spinner::new().xsmall())
                            .child(label)
                    }))
                    .child(actions),
            )
            .children(url_row)
            .children(
                ops.and_then(|o| o.error.clone())
                    .map(|e| div().text_xs().text_color(theme.danger).child(e)),
            )
            .children(ops.and_then(|o| o.screenshot.clone()).map(|shot| {
                img(shot)
                    .w_full()
                    .h(px(420.))
                    .object_fit(ObjectFit::Contain)
            }))
    }

    fn render_section(&self, platform: Platform, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let note = |text: SharedString, color| {
            div()
                .px_3()
                .py_2()
                .rounded_md()
                .border_1()
                .border_color(theme.border)
                .text_sm()
                .text_color(color)
                .child(text)
        };
        let body = match self.section(platform) {
            SectionView::Loading => {
                note("Looking for devices…".into(), theme.muted_foreground).into_any_element()
            }
            SectionView::Missing(text) => {
                note(text.into(), theme.muted_foreground).into_any_element()
            }
            SectionView::Failed(e) => note(e.into(), theme.danger).into_any_element(),
            SectionView::Devices([]) => {
                note("No devices".into(), theme.muted_foreground).into_any_element()
            }
            SectionView::Devices(devices) => v_flex()
                .gap_2()
                .children(
                    devices
                        .iter()
                        .map(|d| self.render_device(d, cx).into_any_element())
                        .collect::<Vec<_>>(),
                )
                .into_any_element(),
        };
        v_flex()
            .gap_2()
            .child(
                div()
                    .text_xs()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(theme.muted_foreground)
                    .child(platform.to_string().to_uppercase()),
            )
            .child(body)
    }
}

impl Render for DevicesPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.rendered = true;
        self.ensure_polling(cx);
        let theme = cx.theme().clone();
        let refresh = Button::new("devices-refresh")
            .small()
            .ghost()
            .label("Refresh")
            .loading(self.refreshing.is_some())
            .on_click(cx.listener(|this, _, _, cx| this.refresh(cx)));
        v_flex()
            .id("devices")
            .track_focus(&self.focus)
            .size_full()
            .overflow_y_scroll()
            .p_4()
            .gap_5()
            .bg(theme.background)
            .child(
                h_flex()
                    .justify_between()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("Devices"),
                    )
                    .child(refresh),
            )
            .child(self.render_section(Platform::Ios, cx))
            .child(self.render_section(Platform::Android, cx))
    }
}
