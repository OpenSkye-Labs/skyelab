//! A window's open dialogs, drawn over everything else in a deferred layer. The top one owns the backdrop, Esc and the
//! focus trap; each fades in and out ([`super::motion::FADE`] / [`super::motion::FADE_OUT`]) unless it asked for
//! [`DialogEntrance::None`]. Focus moves into a dialog as it opens and back to what had it when it closes.

use super::dialog::{Dialog, DialogClosed, DialogEntrance, OnClose};
use super::window_root::WindowRoot;
use super::{A11yScrollArea as _, SkyeIcon, icon_button};
use crate::ui::ActiveTheme as _;
use crate::ui::v_flex;
use gpui::{
    Animation, AnimationExt as _, AnyElement, App, BoxShadow, Context, Entity, FocusHandle,
    FontWeight, IntoElement, KeyBinding, MouseButton, ParentElement as _, Pixels, Render,
    Styled as _, Window, accesskit, actions, anchored, deferred, div, hsla, point, prelude::*, px,
    relative,
};
use gpui_base::TestSupportExt as _;
use std::rc::Rc;

actions!(
    skyelab_dialog,
    [DismissDialog, FocusNextInDialog, FocusPrevInDialog]
);

/// Not gpui-base's `Dialog` context, which binds `escape` and `enter` for its own dialogs.
const KEY_CONTEXT: &str = "SkyeDialog";
/// Over SkyeLab's own overlays (settings, tab menus: 1 and 2), under popups and toasts (100), so a menu opened from a
/// dialog paints over it.
const PRIORITY: usize = 50;
/// More than any window's tab stops: Tab walks them looking for the dialog's.
const MAX_TAB_STEPS: usize = 4096;
/// The window's top strip, where a press drags the window instead of closing the dialog.
const TITLE_STRIP_H: Pixels = px(34.);
/// How far each dialog over another sits lower.
const STACK_STEP: f32 = 16.;

pub(super) type Build = Rc<dyn Fn(Dialog, &mut Window, &mut App) -> Dialog>;

struct Open {
    key: u64,
    build: Build,
    focus: FocusHandle,
    restore: Option<FocusHandle>,
    closing: bool,
    /// From the last frame's build: closing must not run `build` again, since a dialog is often closed from inside an
    /// update of the very entity its build reads.
    on_close: Option<OnClose>,
    entrance: DialogEntrance,
}

pub struct DialogLayer {
    stack: Vec<Open>,
    next_key: u64,
}

/// The dialog's keys. Bound with the app's other defaults, before the keymap takes its snapshot of them, so a reloaded
/// keybindings file keeps them.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", DismissDialog, Some(KEY_CONTEXT)),
        KeyBinding::new("tab", FocusNextInDialog, Some(KEY_CONTEXT)),
        KeyBinding::new("shift-tab", FocusPrevInDialog, Some(KEY_CONTEXT)),
    ]);
}

impl DialogLayer {
    pub(super) fn new() -> Self {
        Self {
            stack: Vec::new(),
            next_key: 0,
        }
    }

    /// The layer of the window built with [`super::window_root`], if this one was.
    pub(super) fn of(window: &Window, cx: &App) -> Option<Entity<Self>> {
        let root = window.root::<gpui_component::Root>()??;
        let ours = root.read(cx).view().clone().downcast::<WindowRoot>().ok()?;
        Some(ours.read(cx).dialogs.clone())
    }

    pub(super) fn is_open(&self) -> bool {
        self.stack.iter().any(|open| !open.closing)
    }

    pub(super) fn open(&mut self, build: Build, window: &mut Window, cx: &mut Context<Self>) {
        // What was selected behind the dialog must not be what cmd-c in the dialog copies.
        gpui_base::TextSelection::clear(window, cx);
        let focus = cx.focus_handle();
        let restore = window.focused(cx);
        window.focus(&focus, cx);
        self.next_key += 1;
        self.stack.push(Open {
            key: self.next_key,
            build,
            focus,
            restore,
            closing: false,
            on_close: None,
            entrance: DialogEntrance::Fade,
        });
        cx.notify();
    }

