//! How a thread tab looks. State and behavior live in `thread.rs`.

use super::thread::{CONTEXT, Interrupt, SendPrompt, Stop, ThreadView};
use super::thread_items::render_item;
use super::{NewThread, OpenSettings, OpenThread, SelectProject};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, Window, div, list, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::Textarea,
    v_flex,
};

/// Agent text is untrusted (a repo can steer it), so its links only open web pages, never
/// `file://` paths or app URL schemes.
pub(super) fn opens_in_browser(url: &str) -> bool {
    let lower = url.trim_start().to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://")
}

impl ThreadView {
    fn render_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let running = self.session.is_some();
        h_flex()
            .px_4()
            .py_2()
            .gap_3()
            .border_b_1()
            .border_color(theme.border)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate()
                            .child(self.title()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .truncate()
                            .text_color(theme.muted_foreground)
                            .child(self.cwd.display().to_string()),
                    )
                    .children(self.parent.clone().map(|(id, title)| {
                        h_flex()
                            .gap_1()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("Delegated by")
                            .child(thread_link("parent-link", id, title))
                    })),
            )
            .children(self.badge().map(|badge| {
                h_flex()
                    .gap_1p5()
                    .px_2()
                    .py_0p5()
                    .rounded_md()
                    .border_1()
                    .border_color(theme.border)
                    .text_xs()
                    .child(div().size(px(6.)).rounded_full().bg(theme.success))
                    .child(badge)
            }))
            .child(self.render_worktree_actions(cx))
            .when(running, |this| {
                this.child(
                    Button::new("interrupt")
                        .outline()
                        .xsmall()
                        .label("Interrupt")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.interrupt(&Interrupt, window, cx)
                        })),
                )
                .child(
                    Button::new("stop")
                        .danger()
                        .xsmall()
                        .label("Stop")
                        .on_click(cx.listener(|this, _, window, cx| this.stop(&Stop, window, cx))),
                )
            })
            .into_any_element()
    }

    /// The threads this one's agent delegated, each a link.
    fn render_children(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.children.is_empty() {
            return None;
        }
        let theme = cx.theme();
        Some(
            h_flex()
                .flex_wrap()
                .px_4()
                .py_1()
                .gap_1()
                .border_b_1()
                .border_color(theme.border)
                .text_xs()
                .child(
                    div()
                        .text_color(theme.muted_foreground)
                        .child(format!("Delegated threads ({})", self.children.len())),
                )
                .children(self.children.iter().map(|c| {
                    thread_link(
                        SharedString::from(format!("child-{}", c.id)),
                        c.id,
                        c.title.clone(),
                    )
                }))
                .into_any_element(),
        )
    }

    fn render_cta(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        v_flex()
            .mx_4()
            .mb_2()
            .p_3()
            .gap_2()
            .rounded_lg()
            .border_1()
            .border_color(theme.warning)
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child("This folder isn't bound to an account"),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(format!(
                        "SkyeLab never picks an account for you. Bind {} to one, then send again.",
                        self.cwd.display()
                    )),
            )
            .child(
                div()
                    .self_start()
                    .debug_selector(|| "bind-cta".into())
                    .child(
                        Button::new("bind-cta")
                            .primary()
                            .small()
                            .label("Bind this folder to an account")
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(OpenSettings), cx)
                            }),
                    ),
            )
            .into_any_element()
    }
}

impl ThreadView {
    fn render_refused(&self, reason: SharedString, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let project_id = self.project_id;
        v_flex()
            .mx_4()
            .mb_2()
            .p_3()
            .gap_2()
            .rounded_lg()
            .border_1()
            .border_color(theme.warning)
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child("This thread can't continue here"),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(reason),
            )
            .child(
                div()
                    .self_start()
                    .debug_selector(|| "new-thread-cta".into())
                    .child(
                        Button::new("new-thread-cta")
                            .primary()
                            .small()
                            .label("Start a new thread")
                            .on_click(move |_, window, cx| {
                                // The sidebar may show another project by now; start in this thread's.
                                window.dispatch_action(
                                    Box::new(SelectProject { id: project_id }),
                                    cx,
                                );
                                window.dispatch_action(Box::new(NewThread), cx);
                            }),
                    ),
            )
            .into_any_element()
    }
}

/// A button that opens thread `id`.
fn thread_link(element_id: impl Into<gpui::ElementId>, id: i64, title: SharedString) -> Button {
    Button::new(element_id)
        .ghost()
        .xsmall()
        .label(title)
        .on_click(move |_, window, cx| window.dispatch_action(Box::new(OpenThread { id }), cx))
}

impl Render for ThreadView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = self.render_header(cx);
        let children = self.render_children(cx);
        let cta = self.needs_binding.then(|| self.render_cta(cx));
        let worktree_option = self.render_worktree_option(cx);
        let discard_confirm = self.render_discard_confirm(cx);
        let refused = self
            .resume_refused
            .clone()
            .map(|r| self.render_refused(r, cx));
        let theme = cx.theme();
        let note = if self.starting {
            Some("Starting the agent…")
        } else if self.thread.is_none() || self.session.is_some() || refused.is_some() {
            None
        } else if self.read_only {
            Some("This thread has ended and has no agent session to continue. Start a new thread.")
        } else {
            Some("The agent has ended. Send a message to continue its session.")
        };
        let body = if self.items.is_empty() && !self.read_only {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(
                    "Write a prompt below. The agent runs in this folder under its bound account.",
                )
                .into_any_element()
        } else {
            list(self.list.clone(), cx.processor(render_item))
                .flex_1()
                .py_2()
                .into_any_element()
        };
        v_flex()
            .id("thread")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::send_prompt))
            .on_action(cx.listener(Self::interrupt))
            .on_action(cx.listener(Self::stop))
            .on_action(cx.listener(Self::toggle_worktree))
            .on_action(cx.listener(Self::open_changes))
            .on_action(cx.listener(Self::open_worktree_terminal))
            .on_action(cx.listener(Self::discard_worktree))
            .on_action(cx.listener(Self::confirm_discard))
            .on_action(cx.listener(Self::cancel_discard))
            .size_full()
            .bg(theme.background)
            .child(header)
            .children(children)
            .child(body)
            .children(cta)
            .children(discard_confirm)
            .children(refused)
            .children(self.error.clone().map(|e| {
                div()
                    .mx_4()
                    .mb_2()
                    .text_xs()
                    .text_color(theme.danger)
                    .child(e)
            }))
            .children(note.map(|n| {
                div()
                    .mx_4()
                    .mb_2()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(n)
            }))
            .children(worktree_option)
            .child(
                h_flex()
                    .px_4()
                    .pb_3()
                    .gap_2()
                    .items_end()
                    .child(div().flex_1().child(Textarea::new(&self.prompt)))
                    .child(
                        Button::new("send")
                            .primary()
                            .small()
                            .label("Send")
                            .disabled(self.read_only || self.starting)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.send_prompt(&SendPrompt, window, cx)
                            })),
                    ),
            )
    }
}
