//! The Devices tab: lists iOS Simulators and Android emulators/devices and drives them through `skyelab-devices`.

mod render;
mod source;

pub(super) use source::{DeviceSource, HubSource};

use super::{OpenDevices, Workspace};
use gpui::{
    App, BackgroundExecutor, Context, Entity, EventEmitter, FocusHandle, Focusable, Image,
    ImageFormat, KeyBinding, PathPromptOptions, Subscription, Task, Window, actions, prelude::*,
};
use gpui_component::{
    dock::{BasePanel, DockPlacement, Panel, PanelEvent},
    input::{InputEvent, InputState},
};
use skyelab_devices::{Device, DeviceState, Error, Listing, Platform};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

actions!(skyelab, [HideDevices]);

/// The Devices tab's key context: Esc there hides the tab (a text field in it keeps its own Esc).
pub(super) const CONTEXT: &str = "Devices";

pub(super) fn bind_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("escape", HideDevices, Some(CONTEXT))]);
}

/// How often the list refreshes while the tab is on screen.
pub(super) const POLL: Duration = Duration::from_secs(3);
pub(super) const MISSING_XCODE: &str = "Xcode not found. Install Xcode to use iOS Simulators.";
pub(super) const MISSING_ANDROID: &str =
    "Android SDK not found. Install Android Studio or set ANDROID_HOME.";

/// One platform's block in the panel, as the render draws it.
#[derive(Debug, PartialEq)]
pub(super) enum SectionView<'a> {
    Loading,
    /// The platform's tools aren't installed: one friendly row, not an error.
    Missing(&'static str),
    Failed(String),
    Devices(&'a [Device]),
}

pub(super) fn badge(state: DeviceState) -> &'static str {
    match state {
        DeviceState::Booted => "Booted",
        DeviceState::Booting => "Booting…",
        DeviceState::Shutdown => "Shut down",
        DeviceState::Offline => "Offline",
    }
}

/// A device's in-flight operation, last error and last screenshot. Keyed by device id.
#[derive(Default)]
pub(super) struct DeviceOps {
    pub busy: Option<&'static str>,
    pub error: Option<String>,
    pub screenshot: Option<Arc<Image>>,
    _task: Option<Task<()>>,
}

pub struct DevicesPanel {
    focus: FocusHandle,
    source: Arc<dyn DeviceSource>,
    pub(super) listing: Option<Listing>,
    pub(super) ops: HashMap<String, DeviceOps>,
    pub(super) url_input: Entity<InputState>,
    /// The device whose "Open URL" row is showing.
    pub(super) url_for: Option<String>,
    refreshing: Option<Task<()>>,
    /// A refresh was asked for while one ran: its listing may predate an op that just finished, so list again.
    stale: bool,
    pub(super) poll: Option<Task<()>>,
    /// Set by every render, cleared by every poll tick: a tick that finds it clear means the tab is hidden.
    rendered: bool,
    _pick: Option<Task<()>>,
    _url_enter: Subscription,
    _claims: Subscription,
}

impl DevicesPanel {
    pub(super) fn new(
        source: Arc<dyn DeviceSource>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let url_input = cx.new(|cx| InputState::new(window, cx).placeholder("https://example.com"));
        let _url_enter = cx.subscribe(&url_input, |this, _, e: &InputEvent, cx| {
            if matches!(e, InputEvent::PressEnter { .. }) {
                this.submit_url(cx);
            }
        });
        // Agents claim and release devices from the orchestrating window: redraw on each change, not on a timer.
        let _claims = cx.observe_global::<super::device_agent::DeviceClaims>(|_, cx| cx.notify());
        let mut this = Self {
            _claims,
            focus: cx.focus_handle(),
            source,
            listing: None,
            ops: HashMap::new(),
            url_input,
            url_for: None,
            refreshing: None,
            stale: false,
            poll: None,
            rendered: false,
            _pick: None,
            _url_enter,
        };
        this.refresh(cx);
        this
    }

