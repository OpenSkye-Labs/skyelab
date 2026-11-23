//! The update check with an injected fetch: never the network.

use crate::updates::{self, Fetch, UpdateStatus};
use gpui::TestAppContext;
use skyelab_client::release::Release;
use skyelab_core::AppSettings;
use skyelab_core::update::RELEASES_PAGE;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

const DAY: u64 = 24 * 60 * 60;

fn counting_fetch(tag: &'static str) -> (Fetch, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let fetch: Fetch = Arc::new(move || {
        seen.fetch_add(1, Ordering::SeqCst);
        Ok(Release {
            tag: tag.into(),
            url: format!("{RELEASES_PAGE}tag/{tag}"),
        })
    });
    (fetch, calls)
}

fn last_checked_ago(path: &Path, secs: u64) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs();
    AppSettings::update(path, |s| s.updates.last_check = Some(now - secs)).expect("save");
}

fn status(cx: &mut TestAppContext) -> UpdateStatus {
    cx.update(|cx| cx.global::<updates::Updates>().status.clone())
}

#[gpui::test]
fn start_check_daily_skip_manual_and_off(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.toml");
    let (fetch, calls) = counting_fetch("v99.0.0");
    let n = || calls.load(Ordering::SeqCst);

    // Never checked: the start check runs and finds the newer release.
    cx.update(|cx| updates::init(Some(path.clone()), fetch, cx));
    cx.run_until_parked();
    assert_eq!(n(), 1);
    let url = format!("{RELEASES_PAGE}tag/v99.0.0");
    assert_eq!(
        status(cx),
        UpdateStatus::Available {
            tag: "v99.0.0".into(),
            url: url.clone()
        }
    );

    // Download opens the checked release page.
    cx.update(updates::download);
    assert_eq!(cx.opened_url().as_deref(), Some(url.as_str()));

    // Skip hides it and remembers the version.
    cx.update(updates::skip);
    cx.run_until_parked();
    assert_eq!(status(cx), UpdateStatus::Idle);
    let saved = AppSettings::load(&path).expect("load").updates;
    assert_eq!(saved.skipped.as_deref(), Some("v99.0.0"));

    // Checked an hour ago: the automatic check doesn't fetch.
    last_checked_ago(&path, 60 * 60);
    cx.update(|cx| updates::check(false, cx));
    cx.run_until_parked();
    assert_eq!(n(), 1, "not due yet");

    // A day later it fetches, and stays quiet about the skipped version.
    last_checked_ago(&path, DAY + 60 * 60);
    cx.update(|cx| updates::check(false, cx));
    cx.run_until_parked();
    assert_eq!(n(), 2);
    assert_eq!(status(cx), UpdateStatus::Idle);

    // Check for Updates ignores the schedule and the skip.
    cx.update(|cx| cx.dispatch_action(&updates::CheckForUpdates));
    cx.run_until_parked();
    assert_eq!(n(), 3);
    assert!(matches!(status(cx), UpdateStatus::Available { .. }));

    // Off: the automatic check never fetches, even when due.
    let off = cx.update(|cx| updates::set_enabled(false, cx));
    cx.run_until_parked();
    off.detach();
    cx.run_until_parked();
    assert_eq!(status(cx), UpdateStatus::Idle);
    last_checked_ago(&path, 2 * DAY);
    cx.update(|cx| updates::check(false, cx));
    cx.run_until_parked();
    assert_eq!(n(), 3, "off means no fetch");
}

#[gpui::test]
fn not_newer_or_foreign_page_shows_nothing(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("tempdir");
    let foreign: Fetch = Arc::new(|| {
        Ok(Release {
            tag: "v99.0.0".into(),
            url: "https://example.com/evil".into(),
        })
    });
    cx.update(|cx| updates::init(Some(dir.path().join("a.toml")), foreign, cx));
    cx.run_until_parked();
    assert!(matches!(status(cx), UpdateStatus::Failed(_)));
    cx.update(updates::download);
    assert_eq!(cx.opened_url(), None, "an unchecked page never opens");

    let (same, _) = counting_fetch(concat!("v", env!("CARGO_PKG_VERSION")));
    cx.update(|cx| updates::init(Some(dir.path().join("b.toml")), same, cx));
    cx.run_until_parked();
    assert_eq!(status(cx), UpdateStatus::UpToDate);
}

#[test]
fn debug_fake_release_needs_no_network() {
    let got = updates::fetcher(Some("v9.9.9".into()), true)().expect("fake");
    assert_eq!(got.tag, "v9.9.9");
    assert!(
        skyelab_core::update::release_page_ok(&got.url),
        "{}",
        got.url
    );
}
