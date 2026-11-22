//! Regression tests for review findings: per-device state outliving its device, and Open URL while busy.

use super::devices::{DeviceSource, DevicesPanel};
use super::*;
use gpui::TestAppContext;
use skyelab_devices::{Device, DeviceKind, DeviceState, Listing, Platform};
use std::sync::{Arc, Mutex};

fn emulator(id: &str, state: DeviceState) -> Device {
    Device {
        id: id.into(),
        name: id.into(),
        platform: Platform::Android,
        os_version: None,
        state,
        kind: DeviceKind::Emulator,
    }
}

/// Lists whatever `android` holds now; records `open_url` calls.
#[derive(Default)]
struct Fake {
    android: Mutex<Vec<Device>>,
    opened: Mutex<Vec<String>>,
}

impl DeviceSource for Fake {
    fn list(&self) -> Listing {
        Listing {
            ios: Ok(Vec::new()),
            android: Ok(self.android.lock().expect("android lock").clone()),
        }
    }
    fn boot(&self, _: &Device) -> anyhow::Result<()> {
        Ok(())
    }
    fn shutdown(&self, _: &Device) -> anyhow::Result<()> {
        Ok(())
    }
    fn install(&self, _: &Device, _: &std::path::Path) -> anyhow::Result<()> {
        Ok(())
    }
    fn open_url(&self, _: &Device, url: &str) -> anyhow::Result<()> {
        self.opened.lock().expect("opened lock").push(url.into());
        Ok(())
    }
    fn screenshot(&self, _: &Device) -> anyhow::Result<Vec<u8>> {
        Ok(Vec::new())
    }
}

fn panel(
    cx: &mut TestAppContext,
    fake: Arc<Fake>,
) -> (Entity<DevicesPanel>, &mut gpui::VisualTestContext) {
    cx.update(gpui_component::init);
    let (panel, cx) = cx.add_window_view(|window, cx| DevicesPanel::new(fake, window, cx));
    cx.run_until_parked();
    (panel, cx)
}

#[gpui::test]
fn a_reused_serial_does_not_inherit_the_old_screenshot(cx: &mut TestAppContext) {
    let fake = Arc::new(Fake::default());
    *fake.android.lock().expect("android lock") =
        vec![emulator("emulator-5554", DeviceState::Booted)];
    let (panel, cx) = panel(cx, fake.clone());
    let pixel7 = emulator("emulator-5554", DeviceState::Booted);
    panel.update(cx, |p, cx| p.screenshot(&pixel7, cx));
    cx.run_until_parked();
    assert!(panel.read_with(cx, |p, _| p.ops["emulator-5554"].screenshot.is_some()));

    // Pixel_7 shuts down: its serial leaves the list.
    *fake.android.lock().expect("android lock") = vec![emulator("Pixel_7", DeviceState::Shutdown)];
    panel.update(cx, |p, cx| p.refresh(cx));
    cx.run_until_parked();
    // Another AVD boots and gets the same serial.
    *fake.android.lock().expect("android lock") =
        vec![emulator("emulator-5554", DeviceState::Booted)];
    panel.update(cx, |p, cx| p.refresh(cx));
    cx.run_until_parked();
    panel.read_with(cx, |p, _| {
        assert!(
            p.ops
                .get("emulator-5554")
                .is_none_or(|o| o.screenshot.is_none()),
            "the new emulator shows no stale screenshot"
        );
    });
}

#[gpui::test]
fn open_url_while_busy_keeps_the_row_and_the_url(cx: &mut TestAppContext) {
    let fake = Arc::new(Fake::default());
    *fake.android.lock().expect("android lock") =
        vec![emulator("emulator-5554", DeviceState::Booted)];
    let (panel, cx) = panel(cx, fake.clone());
    let device = emulator("emulator-5554", DeviceState::Booted);
    panel.update_in(cx, |p, window, cx| {
        p.show_url_row("emulator-5554", window, cx);
        p.url_input.update(cx, |input, cx| {
            input.set_value("https://example.com", window, cx)
        });
        p.screenshot(&device, cx);
        p.submit_url(cx);
        assert_eq!(
            p.url_for.as_deref(),
            Some("emulator-5554"),
            "row stays while busy"
        );
    });
    cx.run_until_parked();
    assert!(fake.opened.lock().expect("opened lock").is_empty());
    panel.update(cx, |p, cx| p.submit_url(cx));
    cx.run_until_parked();
    assert_eq!(
        *fake.opened.lock().expect("opened lock"),
        ["https://example.com"],
        "opens once the device is free"
    );
}
