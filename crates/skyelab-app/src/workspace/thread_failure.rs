//! A failed turn's banner over the composer (T3's ThreadErrorBanner): what went wrong, Retry (the last prompt again)
//! and View output (the agent's stderr row). A signed-out account says so by name and points at Settings → Accounts;
//! it never offers another account.

use super::OpenSettingsSection;
use super::thread::ThreadView;
use super::thread_model::Item;
use crate::ui;
use gpui::{AnyElement, Context, FontWeight, Window, div, prelude::*};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};

#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    /// The row that failed (the error divider, or the stderr row of a crash): closing the banner hides it until
    /// another turn fails.
    pub row: usize,
    pub text: String,
    /// The stderr row's id, when there is one to show.
    pub output: Option<String>,
    /// The agent says its account isn't signed in.
    pub signed_out: bool,
    /// The prompt Retry sends again.
    pub prompt: String,
}

/// What a signed-out Claude Code or Codex says.
fn says_signed_out(text: &str) -> bool {
    let t = text.to_lowercase();
    [
        "not logged in",
        "please run /login",
        "invalid api key",
        "authentication_error",
        "oauth token has expired",
        "401",
    ]
    .iter()
    .any(|s| t.contains(s))
}

/// The first line worth showing of the agent's stderr.
fn first_line(text: &str) -> Option<&str> {
    text.lines().map(str::trim).find(|l| !l.is_empty())
}

/// The last turn's failure, if it failed: an error result (an interrupt isn't one), or stderr after its last result (the
/// agent exited). `None` while nothing was sent.
pub fn failure(items: &[Item]) -> Option<Failure> {
    let start = items.iter().rposition(|i| matches!(i, Item::Prompt(_)))?;
    let Item::Prompt(prompt) = &items[start] else {
        return None;
    };
    let tail = &items[start..];
    let output = tail.iter().rev().find_map(|i| match i {
        Item::Output { id, text } => Some((id.clone(), text.as_str())),
        _ => None,
    });
    let done = tail.iter().rposition(|i| matches!(i, Item::Done { .. }));
    let out_at = tail.iter().rposition(|i| matches!(i, Item::Output { .. }));
    // The agent exited after its last result (or before any): its stderr says why.
    let crashed = out_at.is_some() && out_at > done;
    let error = done.and_then(|d| match &tail[d] {
        Item::Done {
            is_error: true,
            error,
            ..
        } => Some(error.as_deref().unwrap_or_default()),
        _ => None,
    });
    if error.is_some_and(|e| e.trim() == "interrupted") || (error.is_none() && !crashed) {
        return None;
    }
    let row = start + if crashed { out_at } else { done }.unwrap_or_default();
    let out = output.as_ref().map(|(_, t)| *t);
    let text = error
        .filter(|e| !e.trim().is_empty())
        .or(out.and_then(first_line))
        .unwrap_or("The agent's turn failed.")
        .to_owned();
    Some(Failure {
        row,
        signed_out: says_signed_out(&text) || out.is_some_and(says_signed_out),
        text,
        output: output.map(|(id, _)| id),
        prompt: prompt.clone(),
    })
}

impl ThreadView {
    /// The failure the banner shows: the last turn's, once no turn runs, unless the user closed it.
    pub(super) fn shown_failure(&self) -> Option<Failure> {
        if self.turn_running() || self.starting {
            return None;
        }
        failure(&self.items).filter(|f| self.timeline.dismissed_failure != Some(f.row))
    }

    pub(super) fn retry_failed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(f) = self.shown_failure() else {
            return;
        };
        if let Err(why) = self.submit_remote(&f.prompt, window, cx) {
            self.error = Some(why.into());
            cx.notify();
        }
    }

    /// Opens the stderr row and scrolls to it.
    pub(super) fn view_failure_output(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.shown_failure().and_then(|f| f.output) else {
            return;
        };
        if !self.is_expanded(&id) {
            self.toggle_tool(&id, cx);
        }
        if let Some(ix) = self
            .items
            .iter()
            .position(|i| i.expand_id() == Some(id.as_str()))
        {
            self.list.scroll_to_reveal_item(ix);
        }
        cx.notify();
    }

    /// "Account Work isn't signed in" names the thread's own account: the banner never suggests another.
    pub(super) fn failure_title(&self, f: &Failure) -> String {
        if !f.signed_out {
            return "The agent's turn failed".to_owned();
        }
        let label = self.thread.as_ref().map_or_else(String::new, |t| {
            let account = self.config.accounts.iter().find(|a| a.id == t.account_id);
            account.map_or(t.account_id.clone(), |a| a.label.clone())
        });
        format!("Account {label} isn't signed in")
    }

    pub(super) fn render_failure(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let f = self.shown_failure()?;
        let title = self.failure_title(&f);
        // What the agent said, then (signed out) what to do about it, on its own line.
        let hint = f.signed_out.then_some(
            "Sign in to this account again in Settings → Accounts, then retry. This folder only runs on its bound \
             account.",
        );
        let detail = f.text.clone();
        let row = f.row;
        let actions = h_flex()
            .gap_2()
            .child(
                Button::new("failure-retry")
                    .primary()
                    .small()
                    .label("Retry")
                    .on_click(cx.listener(|this, _, window, cx| this.retry_failed(window, cx))),
            )
            .when(f.signed_out, |el| {
                el.child(
                    Button::new("failure-settings")
                        .outline()
                        .small()
                        .label("Open Settings")
                        .on_click(|_, window, cx| {
                            window.dispatch_action(
                                Box::new(OpenSettingsSection {
                                    section: crate::settings::Section::Accounts,
                                }),
                                cx,
                            );
                        }),
                )
            })
            .when(f.output.is_some(), |el| {
                el.child(
                    Button::new("failure-output")
                        .ghost()
                        .small()
                        .label("View output")
                        .on_click(cx.listener(|this, _, _, cx| this.view_failure_output(cx))),
                )
            });
        let theme = cx.theme();
        let ink = ui::ink(theme, theme.danger);
        Some(
            h_flex()
                .debug_selector(|| "failure-banner".into())
                .w_full()
                .max_w(ui::CHAT_MAX_W)
                .mb_2()
                .px_4()
                .py_3()
                .gap_3()
                .items_start()
                .rounded(ui::RADIUS_LG)
                .bg(theme.danger.opacity(0.08))
                .border_1()
                .border_color(theme.danger.opacity(0.35))
                .child(
                    Icon::new(IconName::CircleAlert)
                        .size_4()
                        .mt_0p5()
                        .text_color(ink),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .child(
                            div()
                                .text_size(ui::BODY)
                                .font_weight(FontWeight::MEDIUM)
                                .child(title),
                        )
                        .child(
                            div()
                                .debug_selector(|| "failure-text".into())
                                .text_size(ui::META)
                                .text_color(theme.muted_foreground)
                                .child(detail),
                        )
                        .children(hint.map(|h| {
                            div()
                                .text_size(ui::META)
                                .text_color(theme.muted_foreground)
                                .child(h)
                        }))
                        .child(div().pt_1().child(actions)),
                )
                .child(
                    Button::new("failure-dismiss")
                        .ghost()
                        .xsmall()
                        .icon(IconName::Close)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.timeline.dismissed_failure = Some(row);
                            cx.notify();
                        })),
                )
                .into_any_element(),
        )
    }
}
