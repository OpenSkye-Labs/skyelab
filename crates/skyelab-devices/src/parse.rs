use std::collections::BTreeMap;

use serde::Deserialize;

use crate::{Device, DeviceKind, DeviceState, Error, Platform};

/// Parses `xcrun simctl list devices --json`. Unavailable simulators (no runtime) are dropped: they can't boot.
pub fn parse_simctl_list(json: &[u8]) -> Result<Vec<Device>, Error> {
    #[derive(Deserialize)]
    struct Raw {
        devices: BTreeMap<String, Vec<Sim>>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Sim {
        udid: String,
        name: String,
        state: String,
        #[serde(default)]
        is_available: bool,
    }
    let raw: Raw = serde_json::from_slice(json)?;
    Ok(raw
        .devices
        .into_iter()
        .flat_map(|(runtime, sims)| {
            let os_version = runtime_version(&runtime);
            sims.into_iter()
                .filter(|s| s.is_available)
                .map(move |s| Device {
                    id: s.udid,
                    name: s.name,
                    platform: Platform::Ios,
                    os_version: os_version.clone(),
                    state: match s.state.as_str() {
                        "Booted" => DeviceState::Booted,
                        "Shutdown" => DeviceState::Shutdown,
                        _ => DeviceState::Offline,
                    },
                    kind: DeviceKind::Simulator,
                })
        })
        .collect())
}

fn runtime_version(runtime: &str) -> Option<String> {
    let name = runtime.rsplit('.').next()?;
    let (os, version) = name.split_once('-')?;
    Some(format!("{os} {}", version.replace('-', ".")))
}

/// Parses `adb devices -l`. `os_version` is left `None`; it needs a call per device.
pub fn parse_adb_devices(text: &str) -> Vec<Device> {
    text.lines()
        .skip_while(|l| !l.starts_with("List of devices attached"))
        .skip(1)
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let serial = words.next()?;
            let state = words.next()?;
            let name = line
                .split_whitespace()
                .find_map(|w| w.strip_prefix("model:"))
                .unwrap_or(serial);
            Some(Device {
                id: serial.to_owned(),
                name: name.to_owned(),
                platform: Platform::Android,
                os_version: None,
                state: if state == "device" {
                    DeviceState::Booted
                } else {
                    DeviceState::Offline
                },
                kind: if serial.starts_with("emulator-") {
                    DeviceKind::Emulator
                } else {
                    DeviceKind::Physical
                },
            })
        })
        .collect()
}

/// Parses `emulator -list-avds`. Some emulator versions print `INFO | …` lines on stdout too; AVD names are
/// only `[A-Za-z0-9._-]`, so anything else is skipped.
pub fn parse_avd_list(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| {
            !l.is_empty()
                && l.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        })
        .map(str::to_owned)
        .collect()
}
