//! Acceptance 5: when threads are active, Restart to update asks instead of killing work; "Update when threads
//! finish" keeps `install::swap` uncalled while busy and applies once after becoming idle (including in background
//! mode); "Restart now" still applies immediately; Cancel disarms; swap failures surface cleanly without quitting.

use crate::test_wait::wait_until;
use crate::updates::{self, Download, Fetch, Installer, Step, UpdateStatus, Updates};
use crate::workspace::background::{self, IDLE_GRACE};
use crate::workspace::background_keep_test::{approve, hidden_hub, quits, waiting_thread};
use crate::workspace::manage_harness::rig;
use gpui::{Modifiers, TestAppContext, VisualTestContext};
use gpui_component::WindowExt as _;
use skyelab_client::release::Release;
use skyelab_core::update::{Asset, CHECKSUMS, RELEASES_PAGE};
use skyelab_core::update_install::{self as install, Install, Staged};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

fn stage_appimage(cx: &mut VisualTestContext, dir: &Path) -> (PathBuf, Arc<AtomicUsize>) {
    let target = dir.join("SkyeLab.AppImage");
    std::fs::write(&target, b"old-version").expect("write old target");
    let body: &'static [u8] = b"new-version";
    let asset = format!("SkyeLab_99.0.0_{}.AppImage", std::env::consts::ARCH);
    let probe = dir.join("probe");
    std::fs::write(&probe, body).expect("write probe");
    let sums = format!(
        "{}  {asset}\n",
        install::sha256_file(&probe).expect("sha256")
    );
    let assets: Vec<Asset> = [asset.as_str(), CHECKSUMS]
        .iter()
        .map(|n| Asset {
            name: (*n).into(),
            url: format!("{RELEASES_PAGE}download/v99.0.0/{n}"),
        })
        .collect();
    let fetch: Fetch = Arc::new(move |_| {
        Ok(Release {
            tag: "v99.0.0".into(),
            url: format!("{RELEASES_PAGE}tag/v99.0.0"),
            assets: assets.clone(),
        })
    });
    let download: Download = Arc::new(move |url, out, progress| {
        let bytes = if url.ends_with(CHECKSUMS) {
            sums.as_bytes()
        } else {
            body
        };
        out.write_all(bytes)?;
        progress(bytes.len() as u64, Some(bytes.len() as u64));
        Ok(())
    });
    let swaps = Arc::new(AtomicUsize::new(0));
    let seen = swaps.clone();
    let applier: updates::defer::Apply = Arc::new(move |staged| {
        seen.fetch_add(1, Ordering::SeqCst);
        install::recheck(staged)?;
        if let Staged::Swap { new, target, .. } = staged {
            install::swap(new, target)?;
        }
        Ok(())
    });
    let cache = dir.join("cache");
    let settings = dir.join("settings.toml");
    let target_for_installer = target.clone();
    TestAppContext::update(cx, move |cx| {
        updates::init(Some(settings), fetch, cx);
        updates::set_installer(
            Some(Installer {
                install: Install::AppImage(target_for_installer),
                download,
                dir: cache,
            }),
            cx,
        );
        updates::defer::set_applier(Some(applier), cx);
    });
    cx.run_until_parked();
    TestAppContext::update(cx, updates::install);
    wait_until(cx, "update staged", |cx| {
        matches!(
            TestAppContext::update(cx, |cx| cx.global::<Updates>().status.clone()),
            UpdateStatus::Updating {
                step: Step::Ready,
                ..
            }
        )
    });
    (target, swaps)
}

fn step(cx: &mut VisualTestContext) -> Option<Step> {
    match TestAppContext::update(cx, |cx| cx.global::<Updates>().status.clone()) {
        UpdateStatus::Updating { step, .. } => Some(step),
        _ => None,
    }
}

#[gpui::test]
fn restart_while_busy_asks_and_defers_swap_until_idle(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("tempdir");
    let (_r, vcx) = rig(cx);
    let view = waiting_thread(vcx);
    let (target, swaps) = stage_appimage(vcx, dir.path());

    // Busy: Restart to update opens the choice dialog instead of swapping.
    TestAppContext::update(vcx, updates::restart);
    vcx.run_until_parked();
    assert!(
        vcx.update(|window, cx| window.has_active_dialog(cx)),
        "busy restart asks first"
    );
    assert_eq!(
        swaps.load(Ordering::SeqCst),
        0,
        "swap not called while busy"
    );
    assert_eq!(quits(vcx), 0);
    assert_eq!(std::fs::read(&target).expect("read"), b"old-version");

    // Pick "Update when threads finish".
    let btn = vcx
        .debug_bounds("update-when-idle")
        .expect("Update when threads finish button is drawn");
    vcx.simulate_click(btn.center(), Modifiers::none());
    vcx.run_until_parked();
    assert!(TestAppContext::update(vcx, |cx| updates::is_deferred(cx)));
    assert_eq!(step(vcx), Some(Step::Deferred));

    // Still busy: even well past IDLE_GRACE, swap is NOT called.
    vcx.executor().advance_clock(IDLE_GRACE * 3);
    vcx.run_until_parked();
    assert_eq!(swaps.load(Ordering::SeqCst), 0, "still busy: no swap");
    assert_eq!(quits(vcx), 0);
    assert_eq!(std::fs::read(&target).expect("read"), b"old-version");

    // Work finishes: waits for IDLE_GRACE, then swaps once and quits.
    approve(&view, vcx);
    wait_until(vcx, "thread finishes", |cx| {
        !TestAppContext::update(cx, |cx| background::is_busy(cx))
    });
    vcx.executor().advance_clock(Duration::from_secs(10));
    vcx.run_until_parked();
    assert_eq!(
        swaps.load(Ordering::SeqCst),
        0,
        "idle grace not elapsed yet"
    );

    vcx.executor().advance_clock(IDLE_GRACE);
    wait_until(vcx, "deferred swap to complete and quit", |cx| {
        swaps.load(Ordering::SeqCst) == 1 && quits(cx) == 1
    });
    assert_eq!(
        std::fs::read(&target).expect("read"),
        b"new-version",
        "staged update swapped in once idle"
    );
}

