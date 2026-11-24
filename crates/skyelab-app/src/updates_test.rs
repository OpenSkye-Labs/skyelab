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
            assets: vec![],
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
            assets: vec![],
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
    let got = updates::fetcher(Some("v9.9.9".into()), None, true)().expect("fake");
    assert_eq!(got.tag, "v9.9.9");
    assert!(
        skyelab_core::update::release_page_ok(&got.url),
        "{}",
        got.url
    );
}

/// A release of `v99.0.0` with a .deb for this arch and its SHA256SUMS; the download serves `body` for the .deb.
fn installable(cx: &mut TestAppContext, dir: &Path, body: &'static [u8], sums_of: &[u8]) -> String {
    use skyelab_core::update::{Asset, CHECKSUMS};
    let deb = format!("skyelab_99.0.0_{}.deb", std::env::consts::ARCH);
    let probe = dir.join("probe");
    std::fs::write(&probe, sums_of).expect("write");
    let sums = format!(
        "{}  {deb}\n",
        skyelab_core::update_install::sha256_file(&probe).expect("hash")
    );
    let assets: Vec<Asset> = [deb.as_str(), CHECKSUMS]
        .iter()
        .map(|n| Asset {
            name: (*n).into(),
            url: format!("{RELEASES_PAGE}download/v99.0.0/{n}"),
        })
        .collect();
    let fetch: Fetch = Arc::new(move || {
        Ok(Release {
            tag: "v99.0.0".into(),
            url: format!("{RELEASES_PAGE}tag/v99.0.0"),
            assets: assets.clone(),
        })
    });
    let download: updates::Download = Arc::new(move |url, out, progress| {
        let body = if url.ends_with(CHECKSUMS) {
            sums.as_bytes()
        } else {
            body
        };
        out.write_all(body)?;
        progress(body.len() as u64, Some(body.len() as u64));
        Ok(())
    });
    let downloads = dir.join("downloads");
    cx.update(|cx| {
        updates::init(Some(dir.join("settings.toml")), fetch, cx);
        updates::set_installer(
            Some(updates::Installer {
                install: skyelab_core::update_install::Install::Deb,
                download,
                dir: downloads.clone(),
            }),
            cx,
        );
    });
    cx.run_until_parked();
    deb
}

fn step(cx: &mut TestAppContext) -> Option<updates::Step> {
    match status(cx) {
        UpdateStatus::Updating { step, .. } => Some(step),
        _ => None,
    }
}

#[gpui::test]
fn download_and_install_verifies_then_waits_for_the_click(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("tempdir");
    let deb = installable(cx, dir.path(), b"package", b"package");
    assert!(cx.update(|cx| updates::can_install(cx)));

    cx.update(updates::install);
    cx.run_until_parked();
    assert_eq!(step(cx), Some(updates::Step::Ready));
    let file = dir.path().join("downloads").join(&deb);
    assert_eq!(std::fs::read(&file).expect("saved"), b"package");
    assert!(cx.update(|cx| updates::staged_is_folder(cx)));

    // A check doesn't replace an update that is ready.
    cx.update(|cx| updates::check(true, cx));
    cx.run_until_parked();
    assert_eq!(step(cx), Some(updates::Step::Ready));
}

#[gpui::test]
fn a_tampered_download_fails_and_can_be_retried(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("tempdir");
    let deb = installable(cx, dir.path(), b"evil", b"package");
    cx.update(updates::install);
    cx.run_until_parked();
    match step(cx) {
        Some(updates::Step::Failed(e)) => assert!(e.contains("checksum"), "{e}"),
        other => panic!("expected a failure, got {other:?}"),
    }
    assert!(!dir.path().join("downloads").join(&deb).exists());
    // Release page still opens; Try again runs it again.
    cx.update(updates::download);
    assert!(cx.opened_url().is_some());
    cx.update(updates::install);
    cx.run_until_parked();
    assert!(matches!(step(cx), Some(updates::Step::Failed(_))));
    // After a failure, a check runs again (a fixed release may be out).
    cx.update(|cx| updates::check(true, cx));
    cx.run_until_parked();
    assert!(matches!(status(cx), UpdateStatus::Available { .. }));
}

#[gpui::test]
fn no_installer_means_no_install_button(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("tempdir");
    let (fetch, _) = counting_fetch("v99.0.0");
    cx.update(|cx| updates::init(Some(dir.path().join("s.toml")), fetch, cx));
    cx.run_until_parked();
    assert!(!cx.update(|cx| updates::can_install(cx)));
    cx.update(updates::install);
    cx.run_until_parked();
    assert_eq!(step(cx), None, "nothing to install with");

    // An installer, but the release has nothing for this computer: still only Download.
    let download: updates::Download = Arc::new(|_, _, _| Err(std::io::Error::other("unused")));
    cx.update(|cx| {
        updates::set_installer(
            Some(updates::Installer {
                install: skyelab_core::update_install::Install::Deb,
                download,
                dir: dir.path().join("downloads"),
            }),
            cx,
        )
    });
    assert!(!cx.update(|cx| updates::can_install(cx)));
}

#[gpui::test]
fn a_failed_install_shows_on_the_next_start(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("tempdir");
    let downloads = dir.path().join("downloads");
    std::fs::create_dir_all(&downloads).expect("mkdir");
    let marker = downloads.join(skyelab_core::update_install::FAILURE_MARKER);
    std::fs::write(&marker, "v99.0.0\nthe installer failed (2)\n").expect("marker");
    // The marker is read when the installer is set, before the start check answers.
    let deb = installable(cx, dir.path(), b"package", b"package");
    match status(cx) {
        UpdateStatus::Updating {
            tag,
            step: updates::Step::Failed(e),
            ..
        } => {
            assert_eq!(tag, "v99.0.0");
            assert!(e.contains("installer failed (2)"), "{e}");
        }
        other => panic!("expected the failure, got {other:?}"),
    }
    assert!(!marker.exists(), "shown once");
    // Try again works: the start check's release was kept.
    cx.update(updates::install);
    cx.run_until_parked();
    assert_eq!(step(cx), Some(updates::Step::Ready));
    assert!(downloads.join(deb).exists());
}
