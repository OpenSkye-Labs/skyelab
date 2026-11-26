//! The thread details card (T3's ⌘⇧B card): the shown thread's folder, branch, git actions, scripts, agents and pull
//! request (see `thread_details.rs`), with every account's usage. It floats over the chat at the top right
//! (`render::details_card`).

use super::thread::ThreadView;
use super::thread_model::Record;
use crate::ui;
use gpui::{
    AnimationExt as _, AnyElement, App, Context, EventEmitter, FocusHandle, Focusable, FontWeight,
    IntoElement, Render, Subscription, WeakEntity, Window, div, prelude::*,
};
use gpui_component::{ActiveTheme as _, Theme, h_flex, v_flex};
use skyelab_agent::Event;
use skyelab_core::{Account, Provider};
use skyelab_store::{Store, UsageReading};

/// One window's newest reading: how much of it is used (0 to 1), when it resets and when the reading came.
#[derive(Debug, Clone, PartialEq)]
pub struct UsageWindow {
    /// "5h" or "Weekly".
    pub label: &'static str,
    pub used: f32,
    /// Unix seconds, when the reading said.
    pub resets_at: Option<i64>,
    /// When this reading was stored, ms since the epoch.
    pub at_ms: i64,
}

impl UsageWindow {
    /// The window has reset since its reading, so its percentage no longer holds (`now` in Unix seconds).
    pub(super) fn is_reset(&self, now: i64) -> bool {
        self.resets_at.is_some_and(|at| at <= now)
    }
}

/// What SkyeLab last saw of one account's limits: the newest reading of each window, and whether it is limited.
#[derive(Debug, Clone, PartialEq)]
pub struct AccountUsage {
    pub id: String,
    pub label: String,
    pub provider: Provider,
    /// 5h, then Weekly, when a reading said.
    pub windows: Vec<UsageWindow>,
    /// `Some(resets_at)` (Unix seconds) when the newest reading says the account is refused.
    pub limited: Option<Option<i64>>,
    /// When that newest reading was stored (ms), which ages out a "Limited" that names no reset.
    pub limited_ms: Option<i64>,
    /// Why the last time SkyeLab asked the account's CLI for its limits failed (the usage poll), if it did.
    pub refresh_error: Option<String>,
}

/// `account`'s newest stored readings, kept per window by the store as its threads' agents report them
/// (Claude's `rate_limit_event`, Codex's `account/rateLimits/updated`). Only what was reported: nothing is asked of
/// the account, and a store error reads as no data.
pub fn account_usage(
    store: &Store,
    account: &Account,
    errors: &std::collections::HashMap<String, String>,
) -> AccountUsage {
    AccountUsage {
        refresh_error: errors.get(&account.id).cloned(),
        ..usage_from(
            &store.usage_readings(&account.id).unwrap_or_default(),
            account,
        )
    }
}

pub(super) fn usage_from(readings: &[UsageReading], account: &Account) -> AccountUsage {
    let mut windows = Vec::new();
    let (mut limited, mut limited_ms) = (None, None);
    for r in readings {
        let Ok(Record::Agent(event)) = Record::from_row("agent", &r.payload_json) else {
            continue;
        };
        if r.window.is_empty() {
            if let Event::RateLimit {
                limited: true,
                resets_at,
                ..
            } = event
            {
                limited = Some(resets_at);
                limited_ms = Some(r.created_at);
            }
        } else if let Some((label @ ("5h" | "Weekly"), used)) = event.rate_limit()
            && let Event::RateLimit { resets_at, .. } = event
        {
            windows.push(UsageWindow {
                label,
                used,
                resets_at,
                at_ms: r.created_at,
            });
        }
    }
    // 5h before Weekly, like the CLI's /usage.
    windows.sort_by_key(|w| w.label != "5h");
    AccountUsage {
        id: account.id.clone(),
        label: account.label.clone(),
        provider: account.provider,
        windows,
        limited,
        limited_ms,
        refresh_error: None,
    }
}