    pub(super) fn close_top(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(top) = self.stack.iter_mut().rev().find(|open| !open.closing) else {
            return;
        };
        top.closing = true;
        let (key, restore, on_close, entrance) = (
            top.key,
            top.restore.clone(),
            top.on_close.clone(),
            top.entrance,
        );
        if let Some(restore) = restore {
            window.focus(&restore, cx);
        } else if let Some(below) = self.stack.iter().rev().find(|open| !open.closing) {
            window.focus(&below.focus, cx);
        }
        // After this update: an `on_close` may open, close or ask about dialogs, which needs this layer again.
        if let Some(on_close) = on_close {
            window.defer(cx, move |window, cx| on_close(&DialogClosed, window, cx));
        }
        if entrance == DialogEntrance::None || cx.reduce_motion() {
            self.stack.retain(|open| open.key != key);
        } else {
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(super::motion::FADE_OUT)
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.stack.retain(|open| open.key != key);
                    cx.notify();
                });
            })
            .detach();
        }
        cx.notify();
    }

    /// Tab / Shift-Tab: the window's next tab stop, skipping every one outside the dialog.
    fn step(focus: &FocusHandle, forward: bool, window: &mut Window, cx: &mut App) {
        // Bounded for a dialog with no tab stops, which then keeps the focus itself.
        for _ in 0..MAX_TAB_STEPS {
            if forward {
                window.focus_next(cx);
            } else {
                window.focus_prev(cx);
            }
            if focus.contains_focused(window, cx) {
                return;
            }
        }
        window.focus(focus, cx);
    }
}

impl Render for DialogLayer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let top = self.stack.iter().rposition(|open| !open.closing);
        let layers: Vec<AnyElement> = (0..self.stack.len())
            .map(|ix| {
                let build = self.stack[ix].build.clone();
                let dialog = build(Dialog::new(), window, cx);
                let open = &mut self.stack[ix];
                open.on_close = dialog.on_close.clone();
                open.entrance = dialog.entrance;
                let place = Place {
                    key: open.key,
                    depth: ix,
                    top: top == Some(ix),
                    closing: open.closing,
                    focus: open.focus.clone(),
                };
                layer(dialog, place, window, cx)
            })
            .collect();
        div().children(layers)
    }
}

struct Place {
    key: u64,
    depth: usize,
    top: bool,
    closing: bool,
    focus: FocusHandle,
}

fn shadow(visible: f32) -> Vec<BoxShadow> {
    [(20., 25., -5.), (8., 10., -6.)]
        .into_iter()
        .map(|(y, blur, spread)| BoxShadow {
            color: hsla(0., 0., 0., 0.1 * visible),
            offset: point(px(0.), px(y)),
            blur_radius: px(blur),
            spread_radius: px(spread),
            inset: false,
        })
        .collect()
}