    pub(super) fn section(&self, platform: Platform) -> SectionView<'_> {
        let Some(listing) = &self.listing else {
            return SectionView::Loading;
        };
        let result = match platform {
            Platform::Ios => &listing.ios,
            Platform::Android => &listing.android,
        };
        match result {
            Ok(devices) => SectionView::Devices(devices),
            Err(Error::NotAvailable(Platform::Ios)) => SectionView::Missing(MISSING_XCODE),
            // Command Line Tools without Xcode: xcrun is there, simctl isn't.
            Err(Error::Command { stderr, .. })
                if platform == Platform::Ios && stderr.contains("unable to find utility") =>
            {
                SectionView::Missing(MISSING_XCODE)
            }
            Err(Error::NotAvailable(Platform::Android)) => SectionView::Missing(MISSING_ANDROID),
            Err(e) => SectionView::Failed(e.to_string()),
        }
    }

    pub(super) fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.refreshing.is_some() {
            self.stale = true;
            return;
        }
        let source = self.source.clone();
        let job = cx.background_spawn(async move { source.list() });
        self.refreshing = Some(cx.spawn(async move |this, cx| {
            let listing = job.await;
            // The panel may be gone; nothing to update then.
            let _ = this.update(cx, |this, cx| {
                let first = this.listing.is_none();
                this.prune_ops(&listing, cx);
                this.listing = Some(listing);
                this.refreshing = None;
                if first {
                    this.verify_screenshot(cx);
                }
                if std::mem::take(&mut this.stale) {
                    this.refresh(cx);
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Starts the poll loop if it isn't running. Called from render, so it only runs while the tab is drawn.
    fn ensure_polling(&mut self, cx: &mut Context<Self>) {
        if self.poll.is_some() {
            return;
        }
        if self.listing.is_some() {
            self.refresh(cx);
        }
        self.poll = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                let visible = this.update(cx, |this, cx| {
                    if !std::mem::take(&mut this.rendered) {
                        this.poll = None;
                        return false;
                    }
                    this.refresh(cx);
                    true
                });
                if !matches!(visible, Ok(true)) {
                    break;
                }
            }
        }));
    }

    /// Debug builds only: `SKYELAB_VERIFY_DEVICE_SCREENSHOT=<device name>` screenshots that booted device once
    /// the first list arrives, because `/loop-verify-ui` can't click a topmost window.
    fn verify_screenshot(&mut self, cx: &mut Context<Self>) {
        let Some(name) = std::env::var("SKYELAB_VERIFY_DEVICE_SCREENSHOT")
            .ok()
            .filter(|_| cfg!(debug_assertions))
        else {
            return;
        };
        let device = self.listing.as_ref().and_then(|l| {
            l.devices()
                .into_iter()
                .find(|d| d.name == name && d.state == DeviceState::Booted)
                .cloned()
        });
        match device {
            Some(device) => self.screenshot(&device, cx),
            None => eprintln!("SKYELAB_VERIFY_DEVICE_SCREENSHOT: no booted device named {name}"),
        }
    }

    /// Forgets ops of devices that left the list, and screenshots of devices no longer booted: an emulator
    /// serial is reused by the next AVD, so a kept entry would show on the wrong device.
    fn prune_ops(&mut self, listing: &Listing, cx: &mut App) {
        let devices = listing.devices();
        self.ops.retain(|id, ops| {
            if ops.busy.is_some() {
                return true;
            }
            let device = devices.iter().find(|d| &d.id == id);
            if device.is_none_or(|d| d.state != DeviceState::Booted) {
                uncache(ops.screenshot.take(), cx);
            }
            device.is_some()
        });
    }

    fn device(&self, id: &str) -> Option<Device> {
        let listing = self.listing.as_ref()?;
        listing.devices().into_iter().find(|d| d.id == id).cloned()
    }

    /// Runs `op` on the background executor with a spinner on `device`, then `done` with its result.
    fn run<T: Send + 'static>(
        &mut self,
        device: &Device,
        label: &'static str,
        op: impl FnOnce(&dyn DeviceSource, &Device) -> anyhow::Result<T> + Send + 'static,
        done: impl FnOnce(&mut Self, &str, T, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let job = move |source: Arc<dyn DeviceSource>, target: Device, ex: BackgroundExecutor| {
            ex.spawn(async move { op(&*source, &target) })
        };
        self.run_async(device, label, job, done, cx);
    }

    /// `run` for an op that waits between blocking steps: it gets the executor for those steps and its timers.
    fn run_async<T: 'static, F: Future<Output = anyhow::Result<T>> + 'static>(
        &mut self,
        device: &Device,
        label: &'static str,
        op: impl FnOnce(Arc<dyn DeviceSource>, Device, BackgroundExecutor) -> F,
        done: impl FnOnce(&mut Self, &str, T, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        if self.ops.get(&device.id).is_some_and(|o| o.busy.is_some()) {
            return;
        }
        let id = device.id.clone();
        let job = op(
            self.source.clone(),
            device.clone(),
            cx.background_executor().clone(),
        );
        let task = cx.spawn(async move |this, cx| {
            let result = job.await;
            let _ = this.update(cx, |this, cx| {
                let ops = this.ops.entry(id.clone()).or_default();
                ops.busy = None;
                match result {
                    Ok(value) => done(this, &id, value, cx),
                    Err(e) => ops.error = Some(format!("{e:#}")),
                }
                cx.notify();
            });
        });
        let ops = self.ops.entry(device.id.clone()).or_default();
        (ops.busy, ops.error, ops._task) = (Some(label), None, Some(task));
        cx.notify();
    }

    fn refresh_after(this: &mut Self, _: &str, _: (), cx: &mut Context<Self>) {
        this.refresh(cx);
    }

    pub(super) fn boot(&mut self, device: &Device, cx: &mut Context<Self>) {
        let boot = |source: Arc<dyn DeviceSource>, device: Device, ex: BackgroundExecutor| async move {
            let (s, d) = (source.clone(), device.clone());
            ex.spawn(async move { s.boot(&d) }).await?;
            if device.platform == Platform::Android {
                source::wait_listed(source, device, ex).await?;
            }
            Ok(())
        };
        self.run_async(device, "Booting…", boot, Self::refresh_after, cx);
    }

    pub(super) fn shutdown(&mut self, device: &Device, cx: &mut Context<Self>) {
        self.run(
            device,
            "Shutting down…",
            |s, d| s.shutdown(d),
            Self::refresh_after,
            cx,
        );
    }

    pub(super) fn screenshot(&mut self, device: &Device, cx: &mut Context<Self>) {
        self.run(
            device,
            "Taking screenshot…",
            |s, d| s.screenshot(d),
            |this, id, png, cx| this.show_screenshot(id, png, cx),
            cx,
        );
    }

    /// Shows `png` as the device's latest screenshot.
    pub(super) fn show_screenshot(&mut self, id: &str, png: Vec<u8>, cx: &mut Context<Self>) {
        let image = Arc::new(Image::from_bytes(ImageFormat::Png, png));
        let new_id = image.id();
        let old = self
            .ops
            .entry(id.to_string())
            .or_default()
            .screenshot
            .replace(image);
        // Same bytes, same cache entry: the new image is drawn from it.
        uncache(old.filter(|old| old.id() != new_id), cx);
        cx.notify();
    }

    pub(super) fn install(&mut self, device: &Device, app: PathBuf, cx: &mut Context<Self>) {
        let want = match device.platform {
            Platform::Ios => "app",
            Platform::Android => "apk",
        };
        if app.extension().and_then(|e| e.to_str()) != Some(want) {
            self.ops.entry(device.id.clone()).or_default().error =
                Some(format!("pick a .{want} for {}", device.platform));
            cx.notify();
            return;
        }
        self.run(
            device,
            "Installing…",
            move |s, d| s.install(d, &app),
            |_, _, _, _| {},
            cx,
        );
    }

    fn pick_app(&mut self, device: &Device, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            // A `.app` is a directory.
            directories: device.platform == Platform::Ios,
            multiple: false,
            prompt: Some("Install".into()),
        });
        let device = device.clone();
        self._pick = Some(cx.spawn_in(window, async move |this, cx| {
            let path = match picked.await {
                Ok(Ok(Some(mut paths))) if !paths.is_empty() => Ok(paths.swap_remove(0)),
                Ok(Ok(_)) | Err(_) => return, // cancelled, or the dialog went away
                Ok(Err(e)) => Err(e),
            };
            let _ = this.update(cx, |this, cx| match path {
                Ok(path) => this.install(&device, path, cx),
                Err(e) => {
                    this.ops.entry(device.id.clone()).or_default().error =
                        Some(format!("file picker failed: {e:#}"));
                    cx.notify();
                }
            });
        }));
    }

    pub(super) fn show_url_row(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.url_for = Some(id.to_string());
        self.url_input
            .update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    pub(super) fn submit_url(&mut self, cx: &mut Context<Self>) {
        let url = self.url_input.read(cx).value().trim().to_string();
        let Some(device) = self.url_for.as_deref().and_then(|id| self.device(id)) else {
            return;
        };
        // Keep the row (and the typed URL) until the running op ends; `run` would drop it silently.
        if url.is_empty() || self.ops.get(&device.id).is_some_and(|o| o.busy.is_some()) {
            return;
        }
        self.url_for = None;
        self.run(
            &device,
            "Opening URL…",
            move |s, d| s.open_url(d, &url),
            |_, _, _, _| {},
            cx,
        );
    }
}
/// GPUI caches every image it has drawn until told otherwise.
/// shortcut: frees the decoded image, not its window sprite-atlas texture (that needs the RenderImage, whose loader
/// type gpui-pre 0.3.12 keeps private); upgrade if screenshot memory shows up in profiles.
fn uncache(image: Option<Arc<Image>>, cx: &mut App) {
    if let Some(image) = image {
        image.remove_asset(cx);
    }
}

impl Focusable for DevicesPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl EventEmitter<PanelEvent> for DevicesPanel {}

impl BasePanel for DevicesPanel {
    fn panel_name(&self) -> &'static str {
        "Devices"
    }
}

