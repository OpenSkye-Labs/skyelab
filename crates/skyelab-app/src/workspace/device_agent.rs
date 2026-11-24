//! The device tools on the UI thread. A thread drives only devices it claimed ([`DeviceClaims`]): never one another
//! thread holds or the user took back. The tools run on the background executor through the Devices panel's
//! source, and every agent action puts the new screenshot in the panel.

use super::Workspace;
use super::devices::{DeviceSource, badge};
use super::orchestrate::Reply;
use gpui::{App, AppContext as _, BorrowAppContext as _, Context, Global, SharedString, Window};
use serde_json::{Value, json};
use skyelab_agent::mcp::{DeviceKey, DeviceTool};
use skyelab_devices::{Claims, Device, DeviceKind, DeviceState, Holder, Input, Key, Listing};
use std::collections::HashMap;
use std::sync::Arc;

/// Which thread holds which device. App-wide: a device is one per machine, whichever window serves the call.
#[derive(Default)]
pub(super) struct DeviceClaims {
    pub claims: Claims,
    /// What each claim was opened on, so later calls need no new listing.
    devices: HashMap<String, Device>,
    /// Holder threads' titles, for the panel.
    pub titles: HashMap<i64, SharedString>,
}

impl Global for DeviceClaims {}

impl DeviceClaims {
    /// The device `thread` holds, or why it can't use it.
    fn owned(thread: i64, id: &str, cx: &App) -> Result<Device, String> {
        let this = cx.global::<Self>();
        this.claims.check(id, thread)?;
        this.devices
            .get(id)
            .cloned()
            .ok_or_else(|| format!("device {id} is gone; call device_list"))
    }

    /// A thread closed: its devices are free.
    pub(super) fn release_thread(thread: i64, cx: &mut App) {
        // Only when it held one: an update notifies every Devices panel.
        if cx
            .global::<Self>()
            .claims
            .threads()
            .any(|(_, t)| t == thread)
        {
            cx.update_global::<Self, _>(|c, _| {
                c.claims.release_thread(thread);
                c.titles.remove(&thread);
            });
        }
    }

    /// The user takes a device back from its thread, or (`back: false`) lets agents use it again.
    pub(super) fn take_back(id: &str, back: bool, cx: &mut App) {
        cx.update_global::<Self, _>(|c, _| {
            if back {
                c.claims.take_back(id)
            } else {
                c.claims.give_back(id)
            }
        });
    }
}

impl Workspace {
    pub(super) fn device_source(&mut self) -> Arc<dyn DeviceSource> {
        self.device_source
            .get_or_insert_with(|| Arc::new(super::devices::HubSource(hub())))
            .clone()
    }

    pub(super) fn device_call(
        &mut self,
        thread_id: i64,
        tool: DeviceTool,
        reply: Reply<Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let source = self.device_source();
        let (id, input) = match tool {
            DeviceTool::List => {
                let job = cx.background_spawn(async move { source.list() });
                cx.spawn(async move |_, cx| {
                    let listing = job.await;
                    let _ = reply.send(Ok(cx.update(|cx| list_json(&listing, thread_id, cx))));
                })
                .detach();
                return;
            }
            DeviceTool::Open { device_id } => {
                return self.device_open(thread_id, device_id, source, reply, window, cx);
            }
            DeviceTool::Close { device_id } => {
                let released = cx.update_global::<DeviceClaims, _>(|c, _| {
                    c.claims.release(&device_id, thread_id)
                });
                let _ = reply.send(Ok(json!({"device_id": device_id, "released": released})));
                return;
            }
            DeviceTool::Screenshot { device_id } => (device_id, None),
            DeviceTool::Tap { device_id, x, y } => (device_id, Some(Input::Tap { x, y })),
            DeviceTool::Type { device_id, text } => (device_id, Some(Input::Text(text))),
            DeviceTool::Press { device_id, key } => {
                let key = match key {
                    DeviceKey::Home => Key::Home,
                    DeviceKey::Back => Key::Back,
                };
                (device_id, Some(Input::Key(key)))
            }
        };
        let device = match DeviceClaims::owned(thread_id, &id, cx) {
            Ok(device) => device,
            Err(e) => {
                let _ = reply.send(Err(e));
                return;
            }
        };
        // An emulator's serial passes to the next AVD that boots: act only on the device the thread opened.
        let job = cx.background_spawn(async move { source.list() });
        cx.spawn(async move |this, cx| {
            let listing = job.await;
            let _ = this.update(cx, |ws, cx| {
                if let Err(e) = DeviceClaims::owned(thread_id, &id, cx)
                    .and_then(|_| still_opened(&listing, &device, thread_id, cx))
                {
                    let _ = reply.send(Err(e));
                    return;
                }
                match input {
                    None => ws.device_shot(thread_id, device, Some(reply), cx),
                    Some(input) => ws.device_input(thread_id, device, input, reply, cx),
                }
            });
        })
        .detach();
    }

