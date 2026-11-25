//! Acceptance tests for the slim left navigation rail and Bots ("Coming soon") page.

use super::center::Page;
use super::details_panel::{AccountUsage, UsageWindow};
use super::gh_pages::{Kind as GhKind, OpenIssues, OpenPullRequests};
use super::left_rail::{RailItem, UsageAlert, usage_alert};
use super::manage_harness::rig;
use super::*;
use crate::palette::Kind as PaletteKind;
use crate::settings::Section;
use crate::ui::{AppAssets, SkyeIcon};
use gpui::{AssetSource as _, Modifiers, TestAppContext, px, size};
use gpui_component::IconNamed as _;
use gpui_component::dock::DockPlacement;
use skyelab_core::Provider;

fn sample_usage(windows: Vec<UsageWindow>, limited: Option<Option<i64>>, now_ms: i64) -> AccountUsage {
    AccountUsage {
        id: "acme".into(),
        label: "Acme Team".into(),
        provider: Provider::Claude,
        windows,
        limited,
        limited_ms: limited.map(|_| now_ms),
        refresh_error: None,
    }
}

#[gpui::test]
fn rail_draws_items_and_tracks_active_destination(cx: &mut TestAppContext) {
    let svg = AppAssets
        .load(SkyeIcon::Home.path().as_ref())
        .expect("asset load")
        .expect("home icon embedded");
    assert!(
        std::str::from_utf8(&svg)
            .expect("utf8 svg")
            .contains("<svg"),
        "SkyeIcon::Home embeds valid SVG"
    );

    let (r, cx) = rig(cx);
    cx.simulate_resize(size(px(1200.), px(800.)));
    cx.run_until_parked();

    let rail = cx.debug_bounds("rail").expect("left rail is drawn");
    assert_eq!(rail.origin.x, px(0.), "rail sits at far left");
    assert_eq!(rail.size.width, px(48.), "rail is 48 px wide");
    for sel in [
        "rail-home",
        "rail-bots",
        "rail-prs",
        "rail-issues",
        "rail-scheduled",
        "rail-usage",
        "rail-settings",
    ] {
        assert!(cx.debug_bounds(sel).is_some(), "{sel} button is drawn");
    }

    let active = |cx: &mut gpui::VisualTestContext| r.ws.read_with(cx, |ws, cx| ws.rail_active(cx));
    assert_eq!(active(cx), RailItem::Home, "Welcome highlights Home");

    let t = r.thread("Alpha", cx);
    let _view = r.open(t, cx);
    assert_eq!(active(cx), RailItem::Home, "Thread highlights Home");

    cx.dispatch_action(OpenBots);
    cx.run_until_parked();
    assert_eq!(active(cx), RailItem::Bots, "Bots page highlights Bots");

    cx.dispatch_action(OpenPullRequests);
    cx.run_until_parked();
    assert_eq!(
        active(cx),
        RailItem::PullRequests,
        "PR page highlights Pull requests"
    );

    cx.dispatch_action(OpenIssues);
    cx.run_until_parked();
    assert_eq!(active(cx), RailItem::Issues, "Issues page highlights Issues");

    cx.dispatch_action(OpenSettingsSection {
        section: Section::Scheduled,
    });
    cx.run_until_parked();
    assert_eq!(
        active(cx),
        RailItem::Scheduled,
        "Settings -> Scheduled highlights Scheduled tasks"
    );

    cx.dispatch_action(OpenUsage);
    cx.run_until_parked();
    assert_eq!(
        active(cx),
        RailItem::Usage,
        "Settings -> Usage highlights Usage"
    );

    cx.dispatch_action(OpenSettingsSection {
        section: Section::Accounts,
    });
    cx.run_until_parked();
    assert_eq!(
        active(cx),
        RailItem::Settings,
        "Other Settings sections highlight Settings"
    );
}