impl Panel for DevicesPanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        "Devices"
    }
}

impl Workspace {
    /// Toggles Devices, as T3's panel openers do: shown, it hides the right panel; else it opens (or selects) the
    /// tab and focuses it.
    pub(super) fn open_devices(
        &mut self,
        _: &OpenDevices,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.devices_shown(cx) {
            self.hide_devices(&HideDevices, window, cx);
            return;
        }
        let open = self.devices.as_ref().and_then(|p| p.upgrade()).is_some();
        let panel = self.devices_panel(window, cx);
        if open {
            self.show_right_panel(panel.entity_id().into(), window, cx);
        }
        panel.read(cx).focus_handle(cx).focus(window, cx);
    }

    /// Whether the Devices tab is on screen: the status bar's button shows it pressed.
    pub(super) fn devices_shown(&self, cx: &App) -> bool {
        self.devices
            .as_ref()
            .and_then(|p| p.upgrade())
            .is_some_and(|p| self.dock_shows(DockPlacement::Right, p.entity_id().into(), cx))
    }

    /// Hides the right panel while it shows Devices; the tab stays for the next open.
    pub(super) fn hide_devices(
        &mut self,
        _: &HideDevices,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.devices_shown(cx) {
            self.toggle(DockPlacement::Right, window, cx);
            self.focus.focus(window, cx);
        }
    }

    /// The Devices tab, added to the right dock if it isn't open.
    pub(super) fn devices_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<DevicesPanel> {
        if let Some(panel) = self.devices.as_ref().and_then(|p| p.upgrade()) {
            return panel;
        }
        let source = self.device_source();
        let panel = cx.new(|cx| DevicesPanel::new(source, window, cx));
        self.add_right_panel(panel.clone(), window, cx);
        self.devices = Some(panel.downgrade());
        cx.notify();
        panel
    }
}
