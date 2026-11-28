//! Windows' caption buttons (minimize, maximize/restore, close) drawn by SkyeLab in the window's top-right corner, over
//! the 52 px top row, the way macOS puts its traffic lights in that row's start. There is no separate title strip; the
//! top rows reserve [`WINDOW_CONTROLS_W`] at their end, and their empty space is the drag area ([`drag_area`]).
//! T3's Electron build does the same with `titleBarOverlay`.

use super::HEADER_H;
use super::SkyeIcon;
use crate::ui::ActiveTheme as _;
use crate::ui::{Icon, h_flex};
use gpui::{
    App, InteractiveElement, IntoElement, ParentElement as _, Pixels, RenderOnce, Styled as _,
    Window, WindowControlArea, div, prelude::*, px,
};
use std::sync::LazyLock;

/// One caption button: Windows 11's 46 px wide, 32 px tall.
const BUTTON_W: Pixels = px(46.);
const BUTTON_H: Pixels = px(32.);
/// Room the three buttons take at the end of the top row.
pub const WINDOW_CONTROLS_W: Pixels = px(46. * 3.);

/// Whether this window draws its own caption buttons: on Windows, and in debug builds with
/// `SKYELAB_VERIFY_WINDOWS_CHROME=1` (macOS then draws them too, inert, so a live check can see the Windows layout).
pub fn drawn() -> bool {
    static ON: LazyLock<bool> = LazyLock::new(|| {
        cfg!(target_os = "windows")
            || (cfg!(debug_assertions)
                && std::env::var_os("SKYELAB_VERIFY_WINDOWS_CHROME").is_some())
    });
    *ON
}

/// Marks empty top-row space as the window's caption on Windows: drag to move, double-click to maximize. Only for an
/// element with no clickable children: a press anywhere in it reaches Windows as a caption press. A clickable child
/// goes in [`no_drag`].
pub fn drag_area<E: InteractiveElement>(el: E) -> E {
    if cfg!(target_os = "windows") {
        el.window_control_area(WindowControlArea::Drag)
    } else {
        el
    }
}

/// A clickable element inside a [`drag_area`]: it occludes the caption under it, so its press stays a click.
pub fn no_drag<E: InteractiveElement>(el: E) -> E {
    if cfg!(target_os = "windows") {
        el.occlude()
    } else {
        el
    }
}

/// The three buttons, absolutely placed at the top-right of the window's root. Draws nothing unless [`drawn`].
#[derive(IntoElement)]
pub struct WindowControls;

impl RenderOnce for WindowControls {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        if !drawn() {
            return div().into_any_element();
        }
        let theme = cx.theme();
        let (fg, hover) = (theme.foreground, theme.accent);
        // Windows' own close hover: red with white.
        let close_bg = gpui::rgb(0xc42b1c);
        let button = |id: &'static str, icon: SkyeIcon, area: WindowControlArea| {
            let close = matches!(area, WindowControlArea::Close);
            div()
                .id(id)
                .w(BUTTON_W)
                .h(BUTTON_H)
                .flex()
                .items_center()
                .justify_center()
                .text_color(fg)
                .when(close, |b| {
                    b.hover(move |s| s.bg(close_bg).text_color(gpui::white()))
                })
                .when(!close, |b| b.hover(move |s| s.bg(hover)))
                .window_control_area(area)
                .child(Icon::new(icon).size_3p5())
        };
        h_flex()
            .id("window-controls")
            .absolute()
            .top_0()
            .right_0()
            .h(BUTTON_H)
            .max_h(HEADER_H)
            .occlude()
            .child(button(
                "window-min",
                SkyeIcon::WindowMinimize,
                WindowControlArea::Min,
            ))
            .child(button(
                "window-max",
                if window.is_maximized() {
                    SkyeIcon::WindowRestore
                } else {
                    SkyeIcon::WindowMaximize
                },
                WindowControlArea::Max,
            ))
            .child(button(
                "window-close",
                SkyeIcon::WindowClose,
                WindowControlArea::Close,
            ))
            .into_any_element()
    }
}
