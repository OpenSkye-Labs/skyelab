//! Slim left navigation rail on the far left of the main window.

use super::center::Page;
use super::details_panel::{AccountUsage, limit_text};
use super::gh_pages::{Kind as GhKind, OpenIssues, OpenPullRequests};
use super::{CONTEXT, OpenBots, OpenHome, OpenSettings, OpenUsage, Workspace};
use crate::settings::Section;
use crate::ui::{self, Button, SkyeIcon};
use gpui::{Action, App, Context, IntoElement, SharedString, WeakEntity, div, prelude::*, px};
use gpui_component::{ActiveTheme as _, Theme, dock::DockPlacement, v_flex};

/// Which primary destination in the left rail is currently active.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RailItem {
    Home,
    Bots,
    PullRequests,
    Issues,
    Usage,
    Settings,
}

/// Severity of the live usage status dot shown on the Usage rail button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum UsageAlert {
    /// Any account has a non-reset window at >= 75%.
    Low,
    /// Any account is actively rate-limited or has a non-reset window at >= 90%.
    Critical,
}

/// Evaluates loaded account usage readings at `now_ms` to decide if the Usage rail button shows a live status dot.
pub(super) fn usage_alert(usage: &[AccountUsage], now_ms: i64) -> Option<UsageAlert> {
    let now_s = now_ms / 1000;
    let mut alert = None;
    for account in usage {
        if limit_text(account.limited, account.limited_ms, now_ms).is_some() {
            return Some(UsageAlert::Critical);
        }
        for window in &account.windows {
            if window.is_reset(now_s) {
                continue;
            }
            if window.used >= 0.9 {
                return Some(UsageAlert::Critical);
            }
            if window.used >= 0.75 {
                alert = Some(UsageAlert::Low);
            }
        }
    }
    alert
}

/// A rail item. `close` (the item of the Settings section shown now): a second click closes Settings, as a page
/// toggle would, instead of opening it again.
fn rail_button(
    id: &'static str,
    icon: SkyeIcon,
    tip: SharedString,
    action: Box<dyn Action>,
    selected: bool,
    close: Option<WeakEntity<Workspace>>,
    theme: &Theme,
) -> Button {
    Button::new(id)
        .debug_selector(move || id.into())
        .ghost()
        .medium()
        .square()
        .icon(icon)
        .selected(selected)
        .toggled(selected)
        .tooltip(tip)
        .when(selected, |b| {
            b.bg(theme.sidebar_accent)
                .border_color(theme.sidebar_border.opacity(0.7))
        })
        .on_click(move |_, window, cx| match &close {
            Some(ws) => {
                let _ = ws.update(cx, |ws, cx| ws.close_settings(window, cx));
            }
            None => crate::workspace::dispatch(action.boxed_clone(), window, cx),
        })
}

impl Workspace {
    /// Bots paused until fixed, plus open bot runs waiting on an approval: the Bots rail item's count.
    pub(super) fn bots_needing_you(&self, cx: &App) -> usize {
        let paused = cx
            .try_global::<super::scheduled::Scheduler>()
            .map_or(0, |s| s.paused);
        let waiting = self
            .open_threads
            .iter()
            .filter(|v| {
                let v = v.read(cx);
                v.scheduled_task.is_some() && super::thread_rows::waiting_ask(v).is_some()
            })
            .count();
        paused + waiting
    }

    pub(super) fn rail_active(&self, cx: &App) -> RailItem {
        if self.settings_open {
            return match self.settings.as_ref().map(|s| s.read(cx).section) {
                Some(Section::Usage) => RailItem::Usage,
                _ => RailItem::Settings,
            };
        }
        match &self.page {
            Page::Bots => RailItem::Bots,
            Page::Github(GhKind::Prs) => RailItem::PullRequests,
            Page::Github(GhKind::Issues) => RailItem::Issues,
            Page::Welcome | Page::Thread(_) | Page::RemoteThread(_) | Page::Compare(_) => {
                RailItem::Home
            }
        }
    }

