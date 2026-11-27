//! Toasts, after Rex: a toast fades in where it lands over [`motion::FADE`] (ease-out quad, no slide) and leaves at
//! once. gpui-component's notifications slide 96 px over 400 ms with fixed timings, so SkyeLab keeps its own layer;
//! clippy bans theirs. The stack sits top right under the header, as in T3, newest on top; a toast hides after
//! [`AUTOHIDE`] unless it has an action or the pointer is on it. [`push_system`] is the OS notification for a window
//! that isn't in front (no in-app toast).

use super::{Button, HEADER_H, META, RADIUS_LG, SkyeIcon, icon_button, motion};
use gpui::{
    Animation, AnimationExt as _, App, AppContext as _, Context, Entity, FontWeight, Global, Hsla,
    InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Task, Window, WindowId, deferred, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{ActiveTheme as _, Icon, h_flex, v_flex};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

/// How long a toast without an action stays (gpui-component's was 5 s too).
pub const AUTOHIDE: Duration = Duration::from_secs(5);
/// T3's toast width (`max-w-90`).
const WIDTH: f32 = 360.;
/// The most toasts shown at once; an older one goes when a newer one would pass it.
const MAX: usize = 5;

/// A toast's click, action or close handler.
pub type Handler = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Plain,
    Info,
    Success,
    Warning,
    Error,
}

/// One toast: a message, optionally a title, a kind's icon, one action button and a click handler.
#[derive(Clone)]
pub struct Toast {
    kind: Kind,
    title: Option<SharedString>,
    message: SharedString,
    action: Option<(SharedString, Handler)>,
    on_click: Option<Handler>,
    key: Option<SharedString>,
    autohide: bool,
}

impl Toast {
    pub fn new(message: impl Into<SharedString>) -> Self {
        Self {
            kind: Kind::Plain,
            title: None,
            message: message.into(),
            action: None,
            on_click: None,
            key: None,
            autohide: true,
        }
    }
    pub fn info(message: impl Into<SharedString>) -> Self {
        Self::new(message).kind(Kind::Info)
    }
    pub fn success(message: impl Into<SharedString>) -> Self {
        Self::new(message).kind(Kind::Success)
    }
    pub fn warning(message: impl Into<SharedString>) -> Self {
        Self::new(message).kind(Kind::Warning)
    }
    pub fn error(message: impl Into<SharedString>) -> Self {
        Self::new(message).kind(Kind::Error)
    }
    pub fn kind(mut self, kind: Kind) -> Self {
        self.kind = kind;
        self
    }
    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }
    /// A button (Undo, Restart) that runs `f` and closes the toast. A toast with one stays until it is used or closed,
    /// unless [`Toast::autohide`] turns hiding back on.
    pub fn action(
        mut self,
        label: impl Into<SharedString>,
        f: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.action = Some((label.into(), Rc::new(f)));
        self.autohide = false;
        self
    }
    /// Whether it hides after [`AUTOHIDE`]: on by default, off once it has an action (call this after `action`
    /// to keep it on, as Undo does).
    pub fn autohide(mut self, autohide: bool) -> Self {
        self.autohide = autohide;
        self
    }
    /// A click anywhere on the toast runs `f` and closes it.
    pub fn on_click(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }
    /// A toast pushed with the key of one still shown replaces it in place.
    pub fn key(mut self, key: impl Into<SharedString>) -> Self {
        self.key = Some(key.into());
        self
    }
}

struct Shown {
    id: u64,
    toast: Toast,
    _hide: Option<Task<()>>,
}

/// A window's toasts, newest last.
#[derive(Default)]
pub struct ToastList {
    shown: Vec<Shown>,
    next: u64,
    hovered: Option<u64>,
}

