//! The update check (T3's update button): on start at most daily, and from Check for Updates, ask GitHub for the
//! latest release and compare it with this build. A newer one shows a small notice in the sidebar with Download and
//! install (or Download, which opens the release page, when this copy can't update itself) and Skip this version.
//! Download and install fetches and verifies the release's asset ([`skyelab_core::update_install`]); only the
//! user's Restart to update click then swaps it in and quits. Settings → General turns the check off. Nothing is
//! sent but the GETs, and tests inject the fetch and the download.

use gpui::{App, AppContext as _, Global, Task, actions};
use skyelab_client::release::Release;
use skyelab_core::update;
use skyelab_core::update_install::{self as install, Install, Staged};
use skyelab_core::{AppSettings, Provider};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod sources;
pub use sources::{downloader, fetcher};

actions!(skyelab, [CheckForUpdates]);

/// How the latest release is fetched: the network in the app, a fake in tests and debug runs.
pub type Fetch = Arc<dyn Fn() -> std::io::Result<Release> + Send + Sync>;
/// How a release asset is downloaded: streams `url` into the writer, reporting (bytes so far, total if known).
pub type Download = Arc<
    dyn Fn(&str, &mut dyn std::io::Write, &mut dyn FnMut(u64, Option<u64>)) -> std::io::Result<()>
        + Send
        + Sync,
>;
/// How an agent CLI's newest version is fetched (npm's `latest`): the network in the app, a fake in tests and debug runs.
pub type AgentFetch = Arc<dyn Fn(Provider) -> std::io::Result<String> + Send + Sync>;

/// How often the app asks whether the daily check is due; the answer is wall-clock time, so a laptop that slept
/// still checks the next day.
const TICK: Duration = Duration::from_secs(60 * 60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateStatus {
    Idle,
    Checking,
    UpToDate,
    /// GitHub has no release yet (its latest release is a 404).
    NoRelease,
    Available {
        tag: String,
        url: String,
    },
    /// Download and install was clicked for `tag` (`url`: its release page, for the fallback).
    Updating {
        tag: String,
        url: String,
        step: Step,
    },
    Failed(String),
}

/// Where Download and install is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// Bytes so far, and the total once the server said it; all of it in means it's being checked.
    Downloading {
        done: u64,
        total: Option<u64>,
    },
    /// Verified and staged: Restart to update (or Show in folder for a package) is next.
    Ready,
    Failed(String),
}

/// How this copy installs updates, set by the app; without one (a dev build) Download opens the release page.
pub struct Installer {
    pub install: Install,
    pub download: Download,
    /// Where downloads go: a cache folder, or Downloads for a package the user installs.
    pub dir: PathBuf,
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
    /// Welcome's agent CLI check; `None` until the app sets it, so tests never reach the network.
    agent_fetch: Option<AgentFetch>,
    installer: Option<Installer>,
    /// The release the notice shows, for its assets.
    release: Option<Release>,
    /// A verified download waiting for Restart to update.
    staged: Option<Staged>,
    _install: Option<Task<()>>,
    _check: Option<Task<()>>,
    _tick: Option<Task<()>>,
}

impl Global for Updates {}

/// How this copy installs updates: from its exe path and `$APPIMAGE`, or in debug builds the bundle or AppImage at
/// `SKYELAB_UPDATE_FAKE_APP` (`fake_app`). `None`: it can't (a dev build), and Download opens the release page.
pub fn installer(fake_app: Option<PathBuf>, debug: bool, download: Download) -> Option<Installer> {
    let install = match fake_app.filter(|_| debug) {
        Some(p) if p.extension().is_some_and(|e| e == "app") => Install::MacApp(p),
        Some(p) => Install::AppImage(p),
        None => install::detect(
            &std::env::current_exe().ok()?,
            std::env::var_os("APPIMAGE")
                .as_deref()
                .map(std::path::Path::new),
            std::env::consts::OS,
        )?,
    };
    let dir = match install {
        Install::Deb => dirs::download_dir()?,
        _ => dirs::cache_dir()?
            .join(skyelab_core::instance::APP_DIR)
            .join("updates"),
    };
    Some(Installer {
        install,
        download,
        dir,
    })
}