#[gpui::test]
fn deferred_update_applies_on_background_idle_quit(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("tempdir");
    let (_r, vcx) = rig(cx);
    let view = waiting_thread(vcx);
    let (target, swaps) = stage_appimage(vcx, dir.path());

    TestAppContext::update(vcx, updates::defer_until_idle);
    TestAppContext::update(vcx, background::enter);
    vcx.run_until_parked();
    let hub = hidden_hub(vcx).expect("hidden hub");
    let mut hcx = VisualTestContext::from_window(hub, cx);

    hcx.executor().advance_clock(IDLE_GRACE * 3);
    hcx.run_until_parked();
    assert_eq!(
        swaps.load(Ordering::SeqCst),
        0,
        "busy in background: no swap"
    );
    assert_eq!(quits(&mut hcx), 0);

    approve(&view, &mut hcx);
    wait_until(&mut hcx, "thread finishes", |cx| {
        !TestAppContext::update(cx, |cx| background::is_busy(cx))
    });
    hcx.executor()
        .advance_clock(IDLE_GRACE + background::IDLE_POLL);
    wait_until(&mut hcx, "background idle quit applies update once", |cx| {
        swaps.load(Ordering::SeqCst) == 1 && quits(cx) == 1
    });
    assert_eq!(std::fs::read(&target).expect("read"), b"new-version");
}

#[gpui::test]
fn cancel_disarms_defer_and_restart_now_still_works(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("tempdir");
    let (_r, vcx) = rig(cx);
    let view = waiting_thread(vcx);
    let (target, swaps) = stage_appimage(vcx, dir.path());

    TestAppContext::update(vcx, updates::defer_until_idle);
    assert_eq!(step(vcx), Some(Step::Deferred));
    TestAppContext::update(vcx, updates::cancel_deferred);
    assert_eq!(step(vcx), Some(Step::Ready));

    approve(&view, vcx);
    wait_until(vcx, "thread finishes", |cx| {
        !TestAppContext::update(cx, |cx| background::is_busy(cx))
    });
    vcx.executor().advance_clock(IDLE_GRACE * 3);
    vcx.run_until_parked();
    assert_eq!(
        swaps.load(Ordering::SeqCst),
        0,
        "cancelled: no swap on idle"
    );
    assert_eq!(quits(vcx), 0);

    // Start busy work again, click Restart to update, and choose "Restart now".
    let _busy = waiting_thread(vcx);
    TestAppContext::update(vcx, updates::restart);
    vcx.run_until_parked();
    let now_btn = vcx
        .debug_bounds("update-restart-now")
        .expect("Restart now button is drawn");
    vcx.simulate_click(now_btn.center(), Modifiers::none());
    wait_until(vcx, "Restart now swaps immediately", |cx| {
        swaps.load(Ordering::SeqCst) == 1 && quits(cx) == 1
    });
    assert_eq!(std::fs::read(&target).expect("read"), b"new-version");
}

#[gpui::test]
fn failed_swap_surfaces_error_without_quitting(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("tempdir");
    let (_r, vcx) = rig(cx);
    let (target, _) = stage_appimage(vcx, dir.path());
    TestAppContext::update(vcx, |cx| {
        updates::defer::set_applier(
            Some(Arc::new(|_| anyhow::bail!("permission denied on swap"))),
            cx,
        );
    });
    TestAppContext::update(vcx, updates::restart);
    wait_until(vcx, "failed step shown", |cx| {
        matches!(step(cx), Some(Step::Failed(_)))
    });
    match step(vcx) {
        Some(Step::Failed(msg)) => assert!(msg.contains("permission denied on swap"), "{msg}"),
        other => panic!("expected Failed, got {other:?}"),
    }
    assert_eq!(quits(vcx), 0, "failed swap does not quit");
    assert_eq!(
        std::fs::read(&target).expect("read"),
        b"old-version",
        "old app left untouched"
    );
}
