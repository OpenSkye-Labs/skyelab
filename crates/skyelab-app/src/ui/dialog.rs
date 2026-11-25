//! The dialog shell. The modal layer itself (focus trap, Esc, the dimmed `overlay` backdrop, the card's radius and
//! border) is gpui-component's `Dialog`, opened with [`open_dialog`] and colored by the theme; what goes in it is
//! ours: a title, the body, and [`dialog_footer`] with SkyeLab buttons, Cancel ([`dialog_cancel`]) before the action.

use super::Button;
use gpui::{App, Div, Styled as _, Window};
use gpui_component::{WindowExt as _, dialog::Dialog, h_flex};

/// Opens a dialog that fades in place ([`super::motion::DIALOG`]): every SkyeLab dialog goes through here, so they all
/// move alike (clippy bans gpui-component's `open_dialog` elsewhere). `build` can still set its own entrance.
#[allow(clippy::disallowed_methods)]
pub fn open_dialog(
    window: &mut Window,
    cx: &mut App,
    build: impl Fn(Dialog, &mut Window, &mut App) -> Dialog + 'static,
) {
    window.open_dialog(cx, move |dialog, window, cx| {
        build(dialog.entrance(super::motion::DIALOG), window, cx)
    });
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

/// A toast's action button (Undo). gpui-component's `Notification::action` takes its own button type, so this is
/// the one place SkyeLab still builds one, sized and labeled like ours.
#[allow(clippy::disallowed_types)]
pub fn notice_action(
    id: &'static str,
    label: &'static str,
    on_click: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> gpui_component::button::Button {
    use gpui_component::Sizable as _;
    gpui_component::button::Button::new(id)
        // gpui-component's `small` is 24 px, our `Xs`.
        .small()
        .label(label)
        .on_click(on_click)
}

/// The footer row: buttons at the end, 8 px apart. Pass it to `Dialog::footer`, or end a custom dialog body with it.
pub fn dialog_footer() -> Div {
    h_flex().w_full().justify_end().gap_2()
}
