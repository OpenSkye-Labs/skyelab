//! The device tools through `Workspace::device_call`, on a fake device source.

use super::device_agent::{DeviceClaims, png_size};
use super::devices::DeviceSource;
use super::threads_test::test_backend;
use super::*;
use gpui::{TestAppContext, VisualTestContext};
use serde_json::Value;
use skyelab_agent::mcp::{DeviceKey, DeviceTool};
use skyelab_devices::{Device, DeviceKind, DeviceState, Error, Holder, Input, Listing, Platform};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

const EMU: &str = "emulator-5554";

/// A booted emulator and a stopped one. Logs every call; each screenshot is a distinct PNG `n` px wide.
#[derive(Default)]
struct Fake {
    calls: Mutex<Vec<String>>,
    shots: AtomicU32,
    /// Another AVD booted under the emulator's serial.
    swapped: AtomicBool,
}

impl Fake {
    fn calls(&self) -> Vec<String> {
        self.calls.lock().expect("calls lock").clone()
    }
    fn log(&self, call: String) {
        self.calls.lock().expect("calls lock").push(call);
    }
}

fn device(id: &str, state: DeviceState) -> Device {
    Device {
        id: id.into(),
        name: format!("Device {id}"),
        platform: Platform::Android,
        os_version: Some("16".into()),
        state,
        kind: DeviceKind::Emulator,
    }
}

/// A PNG header saying `width` x 2400; enough for `png_size` and distinct per width.
fn png(width: u32) -> Vec<u8> {
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    png.extend(width.to_be_bytes());
    png.extend(2400u32.to_be_bytes());
    png
}

impl DeviceSource for Fake {
    fn list(&self) -> Listing {
        Listing {
            ios: Err(Error::NotAvailable(Platform::Ios)),
            android: Ok(vec![
                Device {
                    name: if self.swapped.load(Ordering::Relaxed) {
                        "Other AVD".into()
                    } else {
                        format!("Device {EMU}")
                    },
                    ..device(EMU, DeviceState::Booted)
                },
                device("Stopped_AVD", DeviceState::Shutdown),
                Device {
                    kind: DeviceKind::Physical,
                    ..device("R5CT10PHONE", DeviceState::Booted)
                },
            ]),
        }
    }
    fn boot(&self, _: &Device) -> anyhow::Result<()> {
        anyhow::bail!("not in this fake")
    }
    fn shutdown(&self, _: &Device) -> anyhow::Result<()> {
        anyhow::bail!("not in this fake")
    }
    fn install(&self, _: &Device, _: &std::path::Path) -> anyhow::Result<()> {
        anyhow::bail!("not in this fake")
    }
    fn open_url(&self, _: &Device, _: &str) -> anyhow::Result<()> {
        anyhow::bail!("not in this fake")
    }
    fn screenshot(&self, d: &Device) -> anyhow::Result<Vec<u8>> {
        self.log(format!("screenshot {}", d.id));
        Ok(png(1000 + self.shots.fetch_add(1, Ordering::Relaxed)))
    }
    fn input(&self, d: &Device, input: &Input) -> anyhow::Result<()> {
        self.log(format!("input {} {input:?}", d.id));
        Ok(())
    }
}

fn workspace(cx: &mut TestAppContext) -> (Entity<Workspace>, Arc<Fake>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::settings::init(cx);
        skyelab_terminal::init(cx);
        init(cx);
    });
    let fake = Arc::new(Fake::default());
    let source = fake.clone();
    let (ws, cx) = cx.add_window_view(|window, cx| {
        let mut ws = Workspace::new(
            std::env::temp_dir(),
            None,
            AccountsConfig::default(),
            None,
            test_backend(),
            window,
            cx,
        );
        ws.device_source = Some(source);
        ws
    });
    cx.run_until_parked();
    (ws, fake, cx)
}

fn call(
    ws: &Entity<Workspace>,
    thread: i64,
    tool: DeviceTool,
    cx: &mut VisualTestContext,
) -> Result<Value, String> {
    let (reply, answer) = flume::bounded(1);
    ws.update_in(cx, |ws, window, cx| {
        ws.device_call(thread, tool, reply, window, cx)
    });
    cx.run_until_parked();
    answer.try_recv().expect("answered once parked")
}

