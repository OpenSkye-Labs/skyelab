//! The device hub core: lists and controls iOS Simulators and Android emulators/devices.
//!
//! No GPUI, no async runtime. Every operation runs `xcrun simctl`, `adb` or `emulator` directly with explicit
//! argv, never through a local shell. Calls block until the tool exits, so a UI runs them off the main thread.

mod claims;
mod emulator;
mod input;
mod parse;

use std::collections::HashSet;
use std::env::{self, consts::EXE_SUFFIX};
use std::ffi::OsStr;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub use claims::{Claims, Holder};
pub use emulator::Emulator;
pub use input::{Input, Key};
pub use parse::{parse_adb_devices, parse_avd_list, parse_simctl_list};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Platform {
    Ios,
    Android,
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Platform::Ios => "iOS",
            Platform::Android => "Android",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceState {
    Booted,
    Shutdown,
    /// Starting up and not usable yet: a simulator mid-boot, or an emulator adb sees before `sys.boot_completed`.
    Booting,
    /// Known but not usable now: an offline or unauthorized adb device, a simulator shutting down.
    Offline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceKind {
    Simulator,
    Emulator,
    Physical,
}

/// A simulator, emulator or device.
///
/// `id` is what the tools take: a simulator's UDID, an adb serial (`emulator-5554`), or, for an Android emulator
/// that isn't running, its AVD name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub platform: Platform,
    /// `iOS 27.0` for simulators, the Android release (`16`) for online Android devices, else `None`.
    pub os_version: Option<String>,
    pub state: DeviceState,
    pub kind: DeviceKind,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The tool this needs wasn't found: `xcrun` for iOS; `adb`, or `emulator` to boot an AVD, for Android.
    #[error("{0} tools are not available")]
    NotAvailable(Platform),
    #[error("`{tool} {args}` failed: {stderr}")]
    Command {
        tool: String,
        args: String,
        stderr: String,
    },
    /// The operation doesn't apply to this device (booting a physical phone, booting a running emulator).
    #[error("{0}")]
    Unsupported(String),
    #[error("could not parse simctl's output")]
    Parse(#[from] serde_json::Error),
    #[error("could not run or write {}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// Both platforms' devices. Each platform fails on its own, so a missing Xcode never hides Android, or the reverse.
#[derive(Debug)]
pub struct Listing {
    pub ios: Result<Vec<Device>, Error>,
    pub android: Result<Vec<Device>, Error>,
}

impl Listing {
    /// Every device listed, iOS first, skipping a platform that failed.
    pub fn devices(&self) -> Vec<&Device> {
        [&self.ios, &self.android]
            .into_iter()
            .flatten()
            .flatten()
            .collect()
    }
}

/// Lists devices with the tools found in this process's environment.
pub fn list_devices() -> Listing {
    DeviceHub::discover().list_devices()
}

/// The tool paths. A `None` makes that platform [`Error::NotAvailable`] (`emulator` only adds stopped AVDs).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceHub {
    pub xcrun: Option<PathBuf>,
    pub adb: Option<PathBuf>,
    pub emulator: Option<PathBuf>,
}

impl DeviceHub {
    /// Finds the tools from `PATH`, `ANDROID_HOME`, `ANDROID_SDK_ROOT`, and else the SDK's default install dir
    /// (a Dock-launched app has a minimal `PATH` and no SDK vars).
    pub fn discover() -> Self {
        let user_dir = if cfg!(windows) {
            "LOCALAPPDATA"
        } else {
            "HOME"
        };
        Self::discover_with(
            Some(skyelab_core::env::parent_path().as_os_str()),
            env::var_os("ANDROID_HOME").as_deref(),
            env::var_os("ANDROID_SDK_ROOT").as_deref(),
            env::var_os(user_dir).as_deref(),
        )
    }

    /// The Android SDK dirs are searched before `PATH`, `ANDROID_HOME` first: the SDK the user pointed at wins.
    /// Only when neither var is set (or both are empty), the default SDK dir under `user_dir` is searched instead: `$HOME` on Unix
    /// (`Library/Android/sdk` on macOS, `Android/Sdk` on Linux), `%LOCALAPPDATA%` on Windows (`Android\Sdk`).
    pub fn discover_with(
        path: Option<&OsStr>,
        android_home: Option<&OsStr>,
        android_sdk_root: Option<&OsStr>,
        user_dir: Option<&OsStr>,
    ) -> Self {
        // Relative entries (`.`, an empty ANDROID_HOME) would resolve against the cwd, which may be a project folder.
        let path_dirs: Vec<PathBuf> = path
            .map(|p| env::split_paths(p).filter(|d| d.is_absolute()).collect())
            .unwrap_or_default();
        let android_home = android_home.filter(|v| !v.is_empty());
        let android_sdk_root = android_sdk_root.filter(|v| !v.is_empty());
        let default_sdk = (android_home.is_none() && android_sdk_root.is_none())
            .then(|| user_dir.map(|d| Path::new(d).join(DEFAULT_SDK)))
            .flatten();
        let sdks: Vec<PathBuf> = [android_home, android_sdk_root]
            .into_iter()
            .flatten()
            .map(PathBuf::from)
            .chain(default_sdk)
            .filter(|d| d.is_absolute())
            .collect();
        let find = |sdk_subdir: Option<&str>, name: &str| {
            let file = format!("{name}{EXE_SUFFIX}");
            sdk_subdir
                .into_iter()
                .flat_map(|sub| sdks.iter().map(move |sdk| sdk.join(sub)))
                .chain(path_dirs.iter().cloned())
                .map(|dir| dir.join(&file))
                .find(|p| p.is_file())
        };
        Self {
            xcrun: find(None, "xcrun"),
            adb: find(Some("platform-tools"), "adb"),
            emulator: find(Some("emulator"), "emulator"),
        }
    }

    pub fn list_devices(&self) -> Listing {
        Listing {
            ios: self.list_ios(),
            android: self.list_android(),
        }
    }

    fn list_ios(&self) -> Result<Vec<Device>, Error> {
        parse_simctl_list(&self.simctl(&[
            "list".as_ref(),
            "devices".as_ref(),
            "--json".as_ref(),
        ])?)
    }

    fn list_android(&self) -> Result<Vec<Device>, Error> {
        let adb = self
            .adb
            .as_deref()
            .ok_or(Error::NotAvailable(Platform::Android))?;
        let mut devices = parse_adb_devices(&text(run(adb, &["devices", "-l"])?));
        for d in &mut devices {
            if d.kind == DeviceKind::Emulator
                && let Some(avd) = self.avd_name(&d.id)
            {
                d.name = avd;
            }
            // adb lists an emulator as `device` well before Android finishes booting.
            if d.kind == DeviceKind::Emulator
                && d.state == DeviceState::Booted
                && self
                    .adb(&d.id, &["shell", "getprop", "sys.boot_completed"])
                    .ok()
                    .and_then(|out| first_line(&out))
                    .as_deref()
                    != Some("1")
            {
                d.state = DeviceState::Booting;
            }
            if d.state == DeviceState::Booted {
                d.os_version = self
                    .adb(&d.id, &["shell", "getprop", "ro.build.version.release"])
                    .ok()
                    .and_then(|out| first_line(&out));
            }
        }
        if let Some(emulator) = &self.emulator {
            let running: HashSet<String> = devices
                .iter()
                .filter(|d| d.kind == DeviceKind::Emulator)
                .map(|d| d.name.clone())
                .collect();
            // Stopped AVDs are extra: a broken `emulator` must not hide the devices adb found.
            let avds = run(emulator, &["-list-avds"])
                .map(|out| parse_avd_list(&text(out)))
                .unwrap_or_default();
            devices.extend(
                avds.into_iter()
                    .filter(|a| !running.contains(a))
                    .map(|avd| Device {
                        id: avd.clone(),
                        name: avd,
                        platform: Platform::Android,
                        os_version: None,
                        state: DeviceState::Shutdown,
                        kind: DeviceKind::Emulator,
                    }),
            );
        }
        Ok(devices)
    }

    /// The console needs `~/.emulator_console_auth_token`; without it (another `HOME`) adb prints nothing and exits 0,
    /// so the boot property names the AVD instead.
    fn avd_name(&self, serial: &str) -> Option<String> {
        let console = self.adb(serial, &["emu", "avd", "name"]).ok();
        console.as_deref().and_then(console_reply).or_else(|| {
            let out = self
                .adb(serial, &["shell", "getprop", "ro.boot.qemu.avd_name"])
                .ok()?;
            parse_avd_list(&text(out)).into_iter().next()
        })
    }

    /// Boots a simulator (returns `None`), or starts a stopped AVD and returns its process (`Some`).
    pub fn boot(&self, device: &Device) -> Result<Option<Emulator>, Error> {
        match device.platform {
            Platform::Ios => self.sim(device, "boot", &[]).map(|_| None),
            Platform::Android => {
                if device.kind != DeviceKind::Emulator || device.state != DeviceState::Shutdown {
                    return Err(Error::Unsupported(format!(
                        "{} is not a stopped emulator",
                        device.name
                    )));
                }
                let emulator = self
                    .emulator
                    .as_deref()
                    .ok_or(Error::NotAvailable(Platform::Android))?;
                let child = skyelab_core::process::spawn(
                    tool_command(emulator)
                        .arg("-avd")
                        .arg(not_a_flag(device.id.as_ref())?)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null()),
                )
                .map_err(|source| Error::Io {
                    path: emulator.to_owned(),
                    source,
                })?;
                Ok(Some(Emulator { child: Some(child) }))
            }
        }
    }

    pub fn shutdown(&self, device: &Device) -> Result<(), Error> {
        match device.platform {
            Platform::Ios => self.sim(device, "shutdown", &[]).map(drop),
            Platform::Android
                if device.kind == DeviceKind::Emulator && device.state != DeviceState::Shutdown =>
            {
                self.adb(&device.id, &["emu", "kill"]).map(drop)
            }
            Platform::Android => Err(Error::Unsupported(format!(
                "{} is not a running emulator",
                device.name
            ))),
        }
    }

    /// Installs a `.app` bundle (iOS) or an `.apk` (Android, replacing an installed one).
    pub fn install(&self, device: &Device, app: &Path) -> Result<(), Error> {
        match device.platform {
            Platform::Ios => self.sim(device, "install", &[app.as_os_str()]).map(drop),
            Platform::Android => self
                .adb(
                    &device.id,
                    &[
                        "install".as_ref(),
                        "-r".as_ref(),
                        not_a_flag(app.as_os_str())?,
                    ],
                )
                .map(drop),
        }
    }

    /// `app` is a bundle id on iOS and a `package/activity` component on Android.
    pub fn launch(&self, device: &Device, app: &str) -> Result<(), Error> {
        match device.platform {
            Platform::Ios => self.sim(device, "launch", &[app.as_ref()]).map(drop),
            Platform::Android => self
                .adb_shell(&device.id, &["am", "start", "-n"], app)
                .map(drop),
        }
    }

    pub fn open_url(&self, device: &Device, url: &str) -> Result<(), Error> {
        match device.platform {
            Platform::Ios => self.sim(device, "openurl", &[url.as_ref()]).map(drop),
            Platform::Android => {
                let view = ["am", "start", "-a", "android.intent.action.VIEW", "-d"];
                self.adb_shell(&device.id, &view, url).map(drop)
            }
        }
    }

    /// Saves a PNG of the screen to `png`, replacing any file there: the caller picks a path it owns.
    pub fn screenshot(&self, device: &Device, png: &Path) -> Result<(), Error> {
        match device.platform {
            Platform::Ios => {
                let id = not_a_flag(device.id.as_ref())?;
                let png = not_a_flag(png.as_os_str())?;
                self.simctl(&["io".as_ref(), id, "screenshot".as_ref(), png])
                    .map(drop)
            }
            Platform::Android => {
                let bytes = self.adb(&device.id, &["exec-out", "screencap", "-p"])?;
                // exec-out drops screencap's exit status, so a failure shows up only as non-PNG bytes.
                if !bytes.starts_with(b"\x89PNG") {
                    return Err(Error::Command {
                        tool: "adb".into(),
                        args: format!("-s {} exec-out screencap -p", device.id),
                        stderr: text(bytes).chars().take(200).collect(),
                    });
                }
                std::fs::write(png, bytes).map_err(|source| Error::Io {
                    path: png.to_owned(),
                    source,
                })
            }
        }
    }

    fn xcrun(&self) -> Result<&Path, Error> {
        self.xcrun
            .as_deref()
            .ok_or(Error::NotAvailable(Platform::Ios))
    }

    fn simctl(&self, args: &[&OsStr]) -> Result<Vec<u8>, Error> {
        let mut all: Vec<&OsStr> = vec!["simctl".as_ref()];
        all.extend_from_slice(args);
        run(self.xcrun()?, &all)
    }

    /// `simctl <command> <udid> <extra…>`, where `extra` holds caller values only.
    fn sim(&self, device: &Device, command: &str, extra: &[&OsStr]) -> Result<Vec<u8>, Error> {
        let mut args: Vec<&OsStr> = vec![command.as_ref(), not_a_flag(device.id.as_ref())?];
        for &v in extra {
            args.push(not_a_flag(v)?);
        }
        self.simctl(&args)
    }

    fn adb<S: AsRef<OsStr>>(&self, serial: &str, args: &[S]) -> Result<Vec<u8>, Error> {
        let adb = self
            .adb
            .as_deref()
            .ok_or(Error::NotAvailable(Platform::Android))?;
        let mut all: Vec<&OsStr> = vec!["-s".as_ref(), not_a_flag(serial.as_ref())?];
        all.extend(args.iter().map(AsRef::as_ref));
        run(adb, &all)
    }

    /// `adb shell <command…> <value>`. adb joins the words into a command line for the device's shell, so the
    /// caller's `value` is single-quoted there.
    fn adb_shell(&self, serial: &str, command: &[&str], value: &str) -> Result<Vec<u8>, Error> {
        not_a_flag(value.as_ref())?;
        let quoted = sh_quote(value);
        let mut args = vec!["shell"];
        args.extend_from_slice(command);
        args.push(&quoted);
        self.adb(serial, &args)
    }
}

/// Where Android Studio installs the SDK, under `$HOME` (`%LOCALAPPDATA%` on Windows).
const DEFAULT_SDK: &str = if cfg!(target_os = "macos") {
    "Library/Android/sdk"
} else if cfg!(windows) {
    "Android\\Sdk"
} else {
    "Android/Sdk"
};

/// SDK vars the device tools read, on top of the agent allowlist.
const TOOL_ENV: &[&str] = &[
    "ANDROID_HOME",
    "ANDROID_SDK_ROOT",
    "ANDROID_AVD_HOME",
    "ANDROID_USER_HOME",
    "DEVELOPER_DIR",
];

/// A device tool's command, from a cleared env: the agent allowlist plus [`TOOL_ENV`].
fn tool_command(tool: &Path) -> Command {
    let mut cmd = Command::new(tool);
    cmd.env_clear()
        .envs(skyelab_core::env::inherited_agent_env(false))
        .envs(TOOL_ENV.iter().filter_map(|k| Some((*k, env::var_os(k)?))));
    cmd
}

fn run<S: AsRef<OsStr>>(tool: &Path, args: &[S]) -> Result<Vec<u8>, Error> {
    let out =
        skyelab_core::process::output(tool_command(tool).args(args), None).map_err(|source| {
            Error::Io {
                path: tool.to_owned(),
                source,
            }
        })?;
    if out.status.success() {
        return Ok(out.stdout);
    }
    // `am start` reports its errors on stdout.
    let stderr = match text(out.stderr).trim() {
        "" => text(out.stdout).trim().to_owned(),
        s => s.to_owned(),
    };
    Err(Error::Command {
        tool: tool.display().to_string(),
        args: args
            .iter()
            .map(|a| a.as_ref().to_string_lossy())
            .collect::<Vec<_>>()
            .join(" "),
        stderr,
    })
}

fn text(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

fn first_line(bytes: &[u8]) -> Option<String> {
    let line = String::from_utf8_lossy(bytes)
        .lines()
        .next()?
        .trim()
        .to_owned();
    (!line.is_empty()).then_some(line)
}

/// The emulator console answers `<reply>\r\nOK`, or `KO: <why>` on failure.
fn console_reply(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes);
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    let reply = lines.next()?;
    (!reply.starts_with("KO") && lines.any(|l| l == "OK")).then(|| reply.to_owned())
}

/// Rejects a caller value a tool would read as a flag (`-g` to `adb install` grants every permission).
fn not_a_flag(value: &OsStr) -> Result<&OsStr, Error> {
    match value.as_encoded_bytes().first() {
        Some(b'-') => Err(Error::Unsupported(format!(
            "{:?} starts with '-' and would be read as a flag",
            value
        ))),
        _ => Ok(value),
    }
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}
