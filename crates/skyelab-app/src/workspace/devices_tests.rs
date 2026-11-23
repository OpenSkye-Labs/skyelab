use super::devices::{DeviceSource, DevicesPanel, MISSING_XCODE, POLL, SectionView, badge};
use super::threads_test::test_backend;
use super::*;
use gpui::TestAppContext;
use skyelab_devices::{Device, DeviceKind, DeviceState, Error, Listing, Platform};
use std::sync::{Arc, Mutex};

fn device(id: &str, platform: Platform, state: DeviceState, kind: DeviceKind) -> Device {
    Device {
        id: id.into(),
        name: format!("Device {id}"),
        platform,
        os_version: Some("1.0".into()),
        state,
        kind,
    }
}

/// No Xcode; two Android devices, one booting. Shutdown fails; every call is recorded.
#[derive(Default)]
struct Fake {
    calls: Mutex<Vec<String>>,
}

impl Fake {
    fn log(&self, call: String) {
        self.calls.lock().expect("calls lock").push(call);
    }
}

impl DeviceSource for Fake {
    fn list(&self) -> Listing {
        self.log("list".into());
        Listing {
            ios: Err(Error::NotAvailable(Platform::Ios)),
            android: Ok(vec![
                device(
                    "emulator-5554",
                    Platform::Android,
                    DeviceState::Booted,
                    DeviceKind::Emulator,
                ),
                device(
                    "emulator-5556",
                    Platform::Android,
                    DeviceState::Booting,
                    DeviceKind::Emulator,
                ),
            ]),
        }
    }
    fn boot(&self, d: &Device) -> anyhow::Result<()> {
        self.log(format!("boot {}", d.id));
        Ok(())
    }
    fn shutdown(&self, d: &Device) -> anyhow::Result<()> {
        self.log(format!("shutdown {}", d.id));
        anyhow::bail!("adb: device {} is gone", d.id)
    }
    fn install(&self, d: &Device, app: &std::path::Path) -> anyhow::Result<()> {
        self.log(format!("install {} {}", d.id, app.display()));
        Ok(())
    }
    fn open_url(&self, d: &Device, url: &str) -> anyhow::Result<()> {
        self.log(format!("open {} {url}", d.id));
        Ok(())
    }
    fn screenshot(&self, d: &Device) -> anyhow::Result<Vec<u8>> {
        self.log(format!("screenshot {}", d.id));
        Ok(Vec::new())
    }
}

fn setup(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::settings::init(cx);
        skyelab_terminal::init(cx);
        init(cx);
    });
}

/// A workspace whose device source is `fake`, with the Devices tab opened by its key.
fn open(
    cx: &mut TestAppContext,
    fake: Arc<Fake>,
) -> (
    Entity<Workspace>,
    Entity<DevicesPanel>,
    &mut gpui::VisualTestContext,
) {
    setup(cx);
    let home = tempfile::tempdir().expect("tempdir");
    let folder = home.path().to_path_buf();
    let (ws, cx) = cx.add_window_view(|window, cx| {
        let mut ws = Workspace::new(
            folder,
            None,
            AccountsConfig::default(),
            None,
            test_backend(),
            window,
            cx,
        );
        ws.device_source = Some(fake);
        window.focus(&ws.focus, cx);
        ws
    });
    cx.run_until_parked();
    cx.simulate_keystrokes(OPEN_DEVICES_KEY);
    cx.run_until_parked();
    let panel = ws
        .read_with(cx, |ws, _| ws.devices.as_ref().and_then(|p| p.upgrade()))
        .expect("the key opened the Devices tab");
    (ws, panel, cx)
}

#[gpui::test]
fn open_devices_key_opens_one_tab(cx: &mut TestAppContext) {
    let (ws, panel, cx) = open(cx, Arc::default());
    ws.update_in(cx, |ws, window, cx| ws.focus.focus(window, cx));
    cx.simulate_keystrokes(OPEN_DEVICES_KEY);
    cx.run_until_parked();
    let again = ws.read_with(cx, |ws, _| ws.devices.as_ref().and_then(|p| p.upgrade()));
    assert_eq!(again.map(|p| p.entity_id()), Some(panel.entity_id()));
}

