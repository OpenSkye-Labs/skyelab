//! Agent input: taps, text and keys, sent through `adb shell input`.

use crate::{Device, DeviceHub, Error, Platform};

/// Input an agent sends to a device. Android only: `simctl` has no input commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// A tap at screen pixels.
    Tap {
        x: u32,
        y: u32,
    },
    /// Printable ASCII typed into the focused field.
    Text(String),
    Key(Key),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Home,
    Back,
}

impl DeviceHub {
    /// Sends `input` to an Android device (`adb shell input …`). iOS Simulators take none: `simctl` has no input.
    pub fn input(&self, device: &Device, input: &Input) -> Result<(), Error> {
        if device.platform == Platform::Ios {
            return Err(Error::Unsupported(
                "iOS Simulators take no taps, text or keys through simctl; use Android, or ask the user".into(),
            ));
        }
        match input {
            Input::Tap { x, y } => {
                let (x, y) = (x.to_string(), y.to_string());
                self.adb(&device.id, &["shell", "input", "tap", &x, &y])
            }
            // `input text` reads %s as a space and stops at a raw one.
            Input::Text(text) => {
                self.adb_shell(&device.id, &["input", "text"], &text.replace(' ', "%s"))
            }
            Input::Key(key) => {
                let code = match key {
                    Key::Home => "KEYCODE_HOME",
                    Key::Back => "KEYCODE_BACK",
                };
                self.adb(&device.id, &["shell", "input", "keyevent", code])
            }
        }
        .map(drop)
    }
}
