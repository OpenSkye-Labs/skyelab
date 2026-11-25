//! Slim left navigation rail (stubbed for red acceptance tests before implementation).

use super::Workspace;
use super::details_panel::AccountUsage;
use gpui::App;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RailItem {
    Home,
    Bots,
    PullRequests,
    Issues,
    Scheduled,
    Usage,
    Settings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum UsageAlert {
    Low,
    Critical,
}

pub(super) fn usage_alert(_usage: &[AccountUsage], _now_ms: i64) -> Option<UsageAlert> {
    None
}

impl Workspace {
    pub(super) fn rail_active(&self, _cx: &App) -> RailItem {
        RailItem::Home
    }
}