    fn device_input(
        &mut self,
        thread_id: i64,
        device: Device,
        input: Input,
        reply: Reply<Value>,
        cx: &mut Context<Self>,
    ) {
        let (s, d) = (self.device_source(), device.clone());
        let job = cx.background_spawn(async move { s.input(&d, &input) });
        cx.spawn(async move |this, cx| {
            let done = job.await.map_err(|e| format!("{e:#}"));
            let ok = done.is_ok();
            let _ = reply.send(done.map(|()| json!({"ok": true})));
            if ok {
                // The panel follows the agent: one screenshot per action, no polling.
                let _ = this.update(cx, |ws, cx| ws.device_shot(thread_id, device, None, cx));
            }
        })
        .detach();
    }

    /// Lists afresh, claims a booted device for the thread, shows it in the Devices panel and screenshots it there.
    fn device_open(
        &mut self,
        thread_id: i64,
        id: String,
        source: Arc<dyn DeviceSource>,
        reply: Reply<Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Claimed now, while the caller is known live: a thread closed during the listing releases it.
        let title = self.open_view(thread_id, cx).map_or_else(
            || format!("thread {thread_id}").into(),
            |v| v.read(cx).title(),
        );
        let held =
            cx.global::<DeviceClaims>().claims.holder(&id) == Some(Holder::Thread(thread_id));
        let claimed = cx.update_global::<DeviceClaims, _>(|c, _| {
            c.claims.claim(&id, thread_id)?;
            c.titles.insert(thread_id, title);
            Ok::<_, String>(())
        });
        if let Err(e) = claimed {
            let _ = reply.send(Err(e));
            return;
        }
        let job = cx.background_spawn(async move { source.list() });
        cx.spawn_in(window, async move |this, cx| {
            let listing = job.await;
            let done = this.update_in(cx, |ws, window, cx| {
                let device = listing.devices().into_iter().find(|d| d.id == id).cloned();
                let usable = match &device {
                    _ if reply.is_disconnected() => Err("the call timed out".to_owned()),
                    None => Err(format!("no device {id}; call device_list")),
                    Some(d) if d.kind == DeviceKind::Physical => Err(format!(
                        "{} is a physical device; agents may only use simulators and emulators",
                        d.name
                    )),
                    Some(d) if d.state != DeviceState::Booted => Err(format!(
                        "{} is {}; ask the user to boot it in the Devices panel",
                        d.name,
                        badge(d.state).to_lowercase()
                    )),
                    Some(d) => Ok(d.clone()),
                };
                // The thread may have closed, or the user taken the device back, while it listed.
                cx.global::<DeviceClaims>().claims.check(&id, thread_id)?;
                let device = match usable {
                    Ok(d) => d,
                    Err(e) => {
                        if !held {
                            cx.update_global::<DeviceClaims, _>(|c, _| {
                                c.claims.release(&id, thread_id)
                            });
                        }
                        return Err(e);
                    }
                };
                let d = device.clone();
                cx.update_global::<DeviceClaims, _>(|c, _| c.devices.insert(d.id.clone(), d));
                ws.devices_panel(window, cx);
                ws.device_shot(thread_id, device.clone(), None, cx);
                Ok(json!({
                    "device_id": device.id,
                    "name": device.name,
                    "platform": device.platform.to_string(),
                    "os_version": device.os_version,
                }))
            });
            let _ = reply.send(done.unwrap_or_else(|_| Err("SkyeLab's window closed".into())));
        })
        .detach();
    }