#[gpui::test]
fn open_home_and_rail_click_restore_last_home_close_settings_and_open_sidebar(
    cx: &mut TestAppContext,
) {
    let (r, cx) = rig(cx);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    cx.simulate_resize(size(px(1200.), px(800.)));
    cx.run_until_parked();

    let id = r.thread("Main thread", cx);
    let view = r.open(id, cx);

    cx.dispatch_action(OpenBots);
    cx.dispatch_action(ToggleSidebar);
    cx.dispatch_action(OpenSettingsSection {
        section: Section::General,
    });
    cx.run_until_parked();
    assert!(r.ws.read_with(cx, |ws, _| ws.settings_open));
    assert!(
        !r.ws
            .read_with(cx, |ws, cx| ws.dock.read(cx).is_dock_open(DockPlacement::Left))
    );

    // Click rail-home directly while Settings modal is open: backdrop must not swallow it.
    let home_btn = cx
        .debug_bounds("rail-home")
        .expect("rail-home drawn while Settings open");
    cx.simulate_click(home_btn.center(), Modifiers::none());
    cx.run_until_parked();

    r.ws.read_with(cx, |ws, cx| {
        assert!(!ws.settings_open, "clicking Home closes Settings modal");
        assert!(
            ws.dock.read(cx).is_dock_open(DockPlacement::Left),
            "clicking Home reopens a collapsed sidebar"
        );
        assert_eq!(
            ws.page,
            Page::Thread(view.downgrade()),
            "clicking Home returns to the last active thread"
        );
    });

    // If the remembered thread is closed, OpenHome falls back cleanly to Welcome.
    r.ws.update_in(cx, |ws, window, cx| ws.close_thread(&view, window, cx));
    cx.dispatch_action(OpenPullRequests);
    cx.run_until_parked();
    assert_eq!(
        r.ws.read_with(cx, |ws, _| ws.page.clone()),
        Page::Github(GhKind::Prs)
    );
    cx.dispatch_action(OpenHome);
    cx.run_until_parked();
    assert_eq!(
        r.ws.read_with(cx, |ws, _| ws.page.clone()),
        Page::Welcome,
        "falls back to Welcome when no remembered thread is alive"
    );
}

#[gpui::test]
fn bots_page_supports_history_restore_palette_and_quick_actions(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    cx.simulate_resize(size(px(1200.), px(800.)));
    cx.run_until_parked();

    // Command palette lists OpenHome and OpenBots.
    let action_names: Vec<&str> = r
        .ws
        .update_in(cx, |ws, window, cx| {
            let focus = ws.focus.clone();
            ws.palette_entries(&focus, window, cx)
        })
        .into_iter()
        .filter_map(|e| match e.kind {
            PaletteKind::Action(name) => Some(name),
            _ => None,
        })
        .collect();
    assert!(
        action_names.contains(&"skyelab::OpenHome"),
        "palette lists OpenHome"
    );
    assert!(
        action_names.contains(&"skyelab::OpenBots"),
        "palette lists OpenBots"
    );

    cx.dispatch_action(OpenBots);
    cx.run_until_parked();
    r.ws.read_with(cx, |ws, cx| {
        assert_eq!(ws.page, Page::Bots);
        assert_eq!(ws.page.key(cx), "bots");
    });
    assert!(
        cx.debug_bounds("bots-page").is_some(),
        "Bots center page renders"
    );

    // GoBack / GoForward history navigation.
    cx.dispatch_action(GoBack);
    cx.run_until_parked();
    assert_eq!(r.ws.read_with(cx, |ws, _| ws.page.clone()), Page::Welcome);
    cx.dispatch_action(GoForward);
    cx.run_until_parked();
    assert_eq!(r.ws.read_with(cx, |ws, _| ws.page.clone()), Page::Bots);

    // restore_page("bots") restores Page::Bots cleanly.
    r.ws.update_in(cx, |ws, window, cx| {
        ws.show(Page::Welcome, window, cx);
        ws.restore_page("bots", window, cx);
    });
    cx.run_until_parked();
    assert_eq!(r.ws.read_with(cx, |ws, _| ws.page.clone()), Page::Bots);

    // Quick action: Open Scheduled tasks opens Settings -> Scheduled.
    let sched_btn = cx
        .debug_bounds("bots-open-scheduled")
        .expect("Open Scheduled tasks quick action drawn");
    cx.simulate_click(sched_btn.center(), Modifiers::none());
    cx.run_until_parked();
    r.ws.read_with(cx, |ws, cx| {
        assert!(ws.settings_open);
        assert_eq!(
            ws.settings.as_ref().map(|s| s.read(cx).section),
            Some(Section::Scheduled)
        );
    });

    // Clicking rail-bots while Settings is open closes Settings and keeps Bots visible.
    let bots_btn = cx
        .debug_bounds("rail-bots")
        .expect("rail-bots drawn while Settings open");
    cx.simulate_click(bots_btn.center(), Modifiers::none());
    cx.run_until_parked();
    r.ws.read_with(cx, |ws, _| {
        assert!(!ws.settings_open);
        assert_eq!(ws.page, Page::Bots);
    });

    // Quick action: Back to Home returns to Home.
    let back_btn = cx
        .debug_bounds("bots-back-home")
        .expect("Back to Home quick action drawn");
    cx.simulate_click(back_btn.center(), Modifiers::none());
    cx.run_until_parked();
    assert_eq!(r.ws.read_with(cx, |ws, _| ws.page.clone()), Page::Welcome);
}

