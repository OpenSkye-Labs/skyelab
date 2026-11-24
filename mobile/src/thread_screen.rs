//! The thread screen: the timeline (the desktop's model, compact rows, finished turns folded), the requests the agent
//! waits on, and the composer.

use super::*;
use crate::thread_rows::status_line;
use crate::timeline_rows::{opens, row_lines};
use gpui::list;
use skyelab_agent::timeline::fold::{fold_head, fold_key, folded_by, rows_of};
use skyelab_agent::timeline::{Record, apply, stamp_done};

impl MobileApp {
    /// Brings the timeline up to date with the open thread's events. Events only ever get inserted in seq order or
    /// cleared (a re-watch), so if the last event applied is still at its index, every new one came after it and is
    /// applied on top, re-laying out only the rows from the first one that changed; anything else rebuilds.
    pub fn sync_items(&mut self) {
        let (Screen::Thread { thread_id }, Some(source)) = (self.screen, &self.source) else {
            return;
        };
        let events = source.events(thread_id);
        let (n, last) = (events.len(), events.last().map(|e| e.seq));
        let applied = match self.shown {
            Some((shown, len, shown_last))
                if shown == thread_id
                    && len <= n
                    && (len == 0 || Some(events[len - 1].seq) == shown_last) =>
            {
                Some(len)
            }
            _ => None,
        };
        let from = applied.unwrap_or_else(|| {
            self.items = Items::default();
            self.usage = Usage::default();
            self.expanded.clear();
            self.drafts.clear();
            self.typed.clear();
            0
        });
        let before = self.items.len();
        let mut first: Option<usize> = None;
        for e in &events[from..] {
            let changed = match Record::from_row(&e.kind, &e.payload_json) {
                Ok(record) => {
                    let changed = apply(&mut self.items, &record);
                    stamp_done(&mut self.items, e.created_at);
                    self.usage.observe(&record);
                    changed
                }
                Err(_) => {
                    self.items.push(Item::Notice {
                        text: format!("An event this app can't read ({})", e.kind),
                        explains_failure: false,
                    });
                    Some(self.items.len() - 1)
                }
            };
            first = match (first, changed) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
        }
        match (applied, first) {
            (None, _) => self.events_list.reset(self.items.len()),
            (Some(_), Some(f)) => self.events_list.splice(f..before, self.items.len() - f),
            (Some(_), None) => {}
        }
        self.shown = Some((thread_id, n, last));
    }

    /// Opens or closes a row, or a turn's fold (`fold:<row>`).
    pub fn toggle(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.expanded.remove(key) {
            self.expanded.insert(key.to_owned());
        }
        if let Some(rows) = rows_of(&self.items, key) {
            let n = rows.len();
            self.events_list.splice(rows, n);
        }
        cx.notify();
    }

    /// Whether row `ix` is folded away in its finished turn.
    pub fn row_hidden(&self, ix: usize) -> bool {
        folded_by(&self.items, ix)
            .is_some_and(|done| !self.expanded.contains(&fold_key(&self.items, done)))
    }

    /// The "Worked for …" header drawn above row `ix`, when it is the first row its turn folds.
    pub fn fold_header(&self, ix: usize) -> Option<String> {
        let done = folded_by(&self.items, ix)?;
        (fold_head(&self.items, done) == Some(ix))
            .then(|| row_lines(&self.items[done], false).remove(0))
    }

    pub(super) fn event_row(&mut self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        self.rows_built += 1;
        if ix >= self.items.len() {
            return div().into_any_element();
        }
        let header = self.fold_header(ix).map(|text| {
            let key = folded_by(&self.items, ix)
                .map(|done| fold_key(&self.items, done))
                .unwrap_or_default();
            let open = self.expanded.contains(&key);
            div()
                .id(SharedString::from(key.clone()))
                .debug_selector(|| "turn-fold".into())
                .min_h(px(TAP))
                .flex()
                .items_center()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(format!("{text} {}", if open { "⌄" } else { "›" }))
                .on_click(cx.listener(move |this, _, _, cx| this.toggle(&key, cx)))
        });
        let body = (!self.row_hidden(ix)).then(|| self.item_body(ix, cx));
        v_flex()
            .w_full()
            .children(header)
            .children(body)
            .into_any_element()
    }

