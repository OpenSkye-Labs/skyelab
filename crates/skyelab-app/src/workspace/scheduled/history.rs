//! A bot card's run history (T3's delivery log): one line per run, newest first: its status, what started it, the
//! GitHub item, when, why it didn't run, and its thread.

use super::page::{HISTORY_PAGE, ScheduledPage};
use crate::ui;
use crate::workspace::OpenThread;
use gpui::{
    AnyElement, Context, IntoElement, ParentElement as _, SharedString, Styled as _, div,
    prelude::*,
};
use gpui_component::{ActiveTheme as _, h_flex, v_flex};
use skyelab_core::bots::Trigger;
use skyelab_store::scheduled::BotRun;

fn trigger_label(t: &str) -> &'static str {
    match t {
        "schedule" => "Schedule",
        "run_now" => "Run now",
        t => Trigger::parse(t).map_or("Unknown", Trigger::label),
    }
}

/// `#42` for a canonical GitHub item link.
fn item_number(url: &str) -> Option<String> {
    url.rsplit('/')
        .next()
        .filter(|n| n.parse::<u64>().is_ok())
        .map(|n| format!("#{n}"))
}

impl ScheduledPage {
    pub(super) fn render_history(
        &self,
        id: i64,
        runs: &[BotRun],
        now: i64,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let lines: Vec<AnyElement> = runs
            .iter()
            .map(|r| {
                let color = match r.status.as_str() {
                    "started" => theme.success,
                    "queued" => theme.info,
                    "paused" | "failed" => theme.danger,
                    _ => theme.warning,
                };
                let status = ui::Pill::new(r.status.replace('_', " ")).color(color);
                let item = r.item_url.as_deref().and_then(item_number);
                let thread = r.thread_id.map(|thread| {
                    div()
                        .id(SharedString::from(format!("bot-run-thread-{}", r.id)))
                        .underline()
                        .cursor_pointer()
                        .child("Open thread")
                        .on_click(move |_, window, cx| {
                            crate::workspace::dispatch(
                                Box::new(OpenThread { id: thread }),
                                window,
                                cx,
                            )
                        })
                });
                h_flex()
                    .id(SharedString::from(format!("bot-run-{}", r.id)))
                    .flex_wrap()
                    .gap_2()
                    .py_1p5()
                    .text_size(ui::META)
                    .text_color(muted)
                    .child(status)
                    .child(
                        div()
                            .text_color(theme.foreground)
                            .child(trigger_label(&r.trigger).to_owned()),
                    )
                    .children(item)
                    .child(ui::ago(r.at, now))
                    .children(r.error.clone())
                    .children(thread)
                    .into_any_element()
            })
            .collect();
        let more = runs.len() as i64 >= self.expanded.get(&id).copied().unwrap_or(HISTORY_PAGE);
        v_flex()
            .id(SharedString::from(format!("bot-history-{id}")))
            .debug_selector(move || format!("bot-history-{id}"))
            .px_4()
            .pb_2()
            .border_t_1()
            .border_color(theme.border)
            .when(lines.is_empty(), |this| {
                this.child(
                    div()
                        .py_2()
                        .text_size(ui::META)
                        .text_color(muted)
                        .child("No runs yet."),
                )
            })
            .children(lines)
            .when(more, |this| {
                this.child(
                    ui::Button::new(SharedString::from(format!("bot-history-more-{id}")))
                        .ghost()
                        .xsmall()
                        .label("Show more")
                        .on_click(cx.listener(move |this, _, _, cx| this.show_more(id, cx))),
                )
            })
            .into_any_element()
    }
}
