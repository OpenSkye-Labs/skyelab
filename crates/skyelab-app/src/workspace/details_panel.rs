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
use skyelab_agent::Event;
use skyelab_core::{Account, Provider};
use skyelab_store::Store;

/// What SkyeLab last saw of one account's limits: the newest reading of each window and when it came.
#[derive(Debug, Clone, PartialEq)]
pub struct AccountUsage {
    pub id: String,
    pub label: String,
    pub provider: Provider,
    /// 5h, then Weekly: how much of each is used (0 to 1), when a reading said.
    pub windows: Vec<(&'static str, f32)>,
    /// `Some(resets_at)` (Unix seconds) when the newest reading says the account is refused.
    pub limited: Option<Option<i64>>,
    /// When the newest reading was stored (`None`: nothing reported yet).
    pub as_of_ms: Option<i64>,
}

/// `account`'s newest stored readings, kept per window by the store as its threads' agents report them
/// (Claude's `rate_limit_event`, Codex's `account/rateLimits/updated`). Only what was reported: nothing is asked of
/// the account, and a store error reads as no data.
pub fn account_usage(store: &Store, account: &Account) -> AccountUsage {
    let readings = store.usage_readings(&account.id).unwrap_or_default();
    let mut windows: Vec<(&'static str, f32)> = Vec::new();
    let (mut limited, mut as_of_ms) = (None, None);
    for r in &readings {
        let Ok(Record::Agent(event)) = Record::from_row("agent", &r.payload_json) else {
            continue;
        };
        as_of_ms = as_of_ms.max(Some(r.created_at));
        if r.window.is_empty() {
            if let Event::RateLimit {
                limited: true,
                resets_at,
                ..
            } = event
            {
                limited = Some(resets_at);
            }
        } else if let Some(w @ ("5h" | "Weekly", _)) = event.rate_limit() {
            windows.push(w);
        }
    }
    // 5h before Weekly, like the CLI's /usage.
    windows.sort_by_key(|(label, _)| *label != "5h");
    AccountUsage {
        id: account.id.clone(),
        label: account.label.clone(),
        provider: account.provider,
        windows,
        limited,
        as_of_ms,
    }
}

/// "Limited · resets in 2h 14m" while the newest reading says limited and its reset is ahead (`now` in Unix seconds).
pub(super) fn limit_text(limited: Option<Option<i64>>, now: i64) -> Option<String> {
    match limited? {
        Some(at) if at <= now => None,
        Some(at) => Some(format!(
            "Limited · resets in {}",
            super::timeline_text::wait(at - now)
        )),
        None => Some("Limited".into()),
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

/// One row per account: its color, name and provider, then how old the reading is (or that it is limited, in the
/// warning color), and on the right a compact bar per window; "—" when its agents haven't reported anything.
fn render_usage(usage: &[AccountUsage], theme: &Theme, cx: &App) -> AnyElement {
    let now = ui::now_ms();
    v_flex()
        .children(usage.iter().map(|a| {
            let limit = limit_text(a.limited, now / 1000);
            let meta = match (&limit, a.as_of_ms) {
                (Some(text), _) => Some((text.clone(), ui::ink(theme, theme.warning))),
                (None, Some(at)) => {
                    Some((format!("as of {}", as_of(now, at)), theme.muted_foreground))
                }
                (None, None) => None,
            };
            let windows = if a.windows.is_empty() && limit.is_none() {
                vec![div().child("—").into_any_element()]
            } else {
                a.windows
                    .iter()
                    .map(|&(label, used)| usage_bar(label, used, theme))
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
                                        .flex_shrink_0()
                                        .text_color(theme.muted_foreground)
                                        .child(a.provider.label()),
                                ),
                        )
                        .children(meta.map(|(text, color)| {
                            div()
                                .truncate()
                                .text_size(ui::META)
                                .text_color(color)
                                .child(text)
                        })),
                )
                .child(
                    v_flex()
                        .flex_shrink_0()
                        .gap_0p5()
                        .items_end()
                        .text_size(ui::META)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.foreground.opacity(0.8))
                        .children(windows),
                )
        }))
        .into_any_element()
}

/// "5h ▬▬▭ 62%": the window, a 40 px track filled to its use, and the percentage.
fn usage_bar(label: &'static str, used: f32, theme: &Theme) -> AnyElement {
    const TRACK: f32 = 40.;
    let color = usage_color(used, theme);
    h_flex()
        .gap_1p5()
        .child(
            div()
                .w(gpui::px(44.))
                .flex()
                .justify_end()
                .text_color(theme.muted_foreground)
                .child(label),
        )
        .child(
            div()
                .w(gpui::px(TRACK))
                .h(gpui::px(4.))
                .rounded_full()
                .bg(theme.foreground.opacity(0.12))
                .child(
                    div()
                        .h_full()
                        .w(gpui::px(TRACK * used.clamp(0., 1.)))
                        .rounded_full()
                        .bg(color),
                ),
        )
        .child(
            div()
                .w(gpui::px(30.))
                .flex()
                .justify_end()
                .text_color(color)
                .child(format!("{}%", (used * 100.).round() as u32)),
        )
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

#[cfg(test)]
mod tests {
    use super::limit_text;

    #[test]
    fn details_usage_limited_says_when_it_resets() {
        let now = 1_000_000;
        assert_eq!(
            limit_text(Some(Some(now + 2 * 3600 + 14 * 60)), now).as_deref(),
            Some("Limited · resets in 2h 14m")
        );
        assert_eq!(limit_text(Some(None), now).as_deref(), Some("Limited"));
        assert_eq!(limit_text(Some(Some(now)), now), None, "already reset");
        assert_eq!(limit_text(None, now), None);
    }
}