impl ToastList {
    fn push(&mut self, toast: Toast, cx: &mut Context<Self>) {
        let at = toast.key.as_ref().and_then(|k| {
            self.shown
                .iter()
                .position(|s| s.toast.key.as_ref() == Some(k))
        });
        // A replaced toast keeps its id: it swaps in place, without fading in again.
        let id = match at {
            Some(ix) => self.shown.remove(ix).id,
            None => {
                self.next += 1;
                self.next
            }
        };
        let hide = toast.autohide.then(|| {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(AUTOHIDE).await;
                // The pointer is on it: wait until it leaves.
                while this
                    .update(cx, |l, _| l.hovered == Some(id))
                    .unwrap_or(false)
                {
                    cx.background_executor().timer(Duration::from_secs(1)).await;
                }
                let _ = this.update(cx, |l, cx| l.dismiss(id, cx));
            })
        });
        self.shown.push(Shown {
            id,
            toast,
            _hide: hide,
        });
        if self.shown.len() > MAX {
            self.shown.remove(0);
        }
        cx.notify();
    }

    fn dismiss(&mut self, id: u64, cx: &mut Context<Self>) {
        self.shown.retain(|s| s.id != id);
        if self.hovered == Some(id) {
            self.hovered = None;
        }
        cx.notify();
    }
}

impl Render for ToastList {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let list = cx.entity().downgrade();
        let max_w = (window.viewport_size().width - px(32.))
            .max(px(200.))
            .min(px(WIDTH));
        let dark = cx.theme().is_dark();
        v_flex().gap_2().children(self.shown.iter().rev().map(|s| {
            let id = s.id;
            let close: Handler = {
                let list = list.clone();
                Rc::new(move |_, cx| {
                    let _ = list.update(cx, |l, cx| l.dismiss(id, cx));
                })
            };
            let on_click = s.toast.on_click.clone();
            let hover = list.clone();
            card_sized(id, &s.toast, max_w, close.clone(), cx)
                .on_hover(move |on, _, cx| {
                    let _ = hover.update(cx, |l, _| {
                        if *on {
                            l.hovered = Some(id);
                        } else if l.hovered == Some(id) {
                            l.hovered = None;
                        }
                    });
                })
                .when_some(on_click, |el, f| {
                    el.cursor_pointer().on_click(move |_, window, cx| {
                        close(window, cx);
                        f(window, cx);
                    })
                })
                .with_animation(
                    ("toast-in", id),
                    Animation::new(motion::FADE).with_easing(motion::ease_out_quad),
                    // GPUI paints a shadow under the card with no group compositing, so a half-faded card shows
                    // it through: the shadow follows the cube of the fade (gpui-component does the same).
                    move |el, t| {
                        el.opacity(t)
                            .shadow(motion::overlay_shadow(t * t * t, dark))
                    },
                )
        }))
    }
}

fn tint(kind: Kind, cx: &App) -> Option<(SkyeIcon, Hsla)> {
    let t = cx.theme();
    match kind {
        Kind::Plain => None,
        Kind::Info => Some((SkyeIcon::Info, t.info)),
        Kind::Success => Some((SkyeIcon::CircleCheck, t.success)),
        Kind::Warning => Some((SkyeIcon::TriangleAlert, t.warning)),
        Kind::Error => Some((SkyeIcon::CircleX, t.danger)),
    }
}

/// The toast's card: the kind's icon, title and message, the action, and a close button. `close` removes it; the
/// gallery passes a no-op.
pub fn card(id: u64, toast: &Toast, close: Handler, cx: &App) -> gpui::Stateful<gpui::Div> {
    card_sized(id, toast, px(WIDTH), close, cx)
}

