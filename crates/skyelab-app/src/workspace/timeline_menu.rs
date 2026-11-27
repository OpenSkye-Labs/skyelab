//! Right-click context menu for timeline messages (user prompts and assistant replies).
//!
//! Drawn at the thread view level above the virtualized `list` (like `sidebar_snooze.rs`), never with
//! gpui-component's `context_menu` inside a list row: a `deferred(anchored(..))` inside a GPUI `list` row opens
//! invisible and never paints.

use super::remote_thread::RemoteThreadView;
use super::thread::ThreadView;
use super::thread_items::ItemsView;
use super::turn_files::minimap::markdown_plain;
use gpui::{
    AnyElement, App, ClipboardItem, Context, DismissEvent, Entity, FocusHandle, Focusable as _,
    InteractiveElement as _, IntoElement, MouseButton, Pixels, Point, Subscription, Window,
    anchored, deferred, div, prelude::*, px,
};
use gpui_component::menu::{PopupMenu, PopupMenuItem};
use skyelab_core::terminal_context;

type QuoteFn = Box<dyn Fn(&mut Window, &mut App)>;

struct MenuRequest {
    is_markdown: bool,
    text: String,
    at: Point<Pixels>,
    fallback_focus: FocusHandle,
    on_quote: Option<QuoteFn>,
}

struct OpenMenu {
    at: Point<Pixels>,
    menu: Entity<PopupMenu>,
    return_focus: Option<FocusHandle>,
    _dismiss: Subscription,
}

/// The open right-click menu for a thread view's timeline.
#[derive(Default)]
pub(in crate::workspace) struct TimelineMenu {
    open: Option<OpenMenu>,
}

impl TimelineMenu {
    /// Opens a message menu at window point `req.at`. `req.on_quote` is `Some` when the view accepts quotes.
    fn open<V: 'static>(
        &mut self,
        req: MenuRequest,
        on_dismiss: impl Fn(&mut V, &mut Context<V>) + 'static,
        window: &mut Window,
        cx: &mut Context<V>,
    ) {
        let return_focus = window.focused(cx);
        let action_ctx = return_focus.clone().unwrap_or(req.fallback_focus);
        let copy_raw = req.text.clone();
        let copy_plain = req
            .is_markdown
            .then(|| markdown_plain(&req.text).trim_end().to_owned());
        let on_quote = req.on_quote;
        let menu =
            PopupMenu::build(window, cx, move |mut menu, _, _| {
                menu = menu.action_context(action_ctx).item(
                    PopupMenuItem::new("Copy message").on_click(move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(copy_raw.clone()));
                    }),
                );
                if let Some(plain) = copy_plain {
                    menu = menu.item(PopupMenuItem::new("Copy as plain text").on_click(
                        move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(plain.clone()));
                        },
                    ));
                }
                if let Some(quote) = on_quote {
                    menu = menu.item(
                        PopupMenuItem::new("Quote into composer")
                            .on_click(move |_, window, cx| quote(window, cx)),
                    );
                }
                menu
            });
        menu.focus_handle(cx).focus(window, cx);
        let _dismiss = cx.subscribe(&menu, move |this, _, _: &DismissEvent, cx| {
            on_dismiss(this, cx);
        });
        self.open = Some(OpenMenu {
            at: req.at,
            menu,
            return_focus,
            _dismiss,
        });
        cx.notify();
    }

    /// Closes the menu, restoring focus when it was still inside the menu. Returns whether it was open.
    pub(in crate::workspace) fn close<V: 'static>(
        &mut self,
        window: &mut Window,
        cx: &mut Context<V>,
    ) -> bool {
        let Some(open) = self.open.take() else {
            return false;
        };
        if open.menu.focus_handle(cx).contains_focused(window, cx)
            && let Some(prev) = open.return_focus
        {
            prev.focus(window, cx);
        }
        cx.notify();
        true
    }

    /// Clears the stored menu after `PopupMenu` already dismissed itself.
    pub(in crate::workspace) fn clear<V: 'static>(&mut self, cx: &mut Context<V>) {
        if self.open.take().is_some() {
            cx.notify();
        }
    }

    #[cfg(all(test, unix))]
    pub(in crate::workspace) fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// The open menu over everything, with an occluding backdrop that closes it on outside presses.
    pub(in crate::workspace) fn render<V: 'static>(
        &self,
        on_close: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
        cx: &mut Context<V>,
    ) -> Option<AnyElement> {
        let open = self.open.as_ref()?;
        let at = open.at;
        let menu = open.menu.clone();
        let on_close = std::rc::Rc::new(on_close);
        let left_close = on_close.clone();
        let right_close = on_close;
        Some(
            deferred(
                div()
                    .id("timeline-menu-backdrop")
                    .absolute()
                    .inset_0()
                    .occlude()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            left_close(this, window, cx);
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            right_close(this, window, cx);
                        }),
                    )
                    .child(
                        anchored()
                            .position(at)
                            .snap_to_window_with_margin(px(8.))
                            .child(
                                div()
                                    .debug_selector(|| "timeline-context-menu".into())
                                    .occlude()
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation();
                                    })
                                    .on_mouse_down(MouseButton::Right, |_, _, cx| {
                                        cx.stop_propagation();
                                    })
                                    .child(menu),
                            ),
                    ),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }
}