    pub(super) fn render_rail(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let active = self.rail_active(cx);
        let remote = self.is_remote();
        let sidebar_open = self.dock.read(cx).is_dock_open(DockPlacement::Left);
        let alert = usage_alert(&self.details.read(cx).usage, ui::now_ms());
        let dot_color = alert.map(|level| match level {
            UsageAlert::Low => theme.warning,
            UsageAlert::Critical => theme.danger,
        });
        // The dot says why in the tooltip.
        let usage_tip: SharedString = match alert {
            Some(UsageAlert::Critical) => "Usage · an account is at or near its limit".into(),
            Some(UsageAlert::Low) => "Usage · an account is past 75% of a limit".into(),
            None => "Usage".into(),
        };
        let bots_need = self.bots_needing_you(cx);
        let bots_tip: SharedString = match bots_need {
            0 => "Bots".into(),
            n => format!("Bots · {n} need you (paused, or waiting on an approval)").into(),
        };
        let ws = cx.entity().downgrade();
        let close = |item: RailItem| (active == item && self.settings_open).then(|| ws.clone());

        let usage_btn = div()
            .relative()
            .child(rail_button(
                "rail-usage",
                SkyeIcon::ChartNoAxesColumn,
                usage_tip,
                Box::new(OpenUsage),
                active == RailItem::Usage,
                close(RailItem::Usage),
                theme,
            ))
            .children(dot_color.map(|color| {
                div()
                    .debug_selector(|| "rail-usage-dot".into())
                    .absolute()
                    .top(px(4.))
                    .right(px(4.))
                    .size(px(6.))
                    .rounded_full()
                    .bg(color)
            }));

        let items = v_flex()
            .flex_1()
            .min_h_0()
            .w_full()
            .items_center()
            .py_2()
            .gap_1()
            .border_r_1()
            .border_color(theme.sidebar_border)
            .child(rail_button(
                "rail-home",
                SkyeIcon::Home,
                "Home".into(),
                Box::new(OpenHome),
                active == RailItem::Home,
                None,
                theme,
            ))
            .child(
                div()
                    .relative()
                    .child(rail_button(
                        "rail-bots",
                        SkyeIcon::Bot,
                        bots_tip,
                        Box::new(OpenBots),
                        active == RailItem::Bots,
                        None,
                        theme,
                    ))
                    .children((bots_need > 0).then(|| {
                        div()
                            .debug_selector(|| "rail-bots-count".into())
                            .absolute()
                            .top(px(2.))
                            .right(px(2.))
                            .min_w(px(14.))
                            .h(px(14.))
                            .px(px(3.))
                            .rounded_full()
                            .bg(theme.danger)
                            .text_color(theme.danger_foreground)
                            .text_size(px(9.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(bots_need.to_string())
                    })),
            )
            .child(
                rail_button(
                    "rail-prs",
                    SkyeIcon::GitPullRequest,
                    "Pull requests".into(),
                    Box::new(OpenPullRequests),
                    active == RailItem::PullRequests,
                    None,
                    theme,
                )
                .disabled(remote),
            )
            .child(
                rail_button(
                    "rail-issues",
                    SkyeIcon::CircleDot,
                    "Issues".into(),
                    Box::new(OpenIssues),
                    active == RailItem::Issues,
                    None,
                    theme,
                )
                .disabled(remote),
            )
            .child(div().flex_1())
            .child(usage_btn)
            .child(
                rail_button(
                    "rail-settings",
                    SkyeIcon::Settings,
                    "Settings".into(),
                    Box::new(OpenSettings),
                    active == RailItem::Settings,
                    close(RailItem::Settings),
                    theme,
                )
                .tooltip_with_action("Settings", &OpenSettings, Some(CONTEXT)),
            );

        v_flex()
            .id("rail")
            .debug_selector(|| "rail".into())
            .flex_shrink_0()
            .w(ui::RAIL_W)
            .h_full()
            .bg(theme.sidebar)
            .text_color(theme.sidebar_foreground)
            // macOS traffic lights span x=16..68 in TOP_ROW_H: keep this band free of icons and right border so
            // traffic lights rest on one uninterrupted surface matching either the open sidebar or collapsed header.
            .when(cfg!(target_os = "macos"), |d| {
                d.child(
                    div()
                        .flex_shrink_0()
                        .w_full()
                        .h(ui::TOP_ROW_H)
                        .when(sidebar_open, |s| s.bg(theme.sidebar))
                        .when(!sidebar_open, |s| {
                            s.bg(theme.background)
                                .border_b_1()
                                .border_color(theme.border)
                        }),
                )
            })
            .child(items)
    }
}