    /// Screenshots the device into the Devices panel, and answers `reply` (the agent's screenshot) with it.
    /// Dropped if the thread lost the device meanwhile.
    fn device_shot(
        &mut self,
        thread_id: i64,
        device: Device,
        reply: Option<Reply<Value>>,
        cx: &mut Context<Self>,
    ) {
        let source = self.device_source();
        let d = device.clone();
        let job = cx.background_spawn(async move { source.screenshot(&d) });
        cx.spawn(async move |this, cx| {
            let shot = job.await.map_err(|e| format!("{e:#}"));
            let answer = this.update(cx, |ws, cx| {
                DeviceClaims::owned(thread_id, &device.id, cx)?;
                let png = shot?;
                let size = png_size(&png);
                let answer = image_json(&png, size);
                if let Some(panel) = ws.devices.as_ref().and_then(|p| p.upgrade()) {
                    panel.update(cx, |p, cx| p.show_screenshot(&device.id, png, cx));
                }
                Ok(answer)
            });
            if let Some(reply) = reply {
                let _ =
                    reply.send(answer.unwrap_or_else(|_| Err("SkyeLab's window closed".into())));
            }
        })
        .detach();
    }
}

/// Ok when `listing` still shows `device` booted under the same name; else the thread's claim goes.
fn still_opened(
    listing: &Listing,
    device: &Device,
    thread: i64,
    cx: &mut App,
) -> Result<(), String> {
    let same = listing
        .devices()
        .iter()
        .any(|d| d.id == device.id && d.name == device.name && d.state == DeviceState::Booted);
    if same {
        return Ok(());
    }
    cx.update_global::<DeviceClaims, _>(|c, _| c.claims.release(&device.id, thread));
    Err(format!(
        "{} ({}) is no longer booted; call device_list and device_open again",
        device.name, device.id
    ))
}

/// `{image: {data, mimeType}, width, height}`: the MCP server makes `image` an image block.
fn image_json(png: &[u8], size: Option<(u32, u32)>) -> Value {
    use base64::Engine as _;
    json!({
        "image": {"data": base64::engine::general_purpose::STANDARD.encode(png), "mimeType": "image/png"},
        "width": size.map(|s| s.0),
        "height": size.map(|s| s.1),
        "bytes": png.len(),
    })
}

/// A PNG's width and height, from its IHDR chunk.
pub(super) fn png_size(png: &[u8]) -> Option<(u32, u32)> {
    let be = |at: usize| Some(u32::from_be_bytes(png.get(at..at + 4)?.try_into().ok()?));
    (png.starts_with(b"\x89PNG\r\n\x1a\n") && png.get(12..16) == Some(b"IHDR"))
        .then(|| Some((be(16)?, be(20)?)))
        .flatten()
}

fn list_json(listing: &Listing, thread: i64, cx: &App) -> Value {
    let claims = &cx.global::<DeviceClaims>().claims;
    let devices: Vec<Value> = listing
        .devices()
        .into_iter()
        .map(|d| {
            let held_by = match claims.holder(&d.id) {
                Some(Holder::Thread(t)) if t == thread => Some("you"),
                Some(Holder::Thread(_)) => Some("another thread"),
                Some(Holder::User) => Some("the user"),
                None => None,
            };
            json!({
                "device_id": d.id,
                "name": d.name,
                "platform": d.platform.to_string(),
                "os_version": d.os_version,
                "state": badge(d.state),
                "held_by": held_by,
            })
        })
        .collect();
    let error =
        |r: &Result<Vec<Device>, skyelab_devices::Error>| r.as_ref().err().map(|e| e.to_string());
    json!({
        "devices": devices,
        "ios_error": error(&listing.ios),
        "android_error": error(&listing.android),
    })
}

/// The device tools: found on this machine, or in debug builds `SKYELAB_ADB_BIN` / `SKYELAB_XCRUN_BIN` (fakes).
fn hub() -> skyelab_devices::DeviceHub {
    let mut hub = skyelab_devices::DeviceHub::discover();
    if cfg!(debug_assertions) {
        if let Some(adb) = std::env::var_os("SKYELAB_ADB_BIN") {
            (hub.adb, hub.emulator) = (Some(adb.into()), None);
        }
        if let Some(xcrun) = std::env::var_os("SKYELAB_XCRUN_BIN") {
            hub.xcrun = Some(xcrun.into());
        }
    }
    hub
}