/// A "Limited" that names no reset stops showing this long after its reading: the CLI may never report it lifted.
const LIMITED_TRUST_MS: i64 = 6 * 3600 * 1000;

/// "Limited · resets in 2h 14m" while the newest reading says limited and its reset is ahead; a plain "Limited"
/// until it is 6 h old (`now_ms` and `at_ms` in ms since the epoch).
pub(super) fn limit_text(
    limited: Option<Option<i64>>,
    at_ms: Option<i64>,
    now_ms: i64,
) -> Option<String> {
    let now = now_ms / 1000;
    match limited? {
        Some(at) if at <= now => None,
        Some(at) => Some(format!(
            "Limited · resets in {}",
            super::timeline_text::wait(at - now)
        )),
        None if at_ms.is_some_and(|at| now_ms - at > LIMITED_TRUST_MS) => None,
        None => Some("Limited".into()),
    }
}

pub struct DetailsPanel {
    focus: FocusHandle,
    thread: Option<WeakEntity<ThreadView>>,
    /// What the card showed of the thread last time it was drawn: a notify of the view that changes none of it (a
    /// streamed delta) leaves the card alone.
    sig: Option<super::thread_details::DetailsSig>,
    #[cfg(test)]
    pub(super) renders: usize,
    pub(super) usage: Vec<AccountUsage>,
    /// The usage read in flight; a newer one replaces it.
    pub(super) _load: Option<gpui::Task<()>>,
    _subs: Vec<Subscription>,
    /// The thread's branch picker and script menu, watched while shown. A view moving windows makes new ones
    /// (`ThreadView::attach`), so they are watched again when they change.
    pickers: Option<(gpui::EntityId, gpui::EntityId)>,
    _picker_subs: Vec<Subscription>,
    /// "3m" ages and resets move with the clock, not with the thread: one redraw a minute.
    _tick: gpui::Task<()>,
}

