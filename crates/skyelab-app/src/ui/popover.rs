//! SkyeLab's popovers: [`Popover`], opened by a click on its trigger, and [`HoverCard`], opened by resting the pointer
//! on it. Both draw their content in a deferred layer anchored to the trigger, at the popup priority.

use super::menu::POPUP_PRIORITY;
use super::menu_render::popover_surface;
use super::popup_place::popup_place;
use super::v_flex;
use gpui::{
    Anchor, AnyElement, App, Bounds, Context, ElementId, FocusHandle, InteractiveElement as _,
    IntoElement, KeyBinding, MouseButton, ParentElement as _, Pixels, Point, RenderOnce, Role,
    StatefulInteractiveElement as _, StyleRefinement, Styled, Task, Window, actions, canvas,
    deferred, div, point, prelude::FluentBuilder as _, px,
};
use std::{rc::Rc, time::Duration};

actions!(skye_popover, [Toggle, Close]);

const KEY_CONTEXT: &str = "SkyePopover";
const CONTENT_CONTEXT: &str = "SkyePopoverContent";
const WINDOW_MARGIN: Pixels = px(8.);

pub(super) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", Toggle, Some(KEY_CONTEXT)),
        KeyBinding::new("space", Toggle, Some(KEY_CONTEXT)),
        KeyBinding::new("escape", Close, Some(KEY_CONTEXT)),
        KeyBinding::new("escape", Close, Some(CONTENT_CONTEXT)),
        // The content sits inside the trigger's dispatch path: Enter and Space belong to its own controls.
        KeyBinding::new("enter", gpui::NoAction, Some(CONTENT_CONTEXT)),
        KeyBinding::new("space", gpui::NoAction, Some(CONTENT_CONTEXT)),
    ]);
}

fn position(anchor: Anchor, trigger: Bounds<Pixels>, offset: Pixels) -> Point<Pixels> {
    match anchor {
        Anchor::TopLeft => trigger.bottom_left() + point(px(0.), offset),
        Anchor::TopCenter => trigger.bottom_center() + point(px(0.), offset),
        Anchor::TopRight => trigger.bottom_right() + point(px(0.), offset),
        Anchor::BottomLeft => trigger.origin - point(px(0.), offset),
        Anchor::BottomCenter => trigger.top_center() - point(px(0.), offset),
        Anchor::BottomRight => trigger.top_right() - point(px(0.), offset),
        Anchor::LeftCenter => trigger.right_center() + point(offset, px(0.)),
        Anchor::RightCenter => trigger.left_center() - point(offset, px(0.)),
    }
}

/// The trigger, and while `content` is `Some` the content anchored to it: from the trigger's origin in this frame's
/// layout (so it follows a trigger that moves), at the corner its last measured `bounds` size puts it.
fn host(
    trigger: AnyElement,
    bounds: Bounds<Pixels>,
    on_bounds: impl Fn(Bounds<Pixels>, &mut App) + 'static,
    anchor: Anchor,
    offset: Pixels,
    content: Option<AnyElement>,
) -> gpui::Div {
    div()
        .relative()
        .child(trigger)
        .child(
            canvas(move |b, _, cx| on_bounds(b, cx), |_, _, _, _| {})
                .absolute()
                .size_full()
                .top_0()
                .left_0(),
        )
        .when_some(content, |d, content| {
            // Relative to the placer, which flows right below the trigger.
            let trigger = Bounds::new(point(px(0.), -bounds.size.height), bounds.size);
            let corner = position(anchor, trigger, offset);
            d.child(
                deferred(popup_place(
                    anchor,
                    corner,
                    WINDOW_MARGIN,
                    div().occlude().child(content),
                ))
                .with_priority(POPUP_PRIORITY),
            )
        })
}

pub(super) struct PopoverState {
    open: bool,
    focus: FocusHandle,
    restore: Option<FocusHandle>,
    bounds: Bounds<Pixels>,
    pub(super) menu: Option<gpui::Entity<super::PopupMenu>>,
    _dismiss: Option<gpui::Subscription>,
}

impl PopoverState {
    fn new(cx: &mut App) -> Self {
        Self {
            open: false,
            focus: cx.focus_handle(),
            restore: None,
            bounds: Bounds::default(),
            menu: None,
            _dismiss: None,
        }
    }

