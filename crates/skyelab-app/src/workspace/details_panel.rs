//! The right dock's Details tab: a card stack for the shown thread (see `thread_details.rs`) with every account's
//! usage in it. It is always in the dock, so the right-panel toggle opens it.

use super::thread::ThreadView;
use super::thread_model::Record;
use crate::ui;
use gpui::{
    AnyElement, App, Context, EventEmitter, FocusHandle, Focusable, FontWeight, IntoElement,
    Render, Subscription, WeakEntity, Window, div, prelude::*,
};
use gpui_component::{
    ActiveTheme as _, Theme,
    dock::{Panel, PanelEvent},
    h_flex, v_flex,
};
use skyelab_core::{Account, Provider};
use skyelab_store::Store;

/// What SkyeLab last saw of one account's limits: the newest reading of each window and when it came.
#[derive(Debug, Clone, PartialEq)]
pub struct AccountUsage {
    pub id: String,
    pub label: String,
    pub provider: Provider,
    pub windows: Vec<(&'static str, f32)>,
    /// When the newest reading was stored (`None`: nothing reported yet).
    pub as_of_ms: Option<i64>,
}

/// Readings older than this many windows' worth of events aren't looked for.
const RECENT_EVENTS: usize = 50;

/// `account`'s newest stored reading of each usage window, from the `rate_limit_event` lines its threads' agents
/// wrote. Only what was reported: nothing is asked of the account, and a store error reads as no data.
pub fn account_usage(store: &Store, account: &Account) -> AccountUsage {
    let events = store
        .account_events_matching(&account.id, "agent", "\"used\":", RECENT_EVENTS)
        .unwrap_or_default();
    let mut windows: Vec<(&'static str, f32)> = Vec::new();
    let mut as_of_ms = None;
    for e in &events {
        let Ok(Record::Agent(event)) = Record::from_row(&e.kind, &e.payload_json) else {
            continue;
        };
        if let Some((label, used)) = event.rate_limit()
            && !windows.iter().any(|(l, _)| *l == label)
        {
            windows.push((label, used));
            as_of_ms = as_of_ms.max(Some(e.created_at));
        }
    }
    // Session before Weekly, like the CLI's /usage.
    windows.sort_by_key(|(label, _)| *label != "Session");
    AccountUsage {
        id: account.id.clone(),
        label: account.label.clone(),
        provider: account.provider,
        windows,
        as_of_ms,
    }
}

pub struct DetailsPanel {
    focus: FocusHandle,
    thread: Option<WeakEntity<ThreadView>>,
    pub(super) usage: Vec<AccountUsage>,
    /// The usage read in flight; a newer one replaces it.
    pub(super) _load: Option<gpui::Task<()>>,
    _subs: Vec<Subscription>,
}

impl DetailsPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            thread: None,
            usage: Vec::new(),
            _load: None,
            _subs: Vec::new(),
        }
    }

    /// Shows `view`'s details, redrawn whenever it or one of its pickers changes.
    pub fn set_thread(&mut self, view: Option<gpui::Entity<ThreadView>>, cx: &mut Context<Self>) {
        if self.thread.as_ref().and_then(|t| t.upgrade()) == view {
            return;
        }
        self._subs.clear();
        if let Some(view) = &view {
            let (picker, scripts) = {
                let v = view.read(cx);
                (v.branch_picker.clone(), v.scripts.clone())
            };
            self._subs.push(cx.observe(view, |_, _, cx| cx.notify()));
            self._subs.push(cx.observe(&picker, |_, _, cx| cx.notify()));
            self._subs
                .push(cx.observe(&scripts, |_, _, cx| cx.notify()));
        }
        self.thread = view.map(|v| v.downgrade());
        cx.notify();
    }

    pub fn set_usage(&mut self, usage: Vec<AccountUsage>, cx: &mut Context<Self>) {
        if self.usage != usage {
            self.usage = usage;
            cx.notify();
        }
    }
}

impl EventEmitter<PanelEvent> for DetailsPanel {}

impl gpui_component::dock::BasePanel for DetailsPanel {
    fn panel_name(&self) -> &'static str {
        "Details"
    }
    fn closable(&self, _: &App) -> bool {
        false
    }
    fn zoomable(&self, _: &App) -> bool {
        false
    }
}