#[gpui::test]
fn usage_dot_reflects_account_limits_and_sidebar_footer_has_no_duplicates(cx: &mut TestAppContext) {
    assert_eq!(
        panels::FOOTER_BUTTON_IDS,
        &["sidebar-refresh"],
        "sidebar footer keeps only Refresh so Settings/PRs/Issues/Usage are not duplicated"
    );

    let (r, cx) = rig(cx);
    cx.simulate_resize(size(px(1200.), px(800.)));
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("sidebar-refresh").is_some(),
        "sidebar refresh button remains accessible"
    );

    let now_ms = crate::ui::now_ms();
    let now_s = now_ms / 1000;
    assert_eq!(usage_alert(&[], now_ms), None);
    assert!(
        cx.debug_bounds("rail-usage-dot").is_none(),
        "no usage alert dot when usage is healthy"
    );

    // 80% used, reset ahead -> Low (warning dot).
    let low = vec![sample_usage(
        vec![UsageWindow {
            label: "5h",
            used: 0.80,
            resets_at: Some(now_s + 3600),
            at_ms: now_ms,
        }],
        None,
        now_ms,
    )];
    assert_eq!(usage_alert(&low, now_ms), Some(UsageAlert::Low));
    r.ws.update(cx, |ws, cx| {
        ws.details.update(cx, |d, cx| d.set_usage(low, cx));
    });
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("rail-usage-dot").is_some(),
        "usage dot appears when an account is running low"
    );

    // Past reset -> clears dot.
    let reset = vec![sample_usage(
        vec![UsageWindow {
            label: "5h",
            used: 0.95,
            resets_at: Some(now_s - 60),
            at_ms: now_ms - 120_000,
        }],
        None,
        now_ms,
    )];
    assert_eq!(usage_alert(&reset, now_ms), None);
    r.ws.update(cx, |ws, cx| {
        ws.details.update(cx, |d, cx| d.set_usage(reset, cx));
    });
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("rail-usage-dot").is_none(),
        "usage dot clears once the window resets"
    );

    // Active limit -> Critical (danger dot).
    let limited = vec![sample_usage(Vec::new(), Some(Some(now_s + 1800)), now_ms)];
    assert_eq!(usage_alert(&limited, now_ms), Some(UsageAlert::Critical));
    r.ws.update(cx, |ws, cx| {
        ws.details.update(cx, |d, cx| d.set_usage(limited, cx));
    });
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("rail-usage-dot").is_some(),
        "usage dot appears when an account is rate-limited"
    );
}