    pub(super) fn set_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.open == open {
            return;
        }
        self.open = open;
        if open {
            self.restore = window.focused(cx);
            self.focus.focus(window, cx);
        } else {
            let inside = self.focus.contains_focused(window, cx)
                || self.menu.as_ref().is_some_and(|m| {
                    gpui::Focusable::focus_handle(m.read(cx), cx).contains_focused(window, cx)
                });
            self.menu = None;
            self._dismiss = None;
            if inside {
                match self.restore.take() {
                    Some(restore) => restore.focus(window, cx),
                    None => window.blur(cx),
                }
            }
        }
        cx.notify();
    }

    /// Closes the popover when `menu` dismisses itself, and keeps it for later frames.
    pub(super) fn hold_menu(
        &mut self,
        menu: gpui::Entity<super::PopupMenu>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let state = cx.entity().downgrade();
        self._dismiss =
            Some(
                window.subscribe(&menu, cx, move |_, _: &gpui::DismissEvent, window, cx| {
                    let _ = state.update(cx, |s, cx| s.set_open(false, window, cx));
                }),
            );
        self.menu = Some(menu);
    }
}

type CardContent = Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>;
type Content = Rc<dyn Fn(&gpui::Entity<PopoverState>, &mut Window, &mut App) -> AnyElement>;

/// Content shown below (or above) its trigger after a click, closed by another click, a press outside or Escape.
#[derive(IntoElement)]
pub struct Popover {
    id: ElementId,
    anchor: Anchor,
    offset: Option<Pixels>,
    appearance: bool,
    trigger: Option<Box<dyn FnOnce(bool) -> AnyElement>>,
    content: Option<Content>,
}

impl Popover {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            anchor: Anchor::TopLeft,
            offset: None,
            appearance: true,
            trigger: None,
            content: None,
        }
    }

    pub fn anchor(mut self, anchor: Anchor) -> Self {
        self.anchor = anchor;
        self
    }

    /// The gap to the trigger; a quarter rem unless set.
    pub fn offset(mut self, offset: impl Into<Pixels>) -> Self {
        self.offset = Some(offset.into());
        self
    }

    /// Without the popover surface (fill, ring, shadow, padding): for content that draws its own, like a menu.
    pub(super) fn bare(mut self) -> Self {
        self.appearance = false;
        self
    }

    /// The trigger, drawn with whether the popover is open.
    pub fn trigger<E: IntoElement>(mut self, trigger: impl FnOnce(bool) -> E + 'static) -> Self {
        self.trigger = Some(Box::new(move |open| trigger(open).into_any_element()));
        self
    }

    pub fn content<E: IntoElement>(
        mut self,
        content: impl Fn(&mut Window, &mut App) -> E + 'static,
    ) -> Self {
        self.content = Some(Rc::new(move |_, window, cx| {
            content(window, cx).into_any_element()
        }));
        self
    }

    pub(super) fn content_with_state(
        mut self,
        content: impl Fn(&gpui::Entity<PopoverState>, &mut Window, &mut App) -> AnyElement + 'static,
    ) -> Self {
        self.content = Some(Rc::new(content));
        self
    }
}

impl RenderOnce for Popover {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(self.id.clone(), cx, |_, cx| PopoverState::new(cx));
        let (open, bounds, focus) = {
            let s = state.read(cx);
            (s.open, s.bounds, s.focus.clone())
        };
        let offset = self.offset.unwrap_or(window.rem_size() * 0.25);
        let trigger = self
            .trigger
            .map_or_else(|| div().into_any_element(), |t| t(open));
        let content = self.content.filter(|_| open).map(|content| {
            let body = content(&state, window, cx);
            let close = state.clone();
            let outside = state.clone();
            v_flex()
                .id("content")
                .role(Role::Dialog)
                .occlude()
                .tab_group()
                .track_focus(&focus)
                .key_context(CONTENT_CONTEXT)
                .on_action(move |_: &Close, window, cx| {
                    close.update(cx, |s, cx| s.set_open(false, window, cx))
                })
                .on_mouse_down_out(move |e, window, cx| {
                    let in_menu = outside
                        .read(cx)
                        .menu
                        .as_ref()
                        .is_some_and(|m| m.read(cx).chain_contains(e.position, cx));
                    if !in_menu
                        && !super::on_field_menu(e.position, cx)
                        && !outside.read(cx).bounds.contains(&e.position)
                    {
                        outside.update(cx, |s, cx| s.set_open(false, window, cx));
                    }
                })
                .when(self.appearance, |d| popover_surface(d, cx).p_3())
                .child(body)
                .into_any_element()
        });
        let toggle = state.clone();
        let keys = state.clone();
        let bounds_state = state.clone();
        host(
            trigger,
            bounds,
            move |b, cx| bounds_state.update(cx, |s, _| s.bounds = b),
            self.anchor,
            offset,
            content,
        )
        .id(self.id)
        .key_context(KEY_CONTEXT)
        .on_action(move |_: &Toggle, window, cx| {
            keys.update(cx, |s, cx| {
                let open = !s.open;
                s.set_open(open, window, cx)
            })
        })
        .on_action({
            let state = state.clone();
            move |_: &Close, window, cx| {
                if !state.read(cx).open {
                    cx.propagate();
                    return;
                }
                state.update(cx, |s, cx| s.set_open(false, window, cx))
            }
        })
        .on_mouse_down(MouseButton::Left, move |e, window, cx| {
            if !toggle.read(cx).bounds.contains(&e.position) {
                return;
            }
            cx.stop_propagation();
            toggle.update(cx, |s, cx| {
                let open = !s.open;
                s.set_open(open, window, cx)
            });
        })
    }
}

