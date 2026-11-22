//! The device tools seen through a trait, so tests can swap in a fake.

use anyhow::Context as _;
use skyelab_devices::{Device, DeviceHub, Listing};
use std::path::Path;

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
}

pub(in crate::workspace) struct HubSource(pub DeviceHub);

impl DeviceSource for HubSource {
    fn list(&self) -> Listing {
        self.0.list_devices()
    }
    fn boot(&self, device: &Device) -> anyhow::Result<()> {
        // Dropping the AVD's `Emulator` leaves it running; Shutdown stops it through adb, not this handle.
        self.0.boot(device).map(drop).map_err(Into::into)
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
}