fn card_sized(
    id: u64,
    toast: &Toast,
    width: Pixels,
    close: Handler,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let theme = cx.theme();
    let dark = theme.is_dark();
    let group = SharedString::from(format!("toast-{id}"));
    let a11y_label = match &toast.title {
        Some(t) => format!("{t}: {}", toast.message),
        None => toast.message.to_string(),
    };
    let role = if toast.kind == Kind::Error {
        gpui::accesskit::Role::Alert
    } else if toast.on_click.is_some() {
        gpui::accesskit::Role::Button
    } else {
        gpui::accesskit::Role::Status
    };
    h_flex()
        .id(("toast", id))
        .role(role)
        .aria_label(a11y_label)
        .accessibility_id(format!("toast-{id}"))
        .debug_selector(move || format!("toast-{id}"))
        .group(group.clone())
        .occlude()
        .w(width)
        .items_start()
        .gap_2p5()
        .pl_4()
        .pr_2()
        .py_3()
        .rounded(RADIUS_LG)
        .border_1()
        // On true-black dark canvases a black drop shadow vanishes, so use the slightly stronger `input` hairline.
        .border_color(if dark { theme.input } else { theme.border })
        .bg(theme.popover)
        .text_color(theme.popover_foreground)
        .shadow(motion::overlay_shadow(1., dark))
        .when_some(tint(toast.kind, cx), |el, (icon, color)| {
            el.child(
                div()
                    .pt(px(2.))
                    .child(Icon::new(icon).size_4().text_color(color)),
            )
        })
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .when_some(toast.title.clone(), |el, title| {
                    el.child(div().font_weight(FontWeight::MEDIUM).child(title))
                })
                .child(
                    div()
                        .when(toast.title.is_some(), |d| {
                            d.text_size(META).text_color(theme.muted_foreground)
                        })
                        .child(toast.message.clone()),
                ),
        )
        .when_some(toast.action.clone(), |el, (label, f)| {
            let close = close.clone();
            el.child(
                div().debug_selector(|| "toast-action".into()).child(
                    Button::new(("toast-action", id))
                        .xsmall()
                        .label(label)
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            close(window, cx);
                            f(window, cx);
                        }),
                ),
            )
        })
        .child(
            // Subtle at rest instead of `.invisible()`, so keyboard users can always tab to Close.
            div()
                .opacity(0.6)
                .group_hover(group, |s| s.opacity(1.))
                .child(
                    icon_button(("toast-close", id), SkyeIcon::X)
                        .xsmall()
                        .tooltip("Close")
                        // Not "Close": that is the window's own close button in the same tree.
                        .accessibility_label("Dismiss notification")
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            close(window, cx);
                        }),
                ),
        )
}

/// Each window's toast list, made on its first toast.
#[derive(Default)]
struct Layers(HashMap<WindowId, Entity<ToastList>>);
impl Global for Layers {}

/// Shows `toast` in `window`'s stack.
pub fn push(window: &mut Window, toast: Toast, cx: &mut App) {
    let here = window.window_handle().window_id();
    // The window being updated isn't in `cx.windows()` right now, so it is kept by id.
    let live: Vec<WindowId> = cx.windows().iter().map(|w| w.window_id()).collect();
    let layers = &mut cx.default_global::<Layers>().0;
    layers.retain(|id, _| *id == here || live.contains(id));
    let list = match layers.get(&here) {
        Some(list) => list.clone(),
        None => {
            let list = cx.new(|_| ToastList::default());
            cx.default_global::<Layers>().0.insert(here, list.clone());
            // The view drawing `layer` has to draw once to pick the new list up.
            window.refresh();
            list
        }
    };
    list.update(cx, |l, cx| l.push(toast, cx));
}

/// The window's toast stack, for its root view to draw last. `None` until the window's first toast.
pub fn layer(window: &Window, cx: &App) -> Option<impl IntoElement + use<>> {
    let list = cx
        .try_global::<Layers>()?
        .0
        .get(&window.window_handle().window_id())?
        .clone();
    Some(
        // Above dialogs (gpui-component's are 10 + their index), as its toasts were.
        deferred(
            div()
                .absolute()
                .top(HEADER_H + px(16.))
                .right(px(16.))
                .child(list),
        )
        .with_priority(100),
    )
}

/// The messages shown in `window`, newest last (tests).
#[cfg(test)]
pub fn shown(window: &Window, cx: &App) -> Vec<SharedString> {
    cx.try_global::<Layers>()
        .and_then(|l| l.0.get(&window.window_handle().window_id()))
        .map(|l| {
            l.read(cx)
                .shown
                .iter()
                .map(|s| s.toast.message.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// An OS notification for a window that isn't in front: no in-app toast (the user isn't looking). A click on it
/// brings the window forward and runs the toast's click handler. gpui-component owns the app's one OS response
/// handler, so this goes through its notification, the one place it is still built.
#[allow(clippy::disallowed_methods, clippy::disallowed_types)]
pub fn push_system(window: &mut Window, toast: Toast, cx: &mut App) {
    use gpui_component::WindowExt as _;
    use gpui_component::notification::Notification;
    struct Keyed;
    if !matches!(window.root::<gpui_component::Root>(), Some(Some(_))) {
        return; // a test window without gpui-component's Root
    }
    let mut note = Notification::new().message(toast.message).system();
    if let Some(title) = toast.title {
        note = note.title(title);
    }
    if let Some(key) = toast.key {
        note = note.id1::<Keyed>(key);
    }
    if let Some(f) = toast.on_click {
        note = note.on_click(move |_, window, cx| f(window, cx));
    }
    window.push_notification(note, cx);
}
