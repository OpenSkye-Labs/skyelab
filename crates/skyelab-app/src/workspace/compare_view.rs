//! The Compare page: a multi-model send's threads side by side, one column per model with its cost, time, changed
//! files and latest reply, and "Keep this one". The columns read the threads' own views, so they update as the
//! agents stream; costs come from the store's per-turn rows.

use super::compare::KeepCompare;
use super::thread::{ThreadView, ThreadViewEvent};
use super::thread_model::Item;
use super::{OpenThread, SharedStore, Workspace, lock};
use crate::ui;
use crate::ui::Button;
use gpui::{
    AppContext as _, Context, Entity, FocusHandle, Focusable, FontWeight, SharedString,
    Subscription, Task, WeakEntity, Window, div, prelude::*, px,
};
use gpui_component::{ActiveTheme as _, h_flex, v_flex};
use std::collections::{HashMap, HashSet};

pub struct CompareView {
    pub(super) group: i64,
    columns: Vec<WeakEntity<ThreadView>>,
    /// What each column showed when last drawn: a thread's notify redraws the page only when its column changed.
    drawn: Vec<Column>,
    /// Each thread's cost so far, by thread id (the store's per-turn rows).
    costs: HashMap<i64, f64>,
    store: SharedStore,
    focus: FocusHandle,
    _subs: Vec<Subscription>,
    _load: Option<Task<()>>,
}

/// What a column shows of its thread.
#[derive(Clone, PartialEq)]
struct Column {
    id: Option<i64>,
    title: SharedString,
    model: SharedString,
    working: bool,
    limited: bool,
    /// Why its start or turn failed.
    error: Option<SharedString>,
    turns: usize,
    ms: u64,
    files: usize,
    reply: Option<SharedString>,
}

fn column(v: &ThreadView) -> Column {
    let (mut turns, mut ms) = (0, 0);
    for i in &v.items {
        if let Item::Done { duration_ms, .. } = i {
            turns += 1;
            ms += duration_ms;
        }
    }
    let files: HashSet<&str> = v
        .timeline
        .files
        .by_turn
        .values()
        .flatten()
        .map(|f| f.rel.as_str())
        .collect();
    Column {
        id: v.thread.as_ref().map(|t| t.id),
        title: v.title(),
        model: v.chip_label(false).0.into(),
        working: v.working(),
        limited: v.live_status() == crate::ui::Status::Limited,
        error: v.error.clone(),
        turns,
        ms,
        files: files.len(),
        reply: v.items.iter().rev().find_map(|i| match i {
            Item::Text(t) => Some(t.clone().into()),
            _ => None,
        }),
    }
}

impl CompareView {
    fn set_columns(&mut self, views: Vec<Entity<ThreadView>>, cx: &mut Context<Self>) {
        self._subs = views
            .iter()
            .enumerate()
            .flat_map(|(ix, v)| {
                [
                    // A view notifies on every delta, keystroke and tick; redraw only for what a column shows.
                    cx.observe(v, move |this, v, cx| {
                        let now = column(v.read(cx));
                        if this.drawn.get(ix) != Some(&now) {
                            this.drawn[ix] = now;
                            cx.notify();
                        }
                    }),
                    cx.subscribe(v, |this, _, e: &ThreadViewEvent, cx| {
                        if matches!(e, ThreadViewEvent::TurnDone | ThreadViewEvent::Started(_)) {
                            this.load_costs(cx);
                        }
                    }),
                ]
            })
            .collect();
        self.columns = views.iter().map(Entity::downgrade).collect();
        self.drawn = views.iter().map(|v| column(v.read(cx))).collect();
        self.load_costs(cx);
    }

    fn load_costs(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<i64> = self
            .columns
            .iter()
            .filter_map(|v| v.upgrade()?.read(cx).thread.as_ref().map(|t| t.id))
            .collect();
        let store = self.store.clone();
        let task = cx.background_spawn(async move { lock(&store)?.thread_costs(&ids) });
        self._load = Some(cx.spawn(async move |this, cx| {
            if let Ok(costs) = task.await {
                let _ = this.update(cx, |this, cx| {
                    this.costs = costs;
                    cx.notify();
                });
            }
        }));
    }
}

