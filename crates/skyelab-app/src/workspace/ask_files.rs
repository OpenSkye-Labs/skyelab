//! A question card's attached files: the chips and Attach files under each question (T3's question attachments).

use super::thread_items::ItemsView;
use crate::ui;
use gpui::{Context, IntoElement, SharedString, div, prelude::*, px};
use gpui_component::{
    ActiveTheme as _, Icon, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
};

/// Question `qi`'s attached files as chips, and Attach files.
pub(super) fn answer_files<V: ItemsView>(
    id: &str,
    qi: usize,
    files: &[std::path::PathBuf],
    cx: &mut Context<V>,
) -> impl IntoElement {
    let theme = cx.theme();
    let chips = files.iter().enumerate().map(|(ix, path)| {
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let id = id.to_owned();
        h_flex()
            .h(px(24.))
            .max_w(px(220.))
            .pl_2()
            .gap_0p5()
            .rounded(ui::RADIUS)
            .border_1()
            .border_color(theme.border)
            .bg(theme.muted)
            .text_size(ui::META)
            .child(
                Icon::new(crate::ui::SkyeIcon::File)
                    .xsmall()
                    .text_color(theme.muted_foreground),
            )
            .child(div().min_w_0().truncate().child(name))
            .child(
                Button::new(SharedString::from(format!("unattach-{id}-{qi}-{ix}")))
                    .ghost()
                    .xsmall()
                    .icon(Icon::new(crate::ui::SkyeIcon::X))
                    .tooltip("Remove")
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.remove_answer_file(&id, qi, ix, cx)),
                    ),
            )
    });
    let attach_id = id.to_owned();
    h_flex().pl_7().flex_wrap().gap_1p5().children(chips).child(
        Button::new(SharedString::from(format!("attach-{id}-{qi}")))
            .debug_selector(move || format!("question-attach-{qi}"))
            .ghost()
            .xsmall()
            .icon(Icon::new(crate::ui::SkyeIcon::Paperclip))
            .label("Attach files")
            .on_click(
                cx.listener(move |this, _, _, cx| this.attach_answer_files(&attach_id, qi, cx)),
            ),
    )
}
