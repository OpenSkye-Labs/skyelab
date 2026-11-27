//! The dialog shell. The modal layer itself (focus trap, Esc, the dimmed `overlay` backdrop, the card's radius and
//! border) is gpui-component's `Dialog`, opened with [`open_dialog`] and colored by the theme; what goes in it is
//! ours: a title, the body, and [`dialog_footer`] with SkyeLab buttons, Cancel ([`dialog_cancel`]) before the action.

use super::{BODY, Button};
use gpui::{App, Div, FontWeight, SharedString, Styled as _, Window, div, prelude::*};
use gpui_component::{WindowExt as _, dialog::Dialog, h_flex};

/// Opens a dialog that fades in place ([`super::motion::DIALOG`]): every SkyeLab dialog goes through here, so they all
/// move alike (clippy bans gpui-component's `open_dialog` elsewhere). `build` can still set its own entrance.
#[allow(clippy::disallowed_methods)]
pub fn open_dialog(
    window: &mut Window,
    cx: &mut App,
    build: impl Fn(Dialog, &mut Window, &mut App) -> Dialog + 'static,
) {
    super::tooltip::dialog_opened(window, cx);
    window.open_dialog(cx, move |dialog, window, cx| {
        build(dialog.entrance(super::motion::DIALOG), window, cx)
    });
}

/// Accessible dialog heading wrapper (`gpui_component::dialog::DialogTitle` omits `Role::Heading`).
pub fn dialog_title(title: impl Into<SharedString>) -> impl IntoElement {
    let title: SharedString = title.into();
    div()
        .id(SharedString::from(format!("dialog-title:{title}")))
        .role(gpui::accesskit::Role::Heading)
        .aria_label(title.clone())
        .text_size(BODY)
        .font_weight(FontWeight::SEMIBOLD)
        .child(title)
}

/// A dialog footer's Cancel. Not gpui-component's `DialogClose`: it wraps its button in a `size_full` div, which
/// stretches Cancel across the footer (docs/gpui.md).
pub fn dialog_cancel(id: &'static str) -> Button {
    Button::new(id)
        .outline()
        .small()
        .label("Cancel")
        .on_click(|_, window, cx| window.close_dialog(cx))
}

/// The footer row: buttons at the end, 8 px apart. Pass it to `Dialog::footer`, or end a custom dialog body with it.
pub fn dialog_footer() -> Div {
    h_flex().w_full().justify_end().gap_2()
}