pub(super) struct HoverCardState {
    open: bool,
    bounds: Bounds<Pixels>,
    over_trigger: bool,
    over_card: bool,
    timer: Option<Task<()>>,
}

impl HoverCardState {
    fn hover(
        &mut self,
        trigger: bool,
        hovered: bool,
        delays: (Duration, Duration),
        cx: &mut Context<Self>,
    ) {
        if trigger {
            self.over_trigger = hovered;
        } else {
            self.over_card = hovered;
        }
        let want = self.over_trigger || self.over_card;
        if want == self.open {
            self.timer = None;
            return;
        }
        let delay = if want { delays.0 } else { delays.1 };
        self.timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let _ = this.update(cx, |s, cx| {
                s.open = want;
                s.timer = None;
                cx.notify();
            });
        }));
    }
}

/// Content shown next to its trigger once the pointer has rested on it for the open delay, kept while the pointer
/// is on the trigger or the card, gone after the close delay.
#[derive(IntoElement)]
pub struct HoverCard {
    id: ElementId,
    anchor: Anchor,
    open_delay: Duration,
    close_delay: Duration,
    style: StyleRefinement,
    trigger: Option<AnyElement>,
    content: Option<CardContent>,
}

impl HoverCard {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            anchor: Anchor::TopLeft,
            open_delay: Duration::from_millis(600),
            close_delay: Duration::from_millis(300),
            style: StyleRefinement::default(),
            trigger: None,
            content: None,
        }
    }

    pub fn anchor(mut self, anchor: Anchor) -> Self {
        self.anchor = anchor;
        self
    }

    pub fn open_delay(mut self, delay: Duration) -> Self {
        self.open_delay = delay;
        self
    }

    pub fn trigger(mut self, trigger: impl IntoElement) -> Self {
        self.trigger = Some(trigger.into_any_element());
        self
    }

    pub fn content<E: IntoElement>(
        mut self,
        content: impl Fn(&mut Window, &mut App) -> E + 'static,
    ) -> Self {
        self.content = Some(Rc::new(move |window, cx| {
            content(window, cx).into_any_element()
        }));
        self
    }
}

/// Styles the trigger's container: the box the parent lays out and the card is anchored to.
impl Styled for HoverCard {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for HoverCard {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(self.id.clone(), cx, |_, _| HoverCardState {
            open: false,
            bounds: Bounds::default(),
            over_trigger: false,
            over_card: false,
            timer: None,
        });
        let (open, bounds) = (state.read(cx).open, state.read(cx).bounds);
        let delays = (self.open_delay, self.close_delay);
        let top = matches!(
            self.anchor,
            Anchor::TopLeft | Anchor::TopCenter | Anchor::TopRight
        );
        let content = self.content.filter(|_| open).map(|content| {
            let card = state.clone();
            popover_surface(v_flex().id("card"), cx)
                .occlude()
                .overflow_hidden()
                .p_3()
                .map(|d| if top { d.mt_1() } else { d.mb_1() })
                .on_hover(move |hovered, _, cx| {
                    card.update(cx, |s, cx| s.hover(false, *hovered, delays, cx))
                })
                .child(content(window, cx))
                .into_any_element()
        });
        let hover = state.clone();
        let trigger = div()
            .id("trigger")
            .on_hover(move |hovered, _, cx| {
                hover.update(cx, |s, cx| s.hover(true, *hovered, delays, cx))
            })
            .children(self.trigger)
            .into_any_element();
        let bounds_state = state.clone();
        let mut root = host(
            trigger,
            bounds,
            move |b, cx| bounds_state.update(cx, |s, _| s.bounds = b),
            self.anchor,
            px(0.),
            content,
        );
        gpui::Refineable::refine(root.style(), &self.style);
        root.id(self.id)
    }
}