impl Focusable for CompareView {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

fn usd(v: f64) -> String {
    format!("${v:.4}")
}

impl Render for CompareView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let cols: Vec<Column> = (self.drawn.iter().zip(&self.columns))
            .filter(|(_, v)| v.upgrade().is_some())
            .map(|(c, _)| c.clone())
            .collect();
        let theme = cx.theme();
        let busy = cols.iter().any(|c| c.working);
        let title = cols.first().map(|c| c.title.clone()).unwrap_or_default();
        // The estimate, once every model has finished: the store's per-turn costs, summed.
        let total = (!busy && !cols.is_empty()).then(|| {
            let sum: f64 = cols.iter().filter_map(|c| self.costs.get(&c.id?)).sum();
            format!("Total {} · {} models", usd(sum), cols.len())
        });
        let header = super::layout::page_header(window, cx)
            .child(
                div()
                    .text_size(ui::BODY)
                    .text_color(theme.muted_foreground)
                    .child("Compare"),
            )
            .child(div().text_color(theme.muted_foreground).child("/"))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(ui::BODY)
                    .font_weight(FontWeight::MEDIUM)
                    .child(title),
            )
            .child(
                div()
                    .id("compare-total")
                    .text_size(ui::META)
                    .text_color(theme.muted_foreground)
                    .child(total.unwrap_or_else(|| "Running…".into())),
            );
        let cards = cols.into_iter().enumerate().map(|(ix, c)| {
            let (dot, status) = if c.working {
                (theme.info, "Working")
            } else if c.error.is_some() {
                (theme.danger, "Failed")
            } else if c.limited {
                (theme.warning, "Limited")
            } else if c.turns > 0 {
                (theme.success, "Done")
            } else {
                (theme.muted_foreground, "Waiting")
            };
            let cost =
                c.id.and_then(|id| self.costs.get(&id))
                    .map_or("—".to_owned(), |v| usd(*v));
            let files = match c.files {
                1 => "1 file".to_owned(),
                n => format!("{n} files"),
            };
            let meta = format!("{cost} · {:.1}s · {files}", c.ms as f64 / 1000.);
            let key =
                c.id.map_or_else(|| format!("draft-{ix}"), |id| id.to_string());
            v_flex()
                .id(SharedString::from(format!("compare-col-{key}")))
                .flex_1()
                .min_w(px(220.))
                .h_full()
                .rounded(ui::RADIUS_LG)
                .border_1()
                .border_color(theme.border)
                .bg(theme.popover)
                .child(
                    v_flex()
                        .px_3()
                        .py_2()
                        .gap_0p5()
                        .border_b_1()
                        .border_color(theme.border)
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(ui::StatusDot::new(dot))
                                .child(
                                    div()
                                        .flex_1()
                                        .truncate()
                                        .text_size(ui::BODY)
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(c.model),
                                )
                                .child(
                                    div()
                                        .text_size(ui::META)
                                        .text_color(theme.muted_foreground)
                                        .child(status),
                                ),
                        )
                        .child(
                            div()
                                .text_size(ui::META)
                                .text_color(theme.muted_foreground)
                                .child(meta),
                        ),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("compare-reply-{key}")))
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .px_3()
                        .py_2()
                        .text_size(ui::BODY)
                        .child(
                            c.reply
                                .or(c.error)
                                .unwrap_or_else(|| "No reply yet.".into()),
                        ),
                )
                .child(
                    h_flex()
                        .px_3()
                        .py_2()
                        .gap_2()
                        .justify_end()
                        .border_t_1()
                        .border_color(theme.border)
                        .children(c.id.map(|id| {
                            Button::new(SharedString::from(format!("compare-open-{id}")))
                                .ghost()
                                .small()
                                .label("Open")
                                .on_click(move |_, window, cx| {
                                    window.dispatch_action(Box::new(OpenThread { id }), cx)
                                })
                        }))
                        .children(c.id.map(|id| {
                            Button::new(SharedString::from(format!("compare-keep-{id}")))
                                .primary()
                                .small()
                                .label("Keep this one")
                                .disabled(busy)
                                .tooltip(if busy {
                                    "Wait for every model to finish"
                                } else {
                                    "Archive or leave the others"
                                })
                                .on_click(move |_, window, cx| {
                                    window.dispatch_action(Box::new(KeepCompare { id }), cx)
                                })
                        })),
                )
        });
        v_flex()
            .id("compare-page")
            .track_focus(&self.focus)
            .size_full()
            .bg(theme.background)
            .child(header)
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .p_5()
                    .gap_3()
                    .items_start()
                    .children(cards),
            )
    }
}

impl Workspace {
    /// The open views of comparison `group`'s threads: drafts still starting, and stored rows, oldest first.
    pub(super) fn group_views(&self, group: i64, cx: &gpui::App) -> Vec<Entity<ThreadView>> {
        let mut views: Vec<Entity<ThreadView>> = self
            .open_threads
            .iter()
            .filter(|v| {
                let v = v.read(cx);
                v.compare_group == Some(group)
                    || v.thread.as_ref().and_then(|t| t.compare_group) == Some(group)
            })
            .cloned()
            .collect();
        views.sort_by_key(|v| v.read(cx).thread.as_ref().map_or(i64::MAX, |t| t.id));
        views
    }

    pub(super) fn group_alive(&self, group: i64, cx: &gpui::App) -> bool {
        !self.group_views(group, cx).is_empty() || !self.group_members(group, cx).is_empty()
    }

    /// The Compare page for `group`, opening (in the background) any of its threads that isn't open yet.
    pub(super) fn compare_page(
        &mut self,
        group: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<CompareView> {
        for id in self.group_members(group, cx) {
            if self.open_view(id, cx).is_none()
                && let Some(t) = self.threads.iter().find(|t| t.id == id).cloned()
            {
                let (config, backend) = (self.config.clone(), self.backend.clone());
                let view = cx.new(|cx| ThreadView::open(t, config, backend, window, cx));
                self.add_thread_tab(view, false, window, cx);
            }
        }
        let views = self.group_views(group, cx);
        let page = match self
            .compare
            .view
            .clone()
            .filter(|v| v.read(cx).group == group)
        {
            Some(page) => page,
            None => {
                let store = self.backend.store.clone();
                let page = cx.new(|cx| CompareView {
                    group,
                    columns: Vec::new(),
                    drawn: Vec::new(),
                    costs: HashMap::new(),
                    store,
                    focus: cx.focus_handle(),
                    _subs: Vec::new(),
                    _load: None,
                });
                self.compare.view = Some(page.clone());
                page
            }
        };
        page.update(cx, |p, cx| p.set_columns(views, cx));
        let focus = page.read(cx).focus.clone();
        focus.focus(window, cx);
        page
    }
}
