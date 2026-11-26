//! The mobile thread list screen: active flat sections plus collapsible Snoozed and Settled shelves, with >= 44 pt
//! touch targets for opening rows, folding shelves, settling, snoozing with presets, and waking/restoring threads.

use super::*;
use crate::thread_rows::{now_ms, partition, row_detail, snooze_badge};
use skyelab_core::snooze;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowShelfKind {
    Active,
    Snoozed,
    Settled,
}

/// A section title in the thread list.
fn section_title(title: &'static str, cx: &App) -> impl IntoElement {
    div()
        .pt_3()
        .pb_1()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(cx.theme().muted_foreground)
        .child(title.to_uppercase())
}

impl MobileApp {
    /// Whether the connected machine takes `shelve_thread` (protocol 1.6) over a live, approved connection.
    pub fn can_shelve(&self) -> bool {
        self.source.as_ref().is_some_and(|s| {
            s.connection() == Connection::Connected
                && s.speaks(skyelab_protocol::SHELVES_SINCE_MINOR)
        })
    }

    /// Friendly reason shelving isn't available right now (offline or an older desktop < 1.6).
    pub fn shelve_unavailable_reason(&self) -> String {
        let host = self
            .source
            .as_ref()
            .and_then(|s| s.host())
            .unwrap_or("that machine");
        let connected = self
            .source
            .as_ref()
            .is_some_and(|s| s.connection() == Connection::Connected);
        if connected {
            format!(
                "{host} runs an older SkyeLab without settle and snooze: update SkyeLab on {host}"
            )
        } else {
            format!("{host} is offline. Settle and snooze its threads once it is back.")
        }
    }

    /// Expands or collapses the inline shelf/snooze actions beneath `thread_id`.
    pub fn toggle_shelf_actions(&mut self, thread_id: i64, cx: &mut Context<Self>) {
        self.shelf_actions_for = (self.shelf_actions_for != Some(thread_id)).then_some(thread_id);
        cx.notify();
    }

    pub fn toggle_snoozed_shelf(&mut self, cx: &mut Context<Self>) {
        self.show_snoozed = !self.show_snoozed;
        cx.notify();
    }

    pub fn toggle_settled_shelf(&mut self, cx: &mut Context<Self>) {
        self.show_settled = !self.show_settled;
        cx.notify();
    }

    /// Puts `thread_id` on a shelf (settled, snoozed until Unix ms, or neither). Refuses locally when offline or
    /// connected to a < 1.6 machine, setting the friendly message in `self.error`.
    pub fn shelve(
        &mut self,
        thread_id: i64,
        settled: bool,
        snoozed_until: Option<i64>,
        cx: &mut Context<Self>,
    ) {
        self.shelf_actions_for = None;
        if !self.can_shelve() {
            self.error = Some(self.shelve_unavailable_reason().into());
            cx.notify();
            return;
        }
        self.error = None;
        if let Some(source) = self.source.as_mut() {
            source.shelve_thread(thread_id, settled, snoozed_until);
        }
        cx.notify();
    }

    pub fn settle_thread(&mut self, thread_id: i64, cx: &mut Context<Self>) {
        self.shelve(thread_id, true, None, cx);
    }

    pub fn snooze_thread(&mut self, thread_id: i64, until_ms: i64, cx: &mut Context<Self>) {
        self.shelve(thread_id, false, Some(until_ms), cx);
    }

    pub fn wake_thread(&mut self, thread_id: i64, cx: &mut Context<Self>) {
        self.shelve(thread_id, false, None, cx);
    }