    /// One row as drawn: its lines, styled by kind; rows that open are a tap target.
    fn item_body(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let item = &self.items[ix];
        // A finished turn's divider shows only when no fold header carries its text.
        if let Item::Done {
            is_error: false, ..
        } = item
            && fold_head(&self.items, ix).is_some()
        {
            return div().into_any_element();
        }
        let key = item.expand_id().map(str::to_owned);
        let open = key.as_ref().is_some_and(|k| self.expanded.contains(k));
        let lines = row_lines(item, open);
        let theme = cx.theme();
        let mono = |l: &String| {
            let color = if l.starts_with("+ ") {
                theme.success
            } else if l.starts_with("- ") {
                theme.danger
            } else {
                theme.muted_foreground
            };
            div()
                .text_xs()
                .font_family("Menlo")
                .text_color(color)
                .child(l.clone())
        };
        let mut row = v_flex().w_full().py_1().gap_0p5();
        row = match item {
            Item::Prompt(_) | Item::Steer(_) => row.child(
                div()
                    .p_3()
                    .rounded_lg()
                    .bg(theme.secondary)
                    .child(lines.join("\n")),
            ),
            Item::Text(_) | Item::Live { .. } | Item::Notice { .. } => row.child(lines.join("\n")),
            Item::Approval(a) if a.decision.is_none() => {
                row.child(div().text_sm().text_color(theme.warning).child(format!(
                    "{} · waiting for your answer below",
                    lines.join(" ")
                )))
            }
            Item::Limit { .. } | Item::Done { is_error: true, .. } => row.child(
                div()
                    .text_sm()
                    .text_color(theme.warning)
                    .child(lines.join("\n")),
            ),
            Item::Todo(_) => row.children(lines.iter().map(|l| div().text_sm().child(l.clone()))),
            Item::Tool(_) => {
                let mut lines = lines.iter();
                let header = lines.next().cloned().unwrap_or_default();
                row.child(div().text_sm().line_clamp(2).child(header))
                    .children(lines.map(mono))
            }
            _ => {
                let mut lines = lines.iter();
                let header = lines.next().cloned().unwrap_or_default();
                row.child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(header),
                )
                .children(lines.map(|l| div().text_sm().child(l.clone())))
            }
        };
        match key.filter(|_| opens(item)) {
            Some(key) => div()
                .id(SharedString::from(format!("row-{key}")))
                .min_h(px(TAP))
                .flex()
                .items_center()
                .child(row)
                .on_click(cx.listener(move |this, _, _, cx| this.toggle(&key, cx)))
                .into_any_element(),
            None => row.into_any_element(),
        }
    }

    pub(super) fn thread_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let events = list(
            self.events_list.clone(),
            cx.processor(|this, ix, _, cx| this.event_row(ix, cx)),
        )
        .flex_1()
        .min_h_0();
        let asks = self.ask_cards(cx);
        let status = self.thread().map(|t| {
            let account = t.account_label.unwrap_or(t.account_id);
            format!("{account} · {}", status_line(&t.status, &self.usage))
        });
        let theme = cx.theme();
        v_flex()
            .flex_1()
            .min_h_0()
            .gap_2()
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .children(status),
            )
            .child(events)
            .children(asks)
            .children(
                self.error
                    .clone()
                    .map(|e| div().text_sm().text_color(theme.danger).child(e)),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(div().flex_1().child(Input::new(&self.composer).h(px(TAP))))
                    .child(
                        Button::new("send")
                            .primary()
                            .debug_selector(|| "send".into())
                            .h(px(TAP))
                            .label("Send")
                            .on_click(cx.listener(|this, _, window, cx| this.send(window, cx))),
                    )
                    .child(
                        Button::new("stop")
                            .danger()
                            .h(px(TAP))
                            .label("Stop")
                            .on_click(cx.listener(|this, _, _, cx| this.stop(cx))),
                    ),
            )
            .into_any_element()
    }
}