impl DetailsPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            thread: None,
            sig: None,
            #[cfg(test)]
            renders: 0,
            usage: Vec::new(),
            _load: None,
            _subs: Vec::new(),
            pickers: None,
            _picker_subs: Vec::new(),
            _tick: cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(std::time::Duration::from_secs(60))
                        .await;
                    if this.update(cx, |_, cx| cx.notify()).is_err() {
                        break;
                    }
                }
            }),
        }
    }

    fn watch_pickers(&mut self, view: &gpui::Entity<ThreadView>, cx: &mut Context<Self>) {
        let (picker, scripts) = {
            let v = view.read(cx);
            (v.branch_picker.clone(), v.scripts.clone())
        };
        let ids = Some((picker.entity_id(), scripts.entity_id()));
        if self.pickers != ids {
            self.pickers = ids;
            self._picker_subs = vec![
                cx.observe(&picker, |_, _, cx| cx.notify()),
                cx.observe(&scripts, |_, _, cx| cx.notify()),
            ];
            cx.notify();
        }
    }

    /// Shows `view`'s details, redrawn whenever what the card shows of it, or one of its pickers, changes.
    pub fn set_thread(&mut self, view: Option<gpui::Entity<ThreadView>>, cx: &mut Context<Self>) {
        if self.thread.as_ref().and_then(|t| t.upgrade()) == view {
            return;
        }
        self._subs.clear();
        (self.pickers, self._picker_subs) = (None, Vec::new());
        if let Some(view) = &view {
            self._subs.push(cx.observe(view, |this, view, cx| {
                this.watch_pickers(&view, cx);
                let sig = view.read(cx).details_sig();
                if this.sig.as_ref() != Some(&sig) {
                    this.sig = Some(sig);
                    cx.notify();
                }
            }));
            self.watch_pickers(view, cx);
        }
        self.sig = view.as_ref().map(|v| v.read(cx).details_sig());
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

/// A press outside the card, at this window point: the workspace closes the card when it covers the chat.
pub struct OutsidePress(pub gpui::Point<gpui::Pixels>);

impl EventEmitter<OutsidePress> for DetailsPanel {}

impl Focusable for DetailsPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for DetailsPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _render = crate::debug_renders::count("DetailsPanel");
        #[cfg(test)]
        {
            self.renders += 1;
        }
        let theme = cx.theme();
        let usage = render_usage(&self.usage, theme, cx);
        let content = match self.thread.as_ref().and_then(|t| t.upgrade()) {
            Some(view) => {
                let on = super::thread_details::On(view.downgrade());
                view.read(cx).render_details(usage, &on, cx)
            }
            None => v_flex()
                .child(caption("Usage", theme))
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
        // The layer is the whole height under the header; the card takes what its content needs and scrolls past that.
        let dark = theme.is_dark();
        let border = if dark { theme.input } else { theme.border };
        v_flex().size_full().child(
            div()
                .id("details-panel")
                .debug_selector(|| "thread-details-card".into())
                .track_focus(&self.focus)
                .occlude()
                .max_h_full()
                .overflow_y_scroll()
                .p_1p5()
                .rounded(ui::RADIUS_XL)
                .border_1()
                .border_color(border)
                .bg(theme.popover)
                .on_mouse_down_out(cx.listener(|_, e: &gpui::MouseDownEvent, _, cx| {
                    cx.emit(OutsidePress(e.position))
                }))
                .child(content)
                .with_animation(
                    "details-card-in",
                    gpui::Animation::new(ui::motion::FADE).with_easing(ui::motion::ease_out_quad),
                    move |el, t| {
                        el.opacity(t)
                            .shadow(ui::motion::overlay_shadow(t * t * t, dark))
                    },
                ),
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

/// One row per account: its color, name and provider, whether it is limited (in the warning color), and on the
/// right a compact bar per window, each dated by its own reading; "No reading yet" when its agents haven't reported
/// anything (as the Usage page says).
fn render_usage(usage: &[AccountUsage], theme: &Theme, cx: &App) -> AnyElement {
    let now = ui::now_ms();
    v_flex()
        .children(usage.iter().map(|a| {
            let limit = limit_text(a.limited, a.limited_ms, now);
            let windows = if a.windows.is_empty() && limit.is_none() {
                vec![
                    div()
                        .debug_selector(|| "usage-no-reading".into())
                        .font_weight(FontWeight::NORMAL)
                        .text_color(theme.muted_foreground)
                        .child("No reading yet")
                        .into_any_element(),
                ]
            } else {
                a.windows.iter().map(|w| usage_bar(w, now, theme)).collect()
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
                        .children(limit.map(|text| {
                            div()
                                .truncate()
                                .text_size(ui::META)
                                .text_color(ui::ink(theme, theme.warning))
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

/// "5h ▬▬▭ 62% 3m": the window, a 40 px track filled to its use, the percentage and how old its reading is. A
/// window past its reset is dimmed and says "reset" instead: its percentage no longer holds.
pub(super) fn usage_bar(w: &UsageWindow, now_ms: i64, theme: &Theme) -> AnyElement {
    const TRACK: f32 = 40.;
    let reset = w.is_reset(now_ms / 1000);
    let color = if reset {
        theme.muted_foreground
    } else {
        usage_color(w.used, theme)
    };
    let filled = if reset { 0. } else { w.used.clamp(0., 1.) };
    h_flex()
        .id(gpui::SharedString::from(format!("usage-bar-{}", w.label)))
        .gap_1p5()
        .when(reset, |el| el.opacity(0.6))
        .child(
            div()
                .w(gpui::px(44.))
                .flex()
                .justify_end()
                .text_color(theme.muted_foreground)
                .child(w.label),
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
                        .w(gpui::px(TRACK * filled))
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
                .child(if reset {
                    "reset".to_owned()
                } else {
                    format!("{}%", (w.used * 100.).round() as u32)
                }),
        )
        .child(
            div()
                .w(gpui::px(24.))
                .flex()
                .justify_end()
                .text_color(theme.muted_foreground)
                .font_weight(FontWeight::NORMAL)
                .child(ui::ago(now_ms, w.at_ms)),
        )
        .into_any_element()
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
#[path = "usage_freshness_test.rs"]
mod usage_freshness_test;
