//! The sidebar's update notice (T3's update pill at the bottom of the sidebar): a newer release, then Download and
//! install's progress, Restart to update, or what failed. Shown only while `updates` has one.

use crate::ui;
use crate::ui::Button;
use crate::ui::SkyeIcon;
use crate::updates::{Step, UpdateStatus, Updates};
use gpui::{AnyElement, App, IntoElement, ParentElement as _, Styled as _, div, px, relative};
use gpui_component::{ActiveTheme as _, Icon, h_flex, v_flex};

pub(super) fn notice(cx: &App) -> Option<AnyElement> {
    let (tag, step) = match &cx.try_global::<Updates>()?.status {
        UpdateStatus::Available { tag, .. } => (tag, None),
        UpdateStatus::Updating { tag, step, .. } => (tag, Some(step)),
        _ => return None,
    };
    let theme = cx.theme();
    let version = skyelab_core::update::display_version(tag);
    let (icon, color, title) = match step {
        None => (
            SkyeIcon::ArrowDown,
            theme.info,
            format!("SkyeLab {version} is available"),
        ),
        Some(Step::Downloading { done, total }) if total.is_some_and(|t| *done >= t) => (
            SkyeIcon::ArrowDown,
            theme.info,
            format!("Checking {version}…"),
        ),
        Some(Step::Downloading { .. }) => (
            SkyeIcon::ArrowDown,
            theme.info,
            format!("Downloading {version}"),
        ),
        Some(Step::Ready) => (
            SkyeIcon::CircleCheck,
            theme.success,
            format!("SkyeLab {version} is ready"),
        ),
        Some(Step::Failed(_)) => (
            SkyeIcon::CircleAlert,
            theme.danger,
            format!("Couldn't update to {version}"),
        ),
    };
    let percent = match step {
        Some(Step::Downloading {
            done,
            total: Some(t),
        }) => Some((*done as f32 / (*t).max(1) as f32).min(1.)),
        _ => None,
    };
    let head = h_flex()
        .gap_2()
        .items_center()
        .child(Icon::new(icon).size_4().text_color(ui::ink(theme, color)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(ui::BODY)
                .font_weight(gpui::FontWeight::MEDIUM)
                .child(title),
        )
        .children(percent.map(|p| {
            div()
                .text_size(ui::META)
                .text_color(theme.muted_foreground)
                .child(format!("{}%", (p * 100.) as u32))
        }));
    let body: AnyElement = match step {
        None => {
            let primary = if crate::updates::can_install(cx) {
                Button::new("update-install")
                    .primary()
                    .xsmall()
                    .label("Download and install")
                    .on_click(|_, _, cx| crate::updates::install(cx))
            } else {
                Button::new("update-download")
                    .primary()
                    .xsmall()
                    .label("Download")
                    .on_click(|_, _, cx| crate::updates::download(cx))
            };
            h_flex()
                .gap_1()
                .child(primary)
                .child(
                    Button::new("update-skip")
                        .ghost()
                        .xsmall()
                        .label("Skip this version")
                        .on_click(|_, _, cx| crate::updates::skip(cx)),
                )
                .into_any_element()
        }
        // A plain bar, not gpui-component's Progress: that animates every value change.
        Some(Step::Downloading { .. }) => div()
            .h(px(4.))
            .w_full()
            .rounded_full()
            .bg(theme.muted)
            .child(
                div()
                    .h_full()
                    .rounded_full()
                    .bg(ui::ink(theme, theme.info))
                    .w(relative(percent.unwrap_or(0.))),
            )
            .into_any_element(),
        Some(Step::Ready) => {
            let folder = crate::updates::staged_is_folder(cx);
            h_flex()
                .child(
                    Button::new("update-restart")
                        .primary()
                        .xsmall()
                        .label(if folder {
                            "Show in folder"
                        } else {
                            "Restart to update"
                        })
                        .on_click(|_, _, cx| crate::updates::restart(cx)),
                )
                .into_any_element()
        }
        Some(Step::Failed(e)) => v_flex()
            .gap_2()
            .child(
                div()
                    .text_size(ui::META)
                    .text_color(theme.muted_foreground)
                    .child(e.clone()),
            )
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        Button::new("update-retry")
                            .primary()
                            .xsmall()
                            .label("Try again")
                            .on_click(|_, _, cx| crate::updates::install(cx)),
                    )
                    .child(
                        Button::new("update-page")
                            .ghost()
                            .xsmall()
                            .label("Release page")
                            .on_click(|_, _, cx| crate::updates::download(cx)),
                    ),
            )
            .into_any_element(),
    };
    Some(
        v_flex()
            .mx_2()
            .mb_1()
            .p_2()
            .gap_2()
            .rounded(ui::RADIUS)
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
            .child(head)
            .child(body)
            .into_any_element(),
    )
}
