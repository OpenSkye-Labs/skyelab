//! Settings → About (T3's About): this build's version and the update check's switch.

use super::{Settings, widgets};
use crate::ui;
use crate::updates::{self, UpdateStatus};
use gpui::{AnyElement, Context, div, prelude::*};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _, button::Button, h_flex, switch::Switch,
    v_flex,
};

impl Settings {
    pub(super) fn render_about(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let u = cx.try_global::<updates::Updates>();
        let enabled = u.is_none_or(|u| u.enabled);
        let status = u.map_or(UpdateStatus::Idle, |u| u.status.clone());
        let said = match &status {
            UpdateStatus::Idle => None,
            UpdateStatus::Checking => Some("Checking…".to_owned()),
            UpdateStatus::UpToDate => Some("SkyeLab is up to date.".to_owned()),
            UpdateStatus::Available { tag, .. } => Some(format!(
                "SkyeLab {} is available.",
                skyelab_core::update::display_version(tag)
            )),
            UpdateStatus::Failed(e) => Some(format!("Couldn't check: {e}")),
        };
        let text = |title: String, about: String| {
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(
                    div()
                        .text_size(ui::BODY)
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .child(title),
                )
                .child(div().text_size(ui::META).text_color(muted).child(about))
        };
        let version = widgets::row(true, cx)
            .flex()
            .items_center()
            .gap_4()
            .child(text(
                format!("SkyeLab {}", env!("CARGO_PKG_VERSION")),
                said.unwrap_or_else(|| "Releases come from GitHub.".to_owned()),
            ))
            .child(
                Button::new("about-check")
                    .outline()
                    .small()
                    .label("Check now")
                    .disabled(status == UpdateStatus::Checking)
                    .on_click(|_, _, cx| updates::check(true, cx)),
            )
            .into_any_element();
        let switch = widgets::row(false, cx)
            .flex()
            .items_center()
            .gap_4()
            .child(text(
                "Check for updates".to_owned(),
                "On start, at most once a day, SkyeLab asks GitHub for its latest release. Nothing is sent but \
                 the request, and nothing installs: Download opens the release page."
                    .to_owned(),
            ))
            .child(
                Switch::new("about-updates")
                    .checked(enabled)
                    .accessibility_label("Check for updates")
                    .on_click(cx.listener(|_, on: &bool, _, cx| {
                        updates::set_enabled(*on, cx).detach_and_log_err(cx);
                        cx.notify();
                    })),
            )
            .into_any_element();
        h_flex()
            .w_full()
            .child(widgets::section("About", [version, switch], cx).w_full())
            .into_any_element()
    }
}