fn layer(
    dialog: Dialog,
    at: Place,
    window: &mut Window,
    cx: &mut Context<DialogLayer>,
) -> AnyElement {
    let theme = cx.theme();
    let viewport = window.viewport_size();
    let margin: Pixels = super::SPACE_LG;
    let lower = px(STACK_STEP * at.depth as f32);
    let width = dialog.width.min((viewport.width - margin * 2.).max(px(0.)));
    let x = (viewport.width - width) / 2.;
    let y = dialog.margin_top.unwrap_or(viewport.height / 10.) + lower;
    let max_height = (viewport.height - margin * 2. - lower).max(px(0.));
    let pad = |side: Option<gpui::DefiniteLength>| {
        side.map_or(px(16.), |l| {
            l.to_pixels(window.text_style().font_size, window.rem_size())
        })
    };
    let padding = &dialog.style.padding;
    let (top_pad, bottom_pad, left_pad, right_pad) = (
        pad(padding.top),
        pad(padding.bottom),
        pad(padding.left),
        pad(padding.right),
    );
    let name = dialog.title.clone().unwrap_or_else(|| "Dialog".into());
    let (overlay, background, border, radius) = (
        theme.overlay,
        theme.background,
        theme.border,
        theme.radius_lg,
    );
    let content = dialog.content.clone().map(|build| {
        let column = v_flex()
            .w_full()
            .flex_1()
            .rounded(radius)
            .gap(bottom_pad)
            .pl(left_pad)
            .pr(right_pad);
        build(column, window, cx)
    });
    let entity = cx.entity();
    let close = {
        let entity = entity.clone();
        move |window: &mut Window, cx: &mut App| {
            entity.update(cx, |layer, cx| layer.close_top(window, cx));
        }
    };
    let close_x = close.clone();
    let close_backdrop = close.clone();
    let focus = at.focus.clone();
    let (next, prev) = (at.focus.clone(), at.focus.clone());

    let mut surface = v_flex()
        .id(("dialog", at.key))
        .test_support()
        .when(!at.closing, |s| {
            s.role(accesskit::Role::Dialog)
                .aria_label(name.clone())
                .track_focus(&focus)
                .key_context(KEY_CONTEXT)
        })
        .when(at.top && !at.closing, |s| {
            s.on_action(move |_: &DismissDialog, window, cx| close(window, cx))
                .on_action(move |_: &FocusNextInDialog, window, cx| {
                    DialogLayer::step(&next, true, window, cx)
                })
                .on_action(move |_: &FocusPrevInDialog, window, cx| {
                    DialogLayer::step(&prev, false, window, cx)
                })
        })
        .absolute()
        .left(x)
        .top(y)
        .bg(background)
        .border_1()
        .border_color(border)
        .rounded(radius)
        .min_h_24()
        .pt(top_pad)
        .pb(bottom_pad)
        .gap(top_pad.max(px(8.)));
    surface.style().refine(&dialog.style);
    let surface = surface
        .px_0()
        .w(width)
        .max_h(max_height)
        .occlude()
        // A dialog fading out is gone for input: a second click on its buttons must not run them again.
        .when(at.closing, |s| {
            s.capture_any_mouse_down(|_, _, cx| cx.stop_propagation())
                .capture_any_mouse_up(|_, _, cx| cx.stop_propagation())
        })
        .shadow(shadow(1.))
        .child(
            v_flex()
                .flex_1()
                .overflow_hidden()
                .gap_y_2()
                .when_some(dialog.title.clone(), |c, title| {
                    c.child(
                        div()
                            .pl(left_pad)
                            .pr(right_pad)
                            .text_base()
                            .font_weight(FontWeight::SEMIBOLD)
                            .line_height(relative(1.25))
                            .child(super::dialog::heading(title).test_support()),
                    )
                })
                .children(content)
                .when(!dialog.children.is_empty(), |c| {
                    c.child(
                        div().flex_1().overflow_hidden().child(
                            v_flex()
                                .id(("dialog-body", at.key))
                                .size_full()
                                .overflow_y_scroll()
                                .a11y_scroll_area()
                                .pl(left_pad)
                                .pr(right_pad)
                                .children(dialog.children),
                        ),
                    )
                }),
        )
        .when_some(dialog.footer, |s, footer| {
            s.child(div().pl(left_pad).pr(right_pad).child(footer))
        })
        .when(dialog.close_button, |s| {
            s.child(
                div()
                    .absolute()
                    .top((top_pad - px(14.)).max(px(4.)))
                    .right((right_pad - px(13.5)).max(px(4.5)))
                    .child(
                        icon_button(("dialog-close", at.key), SkyeIcon::X)
                            .small()
                            .tooltip("Close")
                            .accessibility_label(format!("Close {name}"))
                            .on_click(move |_, window, cx| close_x(window, cx)),
                    ),
            )
        });

    let still = dialog.entrance == DialogEntrance::None || cx.reduce_motion();
    let layer =
        div()
            .id(("dialog-layer", at.key))
            .size_full()
            .when(at.top && !at.closing, |l| {
                l.occlude().child(super::window_controls::drag_area(
                    div().absolute().inset_0().bg(overlay).on_any_mouse_down(
                        move |e, window, cx| {
                            // The window's title strip still drags the window.
                            if e.button == MouseButton::Left && e.position.y >= TITLE_STRIP_H {
                                close_backdrop(window, cx);
                            }
                        },
                    ),
                ))
            })
            .child(surface);
    let layer = match (still, at.closing) {
        (true, _) => layer.into_any_element(),
        (false, false) => layer
            .with_animation(
                ("dialog-in", at.key),
                Animation::new(super::motion::FADE).with_easing(super::motion::ease_out_quad),
                |l, t| l.opacity(t),
            )
            .into_any_element(),
        (false, true) => layer
            .with_animation(
                ("dialog-out", at.key),
                Animation::new(super::motion::FADE_OUT).with_easing(super::motion::ease_out_quad),
                |l, t| l.opacity(1. - t),
            )
            .into_any_element(),
    };
    deferred(
        anchored()
            .position(point(px(0.), px(0.)))
            .snap_to_window()
            .child(div().w(viewport.width).h(viewport.height).child(layer)),
    )
    .with_priority(PRIORITY)
    .into_any_element()
}
