//! The device tools: an agent claims an iOS Simulator or Android emulator for its thread, screenshots it and
//! sends taps, text and keys. Input is checked here for shape and size; the app checks the claim.

use super::tools::bounded;
use serde_json::{Map, Value, json};

const MAX_DEVICE_ID: usize = 128;
/// Largest tap coordinate, in screen pixels.
pub(super) const MAX_COORD: u64 = 10_000;
pub(super) const MAX_TYPE: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKey {
    Home,
    Back,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceTool {
    List,
    /// Claims the device for this thread: one thread per device.
    Open {
        device_id: String,
    },
    /// A PNG of the screen, answered as an MCP image block.
    Screenshot {
        device_id: String,
    },
    Tap {
        device_id: String,
        x: u32,
        y: u32,
    },
    Type {
        device_id: String,
        text: String,
    },
    Press {
        device_id: String,
        key: DeviceKey,
    },
    /// Releases the claim.
    Close {
        device_id: String,
    },
}

pub(super) fn list() -> [Value; 7] {
    let id = json!({"type": "string", "minLength": 1, "maxLength": MAX_DEVICE_ID,
        "description": "A device id from device_list."});
    let one = |name: &str, description: &str| {
        json!({
            "name": name,
            "description": description,
            "inputSchema": {"type": "object", "properties": {"device_id": id}, "required": ["device_id"]},
        })
    };
    let coord = json!({"type": "integer", "minimum": 0, "maximum": MAX_COORD});
    [
        json!({
            "name": "device_list",
            "description": "iOS Simulators and Android emulators/devices on this machine, with their state and \
                who holds each (you, another thread, the user, or nobody).",
            "inputSchema": {"type": "object", "properties": {}},
        }),
        one(
            "device_open",
            "Claim a booted device for this thread so you can screenshot it and send input. A device has one \
                thread at a time; the user sees that you use it and can take it back. Ask the user to boot one \
                in the Devices panel if none is booted.",
        ),
        one(
            "device_screenshot",
            "A PNG of your device's screen, with its width and height in pixels (the coordinates device_tap takes).",
        ),
        json!({
            "name": "device_tap",
            "description": "Tap your device's screen at x, y in screen pixels. Android only.",
            "inputSchema": {
                "type": "object",
                "properties": {"device_id": id, "x": coord, "y": coord},
                "required": ["device_id", "x", "y"],
            },
        }),
        json!({
            "name": "device_type",
            "description": "Type printable ASCII text into the focused field of your device. Android only.",
            "inputSchema": {
                "type": "object",
                "properties": {"device_id": id, "text": {"type": "string", "minLength": 1, "maxLength": MAX_TYPE}},
                "required": ["device_id", "text"],
            },
        }),
        json!({
            "name": "device_press",
            "description": "Press Home or Back on your device. Android only.",
            "inputSchema": {
                "type": "object",
                "properties": {"device_id": id, "key": {"type": "string", "enum": ["home", "back"]}},
                "required": ["device_id", "key"],
            },
        }),
        one(
            "device_close",
            "Release a device you opened, so another thread can use it.",
        ),
    ]
}

pub(super) fn parse(name: &str, args: &Map<String, Value>) -> Option<Result<DeviceTool, String>> {
    let id = || device_id(args);
    Some(match name {
        "device_list" => Ok(DeviceTool::List),
        "device_open" => id().map(|device_id| DeviceTool::Open { device_id }),
        "device_screenshot" => id().map(|device_id| DeviceTool::Screenshot { device_id }),
        "device_close" => id().map(|device_id| DeviceTool::Close { device_id }),
        "device_tap" => (|| {
            Ok(DeviceTool::Tap {
                device_id: id()?,
                x: coord(args, "x")?,
                y: coord(args, "y")?,
            })
        })(),
        "device_type" => (|| {
            let text = bounded(args, "text", MAX_TYPE)?;
            if !text.chars().all(|c| c.is_ascii() && !c.is_ascii_control()) {
                return Err(
                    "text must be printable ASCII (no newlines or other control characters)".into(),
                );
            }
            Ok(DeviceTool::Type {
                device_id: id()?,
                text,
            })
        })(),
        "device_press" => (|| {
            let key = match args.get("key").and_then(Value::as_str) {
                Some("home") => DeviceKey::Home,
                Some("back") => DeviceKey::Back,
                _ => return Err("key must be \"home\" or \"back\"".into()),
            };
            Ok(DeviceTool::Press {
                device_id: id()?,
                key,
            })
        })(),
        _ => return None,
    })
}

/// A simulator UDID, an adb serial (`emulator-5554`, `192.168.1.5:5555`) or an AVD name: never a flag.
fn device_id(args: &Map<String, Value>) -> Result<String, String> {
    let id = bounded(args, "device_id", MAX_DEVICE_ID)?;
    let ok = !id.starts_with('-')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '-'));
    ok.then_some(id).ok_or_else(|| {
        "device_id must be letters, digits, '.', '_', ':' or '-', not starting with '-'".into()
    })
}

fn coord(args: &Map<String, Value>, field: &str) -> Result<u32, String> {
    args.get(field)
        .and_then(Value::as_u64)
        .filter(|n| *n <= MAX_COORD)
        .map(|n| n as u32)
        .ok_or_else(|| format!("{field} must be an integer from 0 to {MAX_COORD}"))
}

#[cfg(test)]
mod tests;