/// Attaches a right-click handler to a timeline message element that opens the view's `TimelineMenu`.
pub(super) fn on_message<V: ItemsView, E: InteractiveElement>(
    el: E,
    is_markdown: bool,
    text: &str,
    cx: &mut Context<V>,
) -> E {
    let text = text.to_owned();
    el.on_mouse_down(
        MouseButton::Right,
        cx.listener(move |this, e: &gpui::MouseDownEvent, window, cx| {
            cx.stop_propagation();
            this.message_menu(is_markdown, text.clone(), e.position, window, cx);
        }),
    )
}

/// Renders a user prompt / steer row with right-click context menu support.
pub(super) fn prompt_row<V: ItemsView>(
    this: &V,
    ix: usize,
    key: u64,
    text: &str,
    cx: &mut Context<V>,
) -> AnyElement {
    use crate::ui;
    use gpui::relative;
    use gpui_component::{ActiveTheme as _, v_flex};
    let theme = cx.theme();
    v_flex()
        .w_full()
        .items_end()
        .gap_1p5()
        .children(
            this.sent_images(text)
                .map(|images| super::image_viewer::thumbnails(key, images)),
        )
        .child(on_message(
            div()
                .debug_selector(|| "prompt-text".into())
                .max_w(relative(0.8))
                .p_3()
                .rounded(ui::RADIUS_XL)
                .bg(theme.secondary)
                .text_size(ui::BODY)
                .child(super::find_marks::prompt_text(
                    text,
                    this.find_words(ix),
                    theme.warning,
                )),
            false,
            text,
            cx,
        ))
        .into_any_element()
}

/// Renders an assistant reply row with right-click context menu support.
pub(super) fn reply_row<V: ItemsView>(
    this: &V,
    ix: usize,
    text: &str,
    root: Option<&std::path::Path>,
    reply_at: Option<i64>,
    window: &mut Window,
    cx: &mut Context<V>,
) -> AnyElement {
    use crate::ui;
    use gpui_component::{ActiveTheme as _, h_flex, v_flex};
    let key = this.items().key(ix);
    let muted = cx.theme().muted_foreground;
    on_message(
        v_flex()
            .debug_selector(|| "reply-text".into())
            .px_1()
            .gap_1()
            .text_size(ui::BODY)
            .child(super::timeline_text::markdown(
                ("md", key),
                text,
                false,
                root,
                this.find_words(ix),
                window,
                cx,
            ))
            .child(
                h_flex()
                    .debug_selector(|| "message-copy".into())
                    .gap_1()
                    .text_size(ui::META)
                    .text_color(muted)
                    .child(
                        div()
                            .opacity(0.6)
                            .child(super::tool_cards::copy(("copy-msg", key), text.to_owned())),
                    )
                    .children(reply_at.map(ui::clock))
                    .children(this.fork(ix).map(super::thread_extras::fork::button)),
            ),
        true,
        text,
        cx,
    )
    .into_any_element()
}

impl ThreadView {
    pub(in crate::workspace) fn open_message_menu(
        &mut self,
        is_markdown: bool,
        text: String,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.composer.quote = None;
        let weak = cx.weak_entity();
        let quote_text = text.clone();
        let on_quote: Option<QuoteFn> =
            (!self.read_only && !quote_text.trim().is_empty()).then(|| {
                Box::new(move |window: &mut Window, cx: &mut App| {
                    let _ = weak.update(cx, |this, cx| {
                        this.quote_text(&quote_text, window, cx);
                    });
                }) as QuoteFn
            });
        let req = MenuRequest {
            is_markdown,
            text,
            at,
            fallback_focus: self.focus.clone(),
            on_quote,
        };
        self.timeline.menu.open(
            req,
            |this: &mut Self, cx| this.timeline.menu.clear(cx),
            window,
            cx,
        );
    }

    pub(in crate::workspace) fn close_message_menu(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.timeline.menu.close(window, cx)
    }

    pub(in crate::workspace) fn render_message_menu(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        self.timeline.menu.render(
            |this: &mut Self, window, cx| {
                this.close_message_menu(window, cx);
            },
            cx,
        )
    }
}

impl RemoteThreadView {
    pub(in crate::workspace) fn open_message_menu(
        &mut self,
        is_markdown: bool,
        text: String,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let weak = cx.weak_entity();
        let quote_text = text.clone();
        let on_quote: Option<QuoteFn> = (!quote_text.trim().is_empty()).then(|| {
            Box::new(move |window: &mut Window, cx: &mut App| {
                let _ = weak.update(cx, |this, cx| {
                    this.quote_into_prompt(&quote_text, window, cx);
                });
            }) as QuoteFn
        });
        let req = MenuRequest {
            is_markdown,
            text,
            at,
            fallback_focus: self.focus_handle(cx),
            on_quote,
        };
        self.menu
            .open(req, |this: &mut Self, cx| this.menu.clear(cx), window, cx);
    }

    /// Appends `text` as a markdown quote into the remote composer and focuses it.
    fn quote_into_prompt(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let clipped = terminal_context::clip(text);
        if clipped.trim().is_empty() {
            return;
        }
        let quoted = terminal_context::quoted(&clipped);
        let existing = self.prompt.read(cx).value().to_string();
        let next = if existing.trim().is_empty() {
            format!("{quoted}\n\n")
        } else {
            format!("{}\n\n{quoted}\n\n", existing.trim_end())
        };
        self.prompt
            .update(cx, |p, cx| p.set_value(next, window, cx));
        self.focus_prompt(window, cx);
    }

    pub(in crate::workspace) fn close_message_menu(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.menu.close(window, cx)
    }

    pub(in crate::workspace) fn render_message_menu(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        self.menu.render(
            |this: &mut Self, window, cx| {
                this.close_message_menu(window, cx);
            },
            cx,
        )
    }
}