/// Sets how this copy installs updates. A failed install from before this start (Windows notes it in the updates
/// folder) is shown once in the notice.
pub fn set_installer(installer: Option<Installer>, cx: &mut App) {
    if !cx.has_global::<Updates>() {
        return;
    }
    let failed = installer.as_ref().and_then(|i| {
        let marker = i.dir.join(install::FAILURE_MARKER);
        let text = std::fs::read_to_string(&marker).ok()?;
        let _ = std::fs::remove_file(&marker);
        install::parse_failure(&text)
    });
    cx.global_mut::<Updates>().installer = installer;
    if let Some((tag, why)) = failed {
        set_status(
            UpdateStatus::Updating {
                url: format!("{}tag/{tag}", update::RELEASES_PAGE),
                tag,
                step: Step::Failed(why),
            },
            cx,
        );
    }
}

/// The real agent fetch, or in debug builds `SKYELAB_AGENT_LATEST_FAKE=<version>` for both agents (no network).
pub fn agent_fetcher(fake: Option<String>, debug: bool) -> AgentFetch {
    match fake.filter(|_| debug) {
        Some(v) => Arc::new(move |_| Ok(v.clone())),
        None => Arc::new(|p| skyelab_client::release::npm_latest(update::agent_latest_url(p))),
    }
}

/// Sets how Welcome asks for the agent CLIs' newest versions.
pub fn set_agent_fetch(fetch: AgentFetch, cx: &mut App) {
    if cx.has_global::<Updates>() {
        cx.global_mut::<Updates>().agent_fetch = Some(fetch);
    }
}

/// The agent fetch while the update check is on (Settings → About); `None` means no request at all.
pub fn agent_check(cx: &App) -> Option<AgentFetch> {
    cx.try_global::<Updates>()
        .filter(|u| u.enabled)
        .and_then(|u| u.agent_fetch.clone())
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
        agent_fetch: None,
        installer: None,
        release: None,
        staged: None,
        _install: None,
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
    if busy(&u.status) {
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
        let release = got.as_ref().ok().cloned();
        Some((
            status(got, &current, settings.skipped.as_deref(), manual),
            release,
        ))
    });
    if manual {
        set_status(UpdateStatus::Checking, cx);
    }
    let task = cx.spawn(async move |cx| {
        let Some((next, release)) = job.await else {
            return;
        };
        cx.update(|cx| {
            // Download and install was clicked meanwhile.
            if !matches!(cx.global::<Updates>().status, UpdateStatus::Checking)
                && busy(&cx.global::<Updates>().status)
            {
                return;
            }
            // A failed install of this same release (shown from the last start) stays up; Try again needs the release.
            let failed_same = matches!(
                (&cx.global::<Updates>().status, &next),
                (UpdateStatus::Updating { tag, step: Step::Failed(_), .. }, UpdateStatus::Available { tag: t, .. }) if tag == t
            );
            if !manual && failed_same {
                cx.global_mut::<Updates>().release = release;
                return;
            }
            // A failed automatic check leaves a notice already shown in place.
            let shown = matches!(
                cx.global::<Updates>().status,
                UpdateStatus::Available { .. }
            );
            if manual || !(shown && matches!(next, UpdateStatus::Failed(_))) {
                if matches!(next, UpdateStatus::Available { .. }) {
                    cx.global_mut::<Updates>().release = release;
                }
                set_status(next, cx);
            }
        });
    });
    cx.global_mut::<Updates>()._check = Some(task);
}

/// Checking, or downloading or holding an update: a check now would replace it. A failed update isn't busy, so a
/// later check can find a fixed release.
fn busy(status: &UpdateStatus) -> bool {
    match status {
        UpdateStatus::Checking => true,
        UpdateStatus::Updating { step, .. } => !matches!(step, Step::Failed(_)),
        _ => false,
    }
}