#[gpui::test]
fn lists_devices_grouped_with_badges_and_missing_toolchain(cx: &mut TestAppContext) {
    let (_, panel, cx) = open(cx, Arc::default());
    panel.read_with(cx, |panel, _| {
        assert_eq!(
            panel.section(Platform::Ios),
            SectionView::Missing(MISSING_XCODE)
        );
        let SectionView::Devices(android) = panel.section(Platform::Android) else {
            panic!("android devices listed");
        };
        let badges: Vec<_> = android
            .iter()
            .map(|d| (d.id.as_str(), badge(d.state)))
            .collect();
        assert_eq!(
            badges,
            [("emulator-5554", "Booted"), ("emulator-5556", "Booting…")]
        );
    });
    let all = [
        DeviceState::Booted,
        DeviceState::Booting,
        DeviceState::Shutdown,
        DeviceState::Offline,
    ]
    .map(badge);
    let distinct: std::collections::HashSet<_> = all.iter().collect();
    assert_eq!(distinct.len(), all.len(), "every state has its own badge");
}

#[gpui::test]
fn failed_operation_shows_error_on_that_device_only(cx: &mut TestAppContext) {
    let fake = Arc::new(Fake::default());
    let (_, panel, cx) = open(cx, fake.clone());
    let booted = device(
        "emulator-5554",
        Platform::Android,
        DeviceState::Booted,
        DeviceKind::Emulator,
    );
    panel.update(cx, |panel, cx| panel.shutdown(&booted, cx));
    panel.read_with(cx, |panel, _| {
        assert!(
            panel.ops["emulator-5554"].busy.is_some(),
            "spinner while running"
        );
    });
    cx.run_until_parked();
    panel.read_with(cx, |panel, _| {
        let ops = &panel.ops["emulator-5554"];
        assert_eq!(ops.busy, None, "spinner cleared");
        let error = ops.error.as_deref().expect("the error shows on the device");
        assert!(error.contains("emulator-5554 is gone"), "{error}");
        assert!(
            panel
                .ops
                .get("emulator-5556")
                .is_none_or(|o| o.error.is_none()),
            "the other device has no error"
        );
    });
    assert!(
        fake.calls
            .lock()
            .expect("calls lock")
            .contains(&"shutdown emulator-5554".to_string())
    );
}

#[gpui::test]
fn polling_stops_when_hidden(cx: &mut TestAppContext) {
    let fake = Arc::new(Fake::default());
    let (ws, panel, cx) = open(cx, fake.clone());
    let lists = || {
        fake.calls
            .lock()
            .expect("calls lock")
            .iter()
            .filter(|c| *c == "list")
            .count()
    };
    assert!(
        panel.read_with(cx, |p, _| p.poll.is_some()),
        "polls while shown"
    );
    let before = lists();
    cx.executor().advance_clock(POLL);
    cx.run_until_parked();
    assert!(lists() > before, "a tick refreshed the list");

    // Closing the right dock hides the Devices tab, so it stops rendering.
    ws.update_in(cx, |ws, window, cx| ws.focus.focus(window, cx));
    cx.simulate_keystrokes(super::layout::TOGGLE_RIGHT_KEY);
    cx.run_until_parked();
    cx.executor().advance_clock(POLL);
    cx.run_until_parked();
    cx.executor().advance_clock(POLL);
    cx.run_until_parked();
    assert!(
        panel.read_with(cx, |p, _| p.poll.is_none()),
        "polling stopped while hidden"
    );
    let hidden = lists();
    cx.executor().advance_clock(POLL * 3);
    cx.run_until_parked();
    assert_eq!(lists(), hidden, "no refresh while hidden");

    ws.update_in(cx, |ws, window, cx| ws.focus.focus(window, cx));
    cx.simulate_keystrokes(OPEN_DEVICES_KEY);
    cx.run_until_parked();
    assert!(
        panel.read_with(cx, |p, _| p.poll.is_some()),
        "polling resumes when shown"
    );
}
