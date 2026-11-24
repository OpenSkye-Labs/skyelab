//! The sidebar's update notice (T3's update pill at the bottom of the sidebar): a newer release, Download and
//! Skip this version. Shown only while `updates` has one.

use crate::ui;
use crate::ui::Button;
use crate::ui::SkyeIcon;
use gpui::{AnyElement, App, IntoElement, ParentElement as _, Styled as _, div};
use gpui_component::{ActiveTheme as _, Icon, h_flex, v_flex};

pub(super) fn notice(cx: &App) -> Option<AnyElement> {
    let (tag, _) = crate::updates::available(cx)?;
    let theme = cx.theme();
    let version = skyelab_core::update::display_version(&tag).to_owned();
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
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Icon::new(SkyeIcon::ArrowDown)
                            .size_4()
                            .text_color(ui::ink(theme, theme.info)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(ui::BODY)
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child(format!("SkyeLab {version} is available")),
                    ),
            )
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        Button::new("update-download")
                            .primary()
                            .xsmall()
                            .label("Download")
                            .on_click(|_, _, cx| crate::updates::download(cx)),
                    )
                    .child(
                        Button::new("update-skip")
                            .ghost()
                            .xsmall()
                            .label("Skip this version")
                            .on_click(|_, _, cx| crate::updates::skip(cx)),
                    ),
            )
            .into_any_element(),
    )
}
