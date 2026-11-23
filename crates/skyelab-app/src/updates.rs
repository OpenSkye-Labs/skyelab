//! The update check (T3's update button, minus installing): on start at most daily, and from Check for Updates,
//! ask GitHub for the latest release and compare it with this build. A newer one shows a small notice in the
//! sidebar with Download (opens the release page) and Skip this version. Settings → About turns it off.
//! Nothing is sent but the GET, nothing installs, and tests inject the fetch.

use gpui::{App, AppContext as _, Global, Task, actions};
use skyelab_client::release::Release;
use skyelab_core::AppSettings;
use skyelab_core::update::{self, LATEST_RELEASE_URL};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

actions!(skyelab, [CheckForUpdates]);

/// How the latest release is fetched: the network in the app, a fake in tests and debug runs.
pub type Fetch = Arc<dyn Fn() -> std::io::Result<Release> + Send + Sync>;

/// How often the app asks whether the daily check is due; the answer is wall-clock time, so a laptop that slept
/// still checks the next day.
const TICK: Duration = Duration::from_secs(60 * 60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateStatus {
    Idle,
    Checking,
    UpToDate,
    Available { tag: String, url: String },
    Failed(String),
}

pub struct Updates {
    pub status: UpdateStatus,
    /// settings.toml: the switch, the last check and the skipped version. `None`: nothing saved, no automatic check.
    path: Option<PathBuf>,
    fetch: Fetch,
    /// This build's version (a test sets its own).
    pub(crate) current: String,
    /// The automatic check's switch, as settings.toml had it at start and Settings → About set it since.
    pub enabled: bool,
    _check: Option<Task<()>>,
    _tick: Option<Task<()>>,
}

impl Global for Updates {}

/// The real fetch, or in debug builds `SKYELAB_UPDATE_FAKE=<tag>`'s release (no network).
pub fn fetcher(fake: Option<String>, debug: bool) -> Fetch {
    match fake.filter(|_| debug) {
        Some(tag) => Arc::new(move || {
            Ok(Release {
                url: format!("{}tag/{tag}", update::RELEASES_PAGE),
                tag: tag.clone(),
            })
        }),
        None => Arc::new(|| skyelab_client::release::latest_release(LATEST_RELEASE_URL)),
    }
}

/// Installs the check and runs the start check if it is due, then asks again every [`TICK`].
pub fn init(path: Option<PathBuf>, fetch: Fetch, cx: &mut App) {
    let enabled = path
        .as_deref()
        .and_then(|p| AppSettings::load(p).ok())
        .is_none_or(|s| s.updates.check);
    cx.set_global(Updates {
        status: UpdateStatus::Idle,
        enabled,
        path,
        fetch,
        current: env!("CARGO_PKG_VERSION").to_owned(),
        _check: None,
        _tick: None,
    });
    // The menu item works with no window open too.
    cx.on_action(|_: &CheckForUpdates, cx| check(true, cx));
    check(false, cx);
    let tick = cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(TICK).await;
            cx.update(|cx| check(false, cx));
        }
    });
    cx.global_mut::<Updates>()._tick = Some(tick);
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Checks now (`manual`: Check for Updates, which ignores the schedule, the switch and a skipped version), or
/// only if the automatic check is on and due.
pub fn check(manual: bool, cx: &mut App) {
    let Some(u) = cx.try_global::<Updates>() else {
        return;
    };
    if u.status == UpdateStatus::Checking {
        return;
    }
    let (path, fetch, current) = (u.path.clone(), u.fetch.clone(), u.current.clone());
    let job = cx.background_spawn(async move {
        let settings = match &path {
            Some(p) => AppSettings::load(p).unwrap_or_default().updates,
            None if manual => Default::default(),
            None => return None, // nowhere to remember the last check: never on its own
        };
        let now = now();
        if !manual && !(settings.check && update::due(settings.last_check, now)) {
            return None;
        }
        let got = fetch();
        // A failed check (offline) is tried again at the next tick, not a day later.
        if let (Some(p), true) = (&path, got.is_ok()) {
            let _ = AppSettings::update(p, |s| s.updates.last_check = Some(now));
        }
        Some(status(got, &current, settings.skipped.as_deref(), manual))
    });
    if manual {
        set_status(UpdateStatus::Checking, cx);
    }
    let task = cx.spawn(async move |cx| {
        let Some(next) = job.await else { return };
        cx.update(|cx| {
            // A failed automatic check leaves a notice already shown in place.
            let shown = matches!(
                cx.global::<Updates>().status,
                UpdateStatus::Available { .. }
            );
            if manual || !(shown && matches!(next, UpdateStatus::Failed(_))) {
                set_status(next, cx);
            }
        });
    });
    cx.global_mut::<Updates>()._check = Some(task);
}

/// What a fetch result means for this build.
fn status(
    got: std::io::Result<Release>,
    current: &str,
    skipped: Option<&str>,
    manual: bool,
) -> UpdateStatus {
    match got {
        Err(e) => UpdateStatus::Failed(e.to_string()),
        Ok(r) if !update::is_newer(&r.tag, current) => UpdateStatus::UpToDate,
        Ok(r) if !update::release_page_ok(&r.url) => {
            UpdateStatus::Failed("the release named an unexpected page".into())
        }
        Ok(r) if !manual && skipped == Some(r.tag.as_str()) => UpdateStatus::Idle,
        Ok(r) => UpdateStatus::Available {
            tag: r.tag,
            url: r.url,
        },
    }
}

fn set_status(status: UpdateStatus, cx: &mut App) {
    cx.global_mut::<Updates>().status = status;
    cx.refresh_windows();
}

/// The newer release, if one is shown.
pub fn available(cx: &App) -> Option<(String, String)> {
    match &cx.try_global::<Updates>()?.status {
        UpdateStatus::Available { tag, url } => Some((tag.clone(), url.clone())),
        _ => None,
    }
}

/// Download: opens the release page in the browser (it was checked to be this repo's).
pub fn download(cx: &mut App) {
    if let Some((_, url)) = available(cx).filter(|(_, url)| update::release_page_ok(url)) {
        cx.open_url(&url);
    }
}

/// Skip this version: hides the notice and remembers the tag, so the automatic check stays quiet about it.
pub fn skip(cx: &mut App) {
    let Some((tag, _)) = available(cx) else {
        return;
    };
    set_status(UpdateStatus::Idle, cx);
    let path = cx.global::<Updates>().path.clone();
    if let Some(path) = path {
        cx.background_spawn(async move {
            if let Err(e) = AppSettings::update(&path, |s| s.updates.skipped = Some(tag)) {
                eprintln!("couldn't save the skipped version: {e:#}");
            }
        })
        .detach(); // one small write
    }
}

/// Settings → About's switch.
pub fn set_enabled(on: bool, cx: &mut App) -> Task<anyhow::Result<()>> {
    let path = cx.try_global::<Updates>().and_then(|u| u.path.clone());
    if cx.has_global::<Updates>() {
        cx.global_mut::<Updates>().enabled = on;
    }
    if !on && available(cx).is_some() {
        set_status(UpdateStatus::Idle, cx);
    }
    cx.background_spawn(async move {
        match path {
            Some(p) => AppSettings::update(&p, |s| s.updates.check = on).map(|_| ()),
            None => Ok(()),
        }
    })
}