/// What a fetch result means for this build.
fn status(
    got: std::io::Result<Release>,
    current: &str,
    skipped: Option<&str>,
    manual: bool,
) -> UpdateStatus {
    match got {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => UpdateStatus::NoRelease,
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

/// The release shown in the notice, available or being installed: its tag and page.
fn shown(cx: &App) -> Option<(String, String)> {
    match &cx.try_global::<Updates>()?.status {
        UpdateStatus::Available { tag, url } | UpdateStatus::Updating { tag, url, .. } => {
            Some((tag.clone(), url.clone()))
        }
        _ => None,
    }
}

/// Download: opens the release page in the browser (it was checked to be this repo's).
pub fn download(cx: &mut App) {
    if let Some((_, url)) = shown(cx).filter(|(_, url)| update::release_page_ok(url)) {
        cx.open_url(&url);
    }
}

/// Whether Download and install is offered: this copy can update itself and the release has its asset.
pub fn can_install(cx: &App) -> bool {
    cx.try_global::<Updates>().is_some_and(|u| {
        let (Some(i), Some(r)) = (&u.installer, &u.release) else {
            return false;
        };
        install::pick(&r.assets, &r.tag, &i.install, std::env::consts::ARCH).is_some()
            && r.assets.iter().any(|a| a.name == update::CHECKSUMS)
    })
}

fn set_step(tag: &str, step: Step, cx: &mut App) {
    if let Some((shown, url)) = shown(cx).filter(|(t, _)| t == tag) {
        set_status(
            UpdateStatus::Updating {
                tag: shown,
                url,
                step,
            },
            cx,
        );
    }
}

/// Download and install (and Try again): downloads this copy's asset, checks it and stages it, with progress in the
/// notice. Nothing running changes; [`restart`] does the swap.
pub fn install(cx: &mut App) {
    let Some((tag, _)) = shown(cx) else { return };
    let u = cx.global::<Updates>();
    if busy(&u.status) {
        return;
    }
    let (Some(i), Some(release)) = (&u.installer, u.release.clone()) else {
        return;
    };
    let (inst, get, dir) = (i.install.clone(), i.download.clone(), i.dir.clone());
    set_step(
        &tag,
        Step::Downloading {
            done: 0,
            total: None,
        },
        cx,
    );
    cx.global_mut::<Updates>().staged = None;
    let (tx, rx) = flume::unbounded();
    let job = cx.background_spawn(async move {
        let mut last = None;
        install::prepare(
            &inst,
            &release.assets,
            &release.tag,
            std::env::consts::ARCH,
            &dir,
            &mut |url, out, p| get(url, out, p),
            // One message per percent (per MB without a total), not per read.
            &mut |done, total| {
                let mark = total.map_or(done >> 20, |t| done * 100 / t.max(1));
                if last != Some(mark) {
                    last = Some(mark);
                    let _ = tx.send((done, total));
                }
            },
        )
    });
    let task = cx.spawn(async move |cx| {
        while let Ok((done, total)) = rx.recv_async().await {
            let tag = tag.clone();
            cx.update(|cx| set_step(&tag, Step::Downloading { done, total }, cx));
        }
        let result = job.await;
        cx.update(|cx| match result {
            Ok(staged) => {
                cx.global_mut::<Updates>().staged = Some(staged);
                set_step(&tag, Step::Ready, cx);
            }
            Err(e) => set_step(&tag, Step::Failed(format!("{e:#}")), cx),
        });
    });
    cx.global_mut::<Updates>()._install = Some(task);
}

/// Whether the staged update is a package shown in its folder (Show in folder, not Restart to update).
pub fn staged_is_folder(cx: &App) -> bool {
    cx.try_global::<Updates>()
        .is_some_and(|u| matches!(u.staged, Some(Staged::Folder(_))))
}

/// Restart to update: swaps the verified download in, starts the relaunch and quits. A failed swap leaves the old
/// app in place and says why. For a package, shows it in its folder instead.
pub fn restart(cx: &mut App) {
    let Some((tag, _)) = shown(cx) else { return };
    if let Some(Staged::Folder(file)) = cx.try_global::<Updates>().and_then(|u| u.staged.as_ref()) {
        cx.reveal_path(file);
        return;
    }
    // Taken, so a second click can't swap twice.
    let Some(staged) = cx.global_mut::<Updates>().staged.take() else {
        return;
    };
    let job = cx.background_spawn(async move {
        install::recheck(&staged)?;
        if let Staged::Swap { new, target, .. } = &staged {
            install::swap(new, target)?;
        }
        install::finish(&staged, std::process::id())
            .map_err(anyhow::Error::from)
            .map_err(|e| e.context("updated, but couldn't restart; quit and reopen SkyeLab"))
    });
    let task = cx.spawn(async move |cx| {
        let result = job.await;
        cx.update(|cx| match result {
            Ok(()) => cx.quit(),
            Err(e) => set_step(&tag, Step::Failed(format!("{e:#}")), cx),
        });
    });
    cx.global_mut::<Updates>()._install = Some(task);
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
