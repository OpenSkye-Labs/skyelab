//! The device tools seen through a trait, so tests can swap in a fake.

use anyhow::Context as _;
use gpui::BackgroundExecutor;
use skyelab_devices::{Device, DeviceHub, DeviceState, Input, Listing};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// What the panel needs from the device tools. Blocking calls: the panel runs them on the background executor.
/// The seam lets tests swap in a fake instead of running `xcrun` and `adb`.
pub(in crate::workspace) trait DeviceSource: Send + Sync + 'static {
    fn list(&self) -> Listing;
    fn boot(&self, device: &Device) -> anyhow::Result<()>;
    fn shutdown(&self, device: &Device) -> anyhow::Result<()>;
    fn install(&self, device: &Device, app: &Path) -> anyhow::Result<()>;
    fn open_url(&self, device: &Device, url: &str) -> anyhow::Result<()>;
    /// PNG bytes of the screen.
    fn screenshot(&self, device: &Device) -> anyhow::Result<Vec<u8>>;
    /// An agent's tap, text or key.
    fn input(&self, device: &Device, input: &Input) -> anyhow::Result<()>;
}

pub(in crate::workspace) struct HubSource(pub DeviceHub);

impl DeviceSource for HubSource {
    fn list(&self) -> Listing {
        self.0.list_devices()
    }
    fn boot(&self, device: &Device) -> anyhow::Result<()> {
        // Dropping the AVD's `Emulator` leaves it running; Shutdown stops it through adb, not this handle.
        drop(self.0.boot(device)?);
        Ok(())
    }
    fn shutdown(&self, device: &Device) -> anyhow::Result<()> {
        Ok(self.0.shutdown(device)?)
    }
    fn install(&self, device: &Device, app: &Path) -> anyhow::Result<()> {
        Ok(self.0.install(device, app)?)
    }
    fn open_url(&self, device: &Device, url: &str) -> anyhow::Result<()> {
        Ok(self.0.open_url(device, url)?)
    }
    fn screenshot(&self, device: &Device) -> anyhow::Result<Vec<u8>> {
        let file = tempfile::Builder::new()
            .prefix("skyelab-screenshot-")
            .suffix(".png")
            .tempfile()
            .context("create a temp file for the screenshot")?;
        self.0.screenshot(device, file.path())?;
        std::fs::read(file.path()).context("read the screenshot")
    }
    fn input(&self, device: &Device, input: &Input) -> anyhow::Result<()> {
        Ok(self.0.input(device, input)?)
    }
}

fn still_stopped(listing: &Listing, avd: &str) -> bool {
    listing
        .devices()
        .iter()
        .any(|d| d.id == avd && d.state == DeviceState::Shutdown)
}

/// How long a booted AVD gets to show up in adb.
pub(super) const BOOT_WAIT: Duration = Duration::from_secs(90);

/// `emulator` returns at once: waits on the executor's timer (no thread held) until adb lists the AVD, so Boot's
/// spinner holds until then instead of looking clickable again.
pub(super) async fn wait_listed(
    source: Arc<dyn DeviceSource>,
    device: Device,
    ex: BackgroundExecutor,
) -> anyhow::Result<()> {
    let deadline = ex.now() + BOOT_WAIT;
    loop {
        let s = source.clone();
        let listing = ex.spawn(async move { s.list() }).await;
        if !still_stopped(&listing, &device.id) {
            return Ok(());
        }
        anyhow::ensure!(
            ex.now() < deadline,
            "{} did not show up in adb within 90 s",
            device.name
        );
        ex.timer(Duration::from_secs(1)).await;
    }
}