fn open(id: &str) -> DeviceTool {
    DeviceTool::Open {
        device_id: id.into(),
    }
}

fn tap(id: &str) -> DeviceTool {
    DeviceTool::Tap {
        device_id: id.into(),
        x: 10,
        y: 20,
    }
}

/// The panel's current screenshot of the emulator, by image id.
fn panel_shot(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> Option<u64> {
    ws.read_with(cx, |ws, cx| {
        let panel = ws.devices.as_ref()?.upgrade()?;
        Some(panel.read(cx).ops.get(EMU)?.screenshot.as_ref()?.id())
    })
}

#[gpui::test]
fn one_thread_per_device(cx: &mut TestAppContext) {
    let (ws, fake, cx) = workspace(cx);
    let opened = call(&ws, 1, open(EMU), cx).expect("thread 1 opens it");
    assert_eq!(opened["device_id"], EMU);
    assert!(
        panel_shot(&ws, cx).is_some(),
        "the panel opened with its screenshot"
    );

    let err = call(&ws, 2, open(EMU), cx).expect_err("thread 2 is refused");
    assert!(err.contains("another thread"), "{err}");
    let err = call(&ws, 2, tap(EMU), cx).expect_err("no input on a device it doesn't hold");
    assert!(err.contains("device_open"), "{err}");
    assert!(
        !fake.calls().iter().any(|c| c.starts_with("input")),
        "{:?}",
        fake.calls()
    );

    let list = call(&ws, 2, DeviceTool::List, cx).expect("list");
    assert_eq!(list["devices"][0]["held_by"], "another thread", "{list}");
    let list = call(&ws, 1, DeviceTool::List, cx).expect("list");
    assert_eq!(list["devices"][0]["held_by"], "you", "{list}");
    assert_eq!(list["devices"][1]["held_by"], Value::Null, "{list}");

    let closed = call(
        &ws,
        1,
        DeviceTool::Close {
            device_id: EMU.into(),
        },
        cx,
    )
    .expect("close");
    assert_eq!(closed["released"], true);
    call(&ws, 2, open(EMU), cx).expect("free again");
}

#[gpui::test]
fn input_runs_on_the_claimed_device_and_refreshes_the_panel(cx: &mut TestAppContext) {
    let (ws, fake, cx) = workspace(cx);
    call(&ws, 1, open(EMU), cx).expect("open");
    let before = panel_shot(&ws, cx);
    call(&ws, 1, tap(EMU), cx).expect("tap");
    let typed = DeviceTool::Type {
        device_id: EMU.into(),
        text: "hi".into(),
    };
    call(&ws, 1, typed, cx).expect("type");
    let home = DeviceTool::Press {
        device_id: EMU.into(),
        key: DeviceKey::Home,
    };
    call(&ws, 1, home, cx).expect("press");
    let calls = fake.calls();
    assert_eq!(
        calls
            .iter()
            .filter(|c| c.starts_with("input"))
            .cloned()
            .collect::<Vec<_>>(),
        [
            format!("input {EMU} Tap {{ x: 10, y: 20 }}"),
            format!("input {EMU} Text(\"hi\")"),
            format!("input {EMU} Key(Home)"),
        ]
    );
    assert_eq!(
        calls.iter().filter(|c| c.starts_with("screenshot")).count(),
        4,
        "one on open, one after each action: {calls:?}"
    );
    assert_ne!(
        panel_shot(&ws, cx),
        before,
        "the panel shows the newest screen"
    );
}

#[gpui::test]
fn screenshot_answers_an_image_with_its_size(cx: &mut TestAppContext) {
    let (ws, _, cx) = workspace(cx);
    let shot = DeviceTool::Screenshot {
        device_id: EMU.into(),
    };
    let err = call(&ws, 1, shot.clone(), cx).expect_err("not opened");
    assert!(err.contains("device_open"), "{err}");
    call(&ws, 1, open(EMU), cx).expect("open");
    let shot = call(&ws, 1, shot, cx).expect("shot");
    assert_eq!(shot["image"]["mimeType"], "image/png");
    assert_eq!(
        (shot["width"].as_u64(), shot["height"].as_u64()),
        (Some(1001), Some(2400))
    );
    use base64::Engine as _;
    let png = base64::engine::general_purpose::STANDARD
        .decode(shot["image"]["data"].as_str().expect("data"))
        .expect("base64");
    assert_eq!(png_size(&png), Some((1001, 2400)));
}

#[gpui::test]
fn take_back_stops_the_agent_until_given_back(cx: &mut TestAppContext) {
    let (ws, fake, cx) = workspace(cx);
    call(&ws, 1, open(EMU), cx).expect("open");
    cx.update(|_, cx| DeviceClaims::take_back(EMU, true, cx));
    let err = call(&ws, 1, tap(EMU), cx).expect_err("taken back");
    assert!(err.contains("took device"), "{err}");
    let err = call(&ws, 2, open(EMU), cx).expect_err("no other thread either");
    assert!(err.contains("took device"), "{err}");
    assert!(!fake.calls().iter().any(|c| c.starts_with("input")));
    cx.update(|_, cx| DeviceClaims::take_back(EMU, false, cx));
    call(&ws, 2, open(EMU), cx).expect("given back");
}

#[gpui::test]
fn only_booted_known_devices_open(cx: &mut TestAppContext) {
    let (ws, _, cx) = workspace(cx);
    let err = call(&ws, 1, open("Stopped_AVD"), cx).expect_err("not booted");
    assert!(
        err.contains("shut down") && err.contains("Devices panel"),
        "{err}"
    );
    let err = call(&ws, 1, open("emulator-9999"), cx).expect_err("unknown");
    assert!(err.contains("device_list"), "{err}");
    let held = cx.update(|_, cx| cx.global::<DeviceClaims>().claims.holder("Stopped_AVD"));
    assert_eq!(held, None);
}

#[gpui::test]
fn a_closed_thread_frees_its_devices(cx: &mut TestAppContext) {
    let (ws, _, cx) = workspace(cx);
    call(&ws, 7, open(EMU), cx).expect("open");
    let held = cx.update(|_, cx| cx.global::<DeviceClaims>().claims.holder(EMU));
    assert_eq!(held, Some(Holder::Thread(7)));
    cx.update(|_, cx| DeviceClaims::release_thread(7, cx));
    call(&ws, 8, open(EMU), cx).expect("free after the thread closed");
}

#[gpui::test]
fn a_physical_phone_is_refused(cx: &mut TestAppContext) {
    let (ws, _, cx) = workspace(cx);
    let err = call(&ws, 1, open("R5CT10PHONE"), cx).expect_err("a real phone");
    assert!(err.contains("physical"), "{err}");
    let held = cx.update(|_, cx| cx.global::<DeviceClaims>().claims.holder("R5CT10PHONE"));
    assert_eq!(held, None, "the claim taken before the listing is undone");
}

/// Review: a new AVD booted under the same serial must not inherit the claim.
#[gpui::test]
fn a_reused_serial_is_not_the_opened_device(cx: &mut TestAppContext) {
    let (ws, fake, cx) = workspace(cx);
    call(&ws, 1, open(EMU), cx).expect("open");
    fake.swapped.store(true, Ordering::Relaxed);
    let err = call(&ws, 1, tap(EMU), cx).expect_err("another AVD now");
    assert!(err.contains("no longer booted"), "{err}");
    assert!(
        !fake.calls().iter().any(|c| c.starts_with("input")),
        "{:?}",
        fake.calls()
    );
    let held = cx.update(|_, cx| cx.global::<DeviceClaims>().claims.holder(EMU));
    assert_eq!(held, None, "the stale claim is gone");
    call(&ws, 2, open(EMU), cx).expect("the new AVD is free");
}

/// Review: a thread closed while device_open lists leaves no claim behind.
#[gpui::test]
fn a_thread_closed_while_opening_leaves_no_claim(cx: &mut TestAppContext) {
    let (ws, _, cx) = workspace(cx);
    let (reply, answer) = flume::bounded(1);
    ws.update_in(cx, |ws, window, cx| {
        ws.device_call(3, open(EMU), reply, window, cx)
    });
    cx.update(|_, cx| DeviceClaims::release_thread(3, cx));
    cx.run_until_parked();
    let err = answer
        .try_recv()
        .expect("answered")
        .expect_err("closed meanwhile");
    assert!(err.contains("device_open"), "{err}");
    let held = cx.update(|_, cx| cx.global::<DeviceClaims>().claims.holder(EMU));
    assert_eq!(held, None);
}
