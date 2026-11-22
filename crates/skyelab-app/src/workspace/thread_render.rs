//! How a thread tab looks. State and behavior live in `thread.rs`.

use super::thread::{CONTEXT, Interrupt, SendPrompt, Stop, ThreadView};
use super::thread_model::{Item, ToolItem};
use super::{NewThread, OpenSettings, SelectProject};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, Window, div, list, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::Textarea,
    text::TextView,
    v_flex,
};

/// Longest tool input or result shown when a row is expanded; the store keeps the whole text.
const TOOL_PREVIEW: usize = 4000;

fn clip(s: &str) -> String {
    match s.char_indices().nth(TOOL_PREVIEW) {
        Some((at, _)) => format!("{}\n…", &s[..at]),
        None => s.to_owned(),
    }
}

/// Agent text is untrusted (a repo can steer it), so its links only open web pages, never
/// `file://` paths or app URL schemes.
pub(super) fn opens_in_browser(url: &str) -> bool {
    let lower = url.trim_start().to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://")
}

impl ThreadView {
    fn render_item(&mut self, ix: usize, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let Some(item) = self.items.get(ix) else {
            return div().into_any_element();
        };
        let row = div().w_full().px_4().py_1p5();
        match item {
            Item::Prompt(text) => row
                .child(
                    div()
                        .ml_auto()
                        .max_w(px(560.))
                        .px_3()
                        .py_2()
                        .rounded_lg()
                        .bg(theme.secondary)
                        .text_sm()
                        .child(text.clone()),
                )
                .flex()
                .into_any_element(),
            Item::Text(text) => row
                .child(
                    TextView::markdown(("md", ix), text.clone())
                        .selectable(true)
                        .on_link_click(|url, _, _, cx| {
                            if opens_in_browser(url) {
                                cx.open_url(url);
                            }
                        }),
                )
                .into_any_element(),
            Item::Tool(tool) => row.child(self.render_tool(tool, cx)).into_any_element(),
            Item::Done {
                is_error,
                duration_ms,
                cost_usd,
            } => {
                let verb = if *is_error { "Failed after" } else { "Done in" };
                let secs = *duration_ms as f64 / 1000.;
                row.child(
                    div()
                        .text_xs()
                        .text_color(if *is_error {
                            theme.danger
                        } else {
                            theme.muted_foreground
                        })
                        .child(format!("{verb} {secs:.1}s · ${cost_usd:.4}")),
                )
                .into_any_element()
            }
        }
    }

    fn render_tool(&self, tool: &ToolItem, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let open = self.is_expanded(&tool.id);
        let summary: String = tool
            .input
            .lines()
            .find(|l| l.contains(':'))
            .unwrap_or_default()
            .trim()
            .trim_end_matches(',')
            .chars()
            .take(80)
            .collect();
        let state = match (&tool.result, tool.is_error) {
            (None, _) => "running",
            (Some(_), true) => "error",
            (Some(_), false) => "",
        };
        let id = tool.id.clone();
        let header = h_flex()
            .id(SharedString::from(format!("tool-{}", tool.id)))
            .gap_2()
            .px_2()
            .py_1()
            .rounded_md()
            .cursor_pointer()
            .text_sm()
            .hover(|s| s.bg(theme.secondary))
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_tool(&id, cx)))
            .child(
                div()
                    .text_color(theme.muted_foreground)
                    .child(if open { "▾" } else { "▸" }),
            )
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .child(tool.name.clone()),
            )
            .child(
                div()
                    .flex_1()
                    .truncate()
                    .text_color(theme.muted_foreground)
                    .child(summary),
            )
            .child(div().text_xs().text_color(theme.warning).child(state));
        let block = |label: &'static str, text: String| {
            v_flex()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(label),
                )
                .child(
                    div()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .bg(theme.muted)
                        .font_family(theme.mono_font_family.clone())
                        .text_xs()
                        .child(text),
                )
        };
        v_flex()
            .w_full()
            .rounded_lg()
            .border_1()
            .border_color(theme.border)
            .child(header)
            .when(open, |this| {
                this.child(
                    v_flex()
                        .gap_2()
                        .px_3()
                        .pb_2()
                        .child(block("Input", clip(&tool.input)))
                        .children(tool.result.as_deref().map(|r| block("Result", clip(r)))),
                )
            })
            .into_any_element()
    }

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
                    ),
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

impl Render for ThreadView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = self.render_header(cx);
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
            list(
                self.list.clone(),
                cx.processor(|this, ix, window, cx| this.render_item(ix, window, cx)),
            )
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