impl Panel for DetailsPanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        "Details"
    }
    fn zoom_control(&self, _: &App) -> Option<gpui_component::dock::PanelControl> {
        None
    }
}

impl Focusable for DetailsPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for DetailsPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let usage = render_usage(&self.usage, &theme, cx);
        let content = match self.thread.as_ref().and_then(|t| t.upgrade()) {
            Some(view) => view.update(cx, |v, cx| v.render_details(usage, cx)),
            None => v_flex()
                .child(caption("Usage", &theme))
                .child(usage)
                .child(
                    div()
                        .px_2p5()
                        .py_2()
                        .text_size(ui::META)
                        .text_color(theme.muted_foreground)
                        .child("Open a thread to see its folder, branch and agents here."),
                )
                .into_any_element(),
        };
        div()
            .id("details-panel")
            .track_focus(&self.focus)
            .size_full()
            .overflow_y_scroll()
            .bg(theme.background)
            .p_3()
            .child(
                v_flex()
                    .debug_selector(|| "thread-details-card".into())
                    .p_1p5()
                    .rounded(ui::RADIUS_XL)
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.popover)
                    .child(content),
            )
    }
}

fn caption(text: &'static str, theme: &Theme) -> impl IntoElement {
    div()
        .h(gpui::px(28.))
        .px_2p5()
        .flex()
        .items_center()
        .text_size(ui::META)
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme.muted_foreground)
        .child(text)
}

/// One row per account: its color, name and provider, how old the reading is, and each window's use; "—" when its
/// agents haven't reported any.
fn render_usage(usage: &[AccountUsage], theme: &Theme, cx: &App) -> AnyElement {
    let now = ui::now_ms();
    v_flex()
        .children(usage.iter().map(|a| {
            let windows = if a.windows.is_empty() {
                vec![div().child("—").into_any_element()]
            } else {
                a.windows
                    .iter()
                    .map(|&(label, used)| {
                        h_flex()
                            .gap_1p5()
                            .child(div().text_color(theme.muted_foreground).child(label))
                            .child(
                                div()
                                    .w(gpui::px(34.))
                                    .flex()
                                    .justify_end()
                                    .text_color(usage_color(used, theme))
                                    .child(format!("{}%", (used * 100.).round() as u32)),
                            )
                            .into_any_element()
                    })
                    .collect()
            };
            h_flex()
                .id(gpui::SharedString::from(format!("usage-{}", a.id)))
                .min_h(ui::ROW_H)
                .py_1()
                .px_2p5()
                .gap_2p5()
                .items_center()
                .child(
                    div()
                        .size_4()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(ui::StatusDot::new(ui::account_color_in(cx, &a.id))),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(
                            h_flex()
                                .gap_1p5()
                                .text_size(ui::CODE)
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.foreground.opacity(0.8))
                                .child(div().truncate().child(a.label.clone()))
                                .child(
                                    div()
                                        .text_color(theme.muted_foreground)
                                        .child(a.provider.label()),
                                ),
                        )
                        .children(a.as_of_ms.map(|at| {
                            div()
                                .text_size(ui::META)
                                .text_color(theme.muted_foreground)
                                .child(format!("as of {}", as_of(now, at)))
                        })),
                )
                .child(
                    v_flex()
                        .flex_shrink_0()
                        .items_end()
                        .text_size(ui::META)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.foreground.opacity(0.8))
                        .children(windows),
                )
        }))
        .into_any_element()
}

/// "just now", else "12m ago".
fn as_of(now_ms: i64, at_ms: i64) -> String {
    match ui::ago(now_ms, at_ms) {
        a if a == "now" => "just now".into(),
        a => format!("{a} ago"),
    }
}

/// Plain until a window is nearly spent.
fn usage_color(used: f32, theme: &Theme) -> gpui::Hsla {
    if used >= 0.9 {
        ui::ink(theme, theme.danger)
    } else if used >= 0.75 {
        ui::ink(theme, theme.warning)
    } else {
        theme.foreground.opacity(0.8)
    }
}
