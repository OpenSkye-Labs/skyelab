//! The device hub core: lists and controls iOS Simulators and Android emulators/devices.
//!
//! No GPUI, no async runtime. Every operation runs `xcrun simctl`, `adb` or `emulator` directly with explicit
//! argv, never through a local shell. Calls block until the tool exits, so a UI runs them off the main thread.

mod parse;

use std::collections::HashSet;
use std::env::{self, consts::EXE_SUFFIX};
use std::ffi::OsStr;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

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
    /// Known but not usable now: an offline or unauthorized adb device, a simulator mid-boot.
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
    /// The platform's toolchain (`xcrun`, or `adb` for Android) isn't installed or isn't on PATH.
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
    /// Finds the tools from `PATH`, `ANDROID_HOME` and `ANDROID_SDK_ROOT`.
    pub fn discover() -> Self {
        Self::discover_with(
            env::var_os("PATH").as_deref(),
            env::var_os("ANDROID_HOME").as_deref(),
            env::var_os("ANDROID_SDK_ROOT").as_deref(),
        )
    }

    /// The Android SDK dirs are searched before `PATH`, `ANDROID_HOME` first: the SDK the user pointed at wins.
    pub fn discover_with(
        path: Option<&OsStr>,
        android_home: Option<&OsStr>,
        android_sdk_root: Option<&OsStr>,
    ) -> Self {
        let path_dirs: Vec<PathBuf> = path
            .map(|p| env::split_paths(p).collect())
            .unwrap_or_default();
        let sdks: Vec<PathBuf> = [android_home, android_sdk_root]
            .into_iter()
            .flatten()
            .map(PathBuf::from)
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
        // Best effort: a device that doesn't answer keeps its adb name and no os_version.
        for d in &mut devices {
            if d.kind == DeviceKind::Emulator
                && let Ok(out) = self.adb(&d.id, &["emu", "avd", "name"])
                && let Some(avd) = first_line(&out)
            {
                d.name = avd;
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
            let avds = parse_avd_list(&text(run(emulator, &["-list-avds"])?));
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
                let child = Command::new(emulator)
                    .args(["-avd", &device.id])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .map_err(|source| Error::Io {
                        path: emulator.to_owned(),
                        source,
                    })?;
                Ok(Some(Emulator { child }))
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
                    &["install".as_ref(), "-r".as_ref(), app.as_os_str()],
                )
                .map(drop),
        }
    }

    /// `app` is a bundle id on iOS and a `package/activity` component on Android.
    pub fn launch(&self, device: &Device, app: &str) -> Result<(), Error> {
        match device.platform {
            Platform::Ios => self.sim(device, "launch", &[app.as_ref()]).map(drop),
            Platform::Android => self
                .adb(&device.id, &["shell", "am", "start", "-n", &sh_quote(app)])
                .map(drop),
        }
    }

    pub fn open_url(&self, device: &Device, url: &str) -> Result<(), Error> {
        match device.platform {
            Platform::Ios => self.sim(device, "openurl", &[url.as_ref()]).map(drop),
            Platform::Android => {
                let args = [
                    "shell",
                    "am",
                    "start",
                    "-a",
                    "android.intent.action.VIEW",
                    "-d",
                    &sh_quote(url),
                ];
                self.adb(&device.id, &args).map(drop)
            }
        }
    }

    /// Saves a PNG of the screen to `png`.
    pub fn screenshot(&self, device: &Device, png: &Path) -> Result<(), Error> {
        match device.platform {
            Platform::Ios => {
                let id: &OsStr = device.id.as_ref();
                let args = [
                    "simctl".as_ref(),
                    "io".as_ref(),
                    id,
                    "screenshot".as_ref(),
                    png.as_os_str(),
                ];
                self.xcrun().and_then(|x| run(x, &args)).map(drop)
            }
            Platform::Android => {
                let bytes = self.adb(&device.id, &["exec-out", "screencap", "-p"])?;
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

    /// `simctl <command> <udid> <extra…>`.
    fn sim(&self, device: &Device, command: &str, extra: &[&OsStr]) -> Result<Vec<u8>, Error> {
        let mut args: Vec<&OsStr> = vec![command.as_ref(), device.id.as_ref()];
        args.extend_from_slice(extra);
        self.simctl(&args)
    }

    /// `adb -s <serial> <args…>`. After `shell`, adb joins the args into a command line for the *device's* shell,
    /// so callers pass any user-supplied value through [`sh_quote`].
    fn adb<S: AsRef<OsStr>>(&self, serial: &str, args: &[S]) -> Result<Vec<u8>, Error> {
        let adb = self
            .adb
            .as_deref()
            .ok_or(Error::NotAvailable(Platform::Android))?;
        let mut all: Vec<&OsStr> = vec!["-s".as_ref(), serial.as_ref()];
        all.extend(args.iter().map(AsRef::as_ref));
        run(adb, &all)
    }
}

/// A running emulator started by [`DeviceHub::boot`]. Dropping it leaves the emulator running.
#[derive(Debug)]
pub struct Emulator {
    child: Child,
}

impl Emulator {
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Asks the emulator to quit (SIGTERM on Unix, so it shuts the AVD down cleanly), waits up to 30 s, then kills it.
    pub fn stop(&mut self) -> io::Result<()> {
        if cfg!(unix) && self.child.try_wait()?.is_none() {
            // std can only SIGKILL; `kill` is a program, not a shell.
            let _ = Command::new("kill")
                .args(["-TERM", &self.child.id().to_string()])
                .stderr(Stdio::null())
                .status();
            let deadline = Instant::now() + Duration::from_secs(30);
            while Instant::now() < deadline {
                if self.child.try_wait()?.is_some() {
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        self.child.kill()?;
        self.child.wait().map(drop)
    }
}

/// Runs `tool args…` and returns its stdout, or [`Error::Command`] when it exits non-zero.
fn run<S: AsRef<OsStr>>(tool: &Path, args: &[S]) -> Result<Vec<u8>, Error> {
    let out = Command::new(tool)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|source| Error::Io {
            path: tool.to_owned(),
            source,
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

/// Quotes one argument for a POSIX shell (the device's, behind `adb shell`).
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}