    /// The flat list: active sections, then collapsible Snoozed and Settled shelves.
    pub(super) fn thread_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let projects = self.projects();
        let starts = skyelab_agent::thread_list::day_starts();
        let now = now_ms();
        let lists = partition(self.threads(), starts, now);
        let mut list = v_flex().id("threads").size_full().overflow_y_scroll();
        if let Some(err) = self.error.clone() {
            list = list.child(div().text_sm().text_color(cx.theme().danger).child(err));
        }
        for (title, threads) in lists.active {
            list = list.child(section_title(title, cx));
            for t in threads {
                list = list.child(self.thread_row(t, &projects, RowShelfKind::Active, now, cx));
            }
        }
        if !lists.snoozed.is_empty() {
            list = list.child(self.shelf_header(
                "shelf-fold-snoozed",
                "Snoozed",
                lists.snoozed.len(),
                self.show_snoozed,
                cx.listener(|this, _, _, cx| this.toggle_snoozed_shelf(cx)),
                cx,
            ));
            if self.show_snoozed {
                for t in lists.snoozed {
                    list =
                        list.child(self.thread_row(t, &projects, RowShelfKind::Snoozed, now, cx));
                }
            }
        }
        if !lists.settled.is_empty() {
            list = list.child(self.shelf_header(
                "shelf-fold-settled",
                "Settled",
                lists.settled.len(),
                self.show_settled,
                cx.listener(|this, _, _, cx| this.toggle_settled_shelf(cx)),
                cx,
            ));
            if self.show_settled {
                for t in lists.settled {
                    list =
                        list.child(self.thread_row(t, &projects, RowShelfKind::Settled, now, cx));
                }
            }
        }
        list.into_any_element()
    }

    fn shelf_header(
        &self,
        id: &'static str,
        title: &'static str,
        count: usize,
        open: bool,
        on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
        cx: &App,
    ) -> AnyElement {
        let arrow = if open { "⌄" } else { "›" };
        div()
            .id(id)
            .debug_selector(|| "shelf-fold".into())
            .min_h(px(TAP))
            .flex()
            .items_center()
            .pt_2()
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(cx.theme().muted_foreground)
            .child(format!("{} ({count}) {arrow}", title.to_uppercase()))
            .on_click(on_click)
            .into_any_element()
    }

    fn thread_row(
        &self,
        t: Thread,
        projects: &[Project],
        kind: RowShelfKind,
        now: i64,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = t.id;
        let project = projects.iter().find(|p| p.id == t.project_id);
        let detail = row_detail(&t, project);
        let badge = snooze_badge(&t, now);
        let expanded = self.shelf_actions_for == Some(id);
        let theme = cx.theme();
        let dot = match t.status.as_str() {
            "starting" | "running" => theme.info,
            "idle" | "done" => theme.success,
            "failed" => theme.danger,
            "interrupted" => theme.warning,
            _ => theme.muted_foreground,
        };
        let quick_action: Option<AnyElement> = match kind {
            RowShelfKind::Active => None,
            RowShelfKind::Snoozed => Some(
                Button::new(SharedString::from(format!("wake-{id}")))
                    .ghost()
                    .debug_selector(|| "shelf-wake".into())
                    .h(px(TAP))
                    .min_w(px(TAP))
                    .label("Wake")
                    .on_click(cx.listener(move |this, _, _, cx| this.wake_thread(id, cx)))
                    .into_any_element(),
            ),
            RowShelfKind::Settled => Some(
                Button::new(SharedString::from(format!("restore-{id}")))
                    .ghost()
                    .debug_selector(|| "shelf-restore".into())
                    .h(px(TAP))
                    .min_w(px(TAP))
                    .label("Restore")
                    .on_click(cx.listener(move |this, _, _, cx| this.wake_thread(id, cx)))
                    .into_any_element(),
            ),
        };
        let toggle = Button::new(SharedString::from(format!("shelf-toggle-{id}")))
            .ghost()
            .debug_selector(|| "shelf-toggle".into())
            .h(px(TAP))
            .min_w(px(TAP))
            .label(if expanded { "✕" } else { "⋯" })
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_shelf_actions(id, cx)));

        let main = div()
            .id(("thread", id as u64))
            .debug_selector(|| "thread-row".into())
            .flex_1()
            .min_w_0()
            .min_h(px(TAP))
            .py_2()
            .child(
                h_flex()
                    .gap_2()
                    .child(div().size_2().rounded_full().bg(dot))
                    .child(div().flex_1().text_base().line_clamp(1).child(t.title)),
            )
            .children(badge.map(|b| div().pl_4().text_xs().text_color(theme.warning).child(b)))
            .child(
                div()
                    .pl_4()
                    .text_sm()
                    .line_clamp(3)
                    .text_color(theme.muted_foreground)
                    .child(detail),
            )
            .on_click(cx.listener(move |this, _, window, cx| this.open_thread(id, window, cx)));

        v_flex()
            .border_b_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .items_center()
                    .gap_1()
                    .child(main)
                    .children(quick_action)
                    .child(toggle),
            )
            .children(expanded.then(|| self.shelf_action_bar(id, kind, cx)))
            .into_any_element()
    }

    fn shelf_action_bar(&self, id: i64, kind: RowShelfKind, cx: &mut Context<Self>) -> AnyElement {
        if !self.can_shelve() {
            return div()
                .pb_2()
                .pl_4()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(self.shelve_unavailable_reason())
                .into_any_element();
        }
        let mut bar = h_flex().flex_wrap().gap_2().pb_2().pl_4();
        match kind {
            RowShelfKind::Active => {
                bar = bar.child(
                    Button::new(SharedString::from(format!("settle-{id}")))
                        .debug_selector(|| "shelf-settle".into())
                        .h(px(TAP))
                        .label("Settle")
                        .on_click(cx.listener(move |this, _, _, cx| this.settle_thread(id, cx))),
                );
                bar = self.add_snooze_presets(bar, id, cx);
            }
            RowShelfKind::Snoozed => {
                bar = bar
                    .child(
                        Button::new(SharedString::from(format!("bar-wake-{id}")))
                            .primary()
                            .debug_selector(|| "shelf-wake".into())
                            .h(px(TAP))
                            .label("Wake now")
                            .on_click(cx.listener(move |this, _, _, cx| this.wake_thread(id, cx))),
                    )
                    .child(
                        Button::new(SharedString::from(format!("settle-{id}")))
                            .debug_selector(|| "shelf-settle".into())
                            .h(px(TAP))
                            .label("Settle")
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.settle_thread(id, cx)),
                            ),
                    );
            }
            RowShelfKind::Settled => {
                bar = bar.child(
                    Button::new(SharedString::from(format!("bar-restore-{id}")))
                        .primary()
                        .debug_selector(|| "shelf-restore".into())
                        .h(px(TAP))
                        .label("Restore")
                        .on_click(cx.listener(move |this, _, _, cx| this.wake_thread(id, cx))),
                );
                bar = self.add_snooze_presets(bar, id, cx);
            }
        }
        bar.into_any_element()
    }

    fn add_snooze_presets(&self, mut bar: gpui::Div, id: i64, cx: &mut Context<Self>) -> gpui::Div {
        for (ix, choice) in snooze::presets(&chrono::Local::now())
            .into_iter()
            .enumerate()
        {
            let until = choice.until_ms;
            bar = bar.child(
                Button::new(SharedString::from(format!("snooze-{id}-{ix}")))
                    .debug_selector(|| "shelf-snooze".into())
                    .h(px(TAP))
                    .label(choice.label)
                    .on_click(cx.listener(move |this, _, _, cx| this.snooze_thread(id, until, cx))),
            );
        }
        bar
    }
}
