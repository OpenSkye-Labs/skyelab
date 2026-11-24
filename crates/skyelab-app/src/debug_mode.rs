//! Debug mode (docs/debugging.md): the switch (`SKYELAB_DEBUG`, Settings → General → Advanced, the overlay action),
//! the debug log file, frame timing, the `SKYELAB_TRACE` trace and the opt-in profilers, and the state dump.
//! Off, nothing here runs per frame: the frame probe and the overlay exist only while it is on.

use gpui::{App, AppContext as _, Styled as _, Window};
use skyelab_core::debug;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

#[cfg(all(feature = "dhat-heap", feature = "hotpath-alloc"))]
compile_error!("dhat-heap and hotpath-alloc each install the global allocator: pick one");

/// A frame slower than this is logged (a visible hitch at 60 Hz is anything past ~17 ms; 50 ms is a stutter).
pub(crate) const SLOW_FRAME: Duration = Duration::from_millis(50);

/// Where debug mode writes: `<local data>/<APP_DIR>/debug` (the log, dumps, profiler reports).
pub(crate) fn debug_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join(skyelab_core::instance::APP_DIR).join("debug"))
}

/// Turns debug mode on for this run: the log starts recording (and appending to `debug/debug.log`) and the
/// render counter rolls once a second.
pub(crate) fn enable() {
    static LOG_FILE: std::sync::Once = std::sync::Once::new();
    // Tests never write into the real data dir.
    LOG_FILE.call_once(|| {
        if let Some(dir) = debug_dir().filter(|_| !cfg!(test))
            && let Err(e) = debug::set_log_file(&dir.join("debug.log"))
        {
            eprintln!("skyelab: debug log file not opened: {e}");
        }
    });
    debug::set_enabled(true);
    crate::debug_renders::start(false);
}

/// Settings → General → Advanced → Developer mode, and where it is saved.
struct DeveloperMode {
    on: bool,
    /// `SKYELAB_DEBUG=1` keeps debug mode on whatever the setting says.
    by_env: bool,
    settings: Option<PathBuf>,
}

impl gpui::Global for DeveloperMode {}

/// Launch: the env switches and the Developer mode saved in `settings` (settings.toml).
pub(crate) fn init(switches: &debug::Switches, settings: Option<PathBuf>, cx: &mut App) {
    let on = settings
        .as_deref()
        .and_then(|p| skyelab_core::AppSettings::load(p).ok())
        .is_some_and(|s| s.developer_mode);
    cx.set_global(DeveloperMode {
        on,
        by_env: switches.debug,
        settings,
    });
    crate::debug_renders::start(switches.renders);
    if switches.debug || on {
        enable();
    }
    if switches.debug {
        OPEN_AT_START.with(|o| o.set(true));
    }
}

pub(crate) fn developer_mode(cx: &App) -> bool {
    cx.try_global::<DeveloperMode>().is_some_and(|d| d.on)
}

/// Turns Developer mode on or off now and saves it. Off keeps debug mode on only for `SKYELAB_DEBUG=1`.
pub(crate) fn set_developer_mode(on: bool, cx: &mut App) -> gpui::Task<anyhow::Result<()>> {
    let (by_env, path) = match cx.try_global::<DeveloperMode>() {
        Some(d) => (d.by_env, d.settings.clone()),
        None => (false, None),
    };
    cx.set_global(DeveloperMode {
        on,
        by_env,
        settings: path.clone(),
    });
    if on {
        enable();
    } else if !by_env {
        debug::set_enabled(false);
    }
    // Two quick clicks save in click order: a save that is no longer the newest skips itself under the lock.
    static NEWEST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    static SAVING: Mutex<()> = Mutex::new(());
    let mine = NEWEST.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    cx.background_spawn(async move {
        let _held = SAVING.lock().unwrap_or_else(PoisonError::into_inner);
        if NEWEST.load(std::sync::atomic::Ordering::SeqCst) != mine {
            return Ok(());
        }
        match path {
            Some(p) => skyelab_core::AppSettings::update(&p, |s| s.developer_mode = on).map(drop),
            None => Ok(()),
        }
    })
}

thread_local! {
    /// `SKYELAB_DEBUG=1`: the first window opens with the overlay.
    static OPEN_AT_START: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Profiler and trace guards; dropping them on quit writes their files (the process exits without unwinding).
    static GUARDS: RefCell<Vec<Box<dyn std::any::Any>>> = RefCell::default();
}

/// True once, for the first window, when `SKYELAB_DEBUG=1` asked for the overlay.
pub(crate) fn take_open_at_start() -> bool {
    OPEN_AT_START.with(|o| o.replace(false))
}

/// Before the app runs: the `SKYELAB_TRACE` trace, and whatever profilers this build has (`hotpath`, `dhat-heap`,
/// `tracy`, `tokio-console` features). Their guards are kept until [`finish_on_quit`] drops them.
pub(crate) fn start_profilers(switches: &debug::Switches) {
    start_trace(switches.trace.as_deref());
    #[cfg(feature = "hotpath")]
    {
        let mut builder =
            hotpath::HotpathGuardBuilder::new("skyelab").percentiles(&[50.0, 95.0, 99.0]);
        if std::env::var_os("HOTPATH_OUTPUT_PATH").is_none()
            && let Some(dir) = debug_dir()
        {
            let _ = std::fs::create_dir_all(&dir);
            builder = builder.output_path(dir.join("hotpath-report.txt"));
        }
        keep(Box::new(builder.build()));
    }
    #[cfg(feature = "dhat-heap")]
    {
        let file =
            debug_dir().map_or_else(|| "dhat-heap.json".into(), |d| d.join("dhat-heap.json"));
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        eprintln!(
            "skyelab: dhat heap profile goes to {} on quit",
            file.display()
        );
        keep(Box::new(dhat::Profiler::builder().file_name(file).build()));
    }
}

fn keep(guard: Box<dyn std::any::Any>) {
    GUARDS.with(|g| g.borrow_mut().push(guard));
}

/// Drops the guards on quit, so the trace is flushed and the profilers write their reports.
pub(crate) fn finish_on_quit(cx: &mut App) {
    cx.on_app_quit(|_| {
        GUARDS.with(|g| g.borrow_mut().clear());
        async {}
    })
    .detach();
}

/// The trace subscriber: a Chrome trace file for `SKYELAB_TRACE`, plus Tracy or tokio-console in builds with those
/// features. With none of them no subscriber is installed, and a span costs one callsite check.
fn start_trace(path: Option<&Path>) {
    use tracing_subscriber::Layer as _;
    use tracing_subscriber::layer::SubscriberExt as _;
    let chrome = path.and_then(|p| match private_file(p) {
        Ok(file) => {
            let (layer, guard) = tracing_chrome::ChromeLayerBuilder::new()
                .writer(file)
                .include_args(true)
                .build();
            keep(Box::new(guard));
            eprintln!(
                "skyelab: tracing to {} (quit with cmd-q / the menu to flush it)",
                p.display()
            );
            // Only SkyeLab's own spans: other crates' fields were never checked for secrets.
            let ours = tracing_subscriber::filter::filter_fn(|m| m.target().starts_with("skyelab"));
            Some(layer.with_filter(ours))
        }
        Err(e) => {
            eprintln!("skyelab: no trace, {} can't be written: {e}", p.display());
            None
        }
    });
    #[cfg(feature = "tracy")]
    let tracy = Some(tracing_tracy::TracyLayer::default());
    #[cfg(not(feature = "tracy"))]
    let tracy: Option<tracing_subscriber::layer::Identity> = None;
    #[cfg(feature = "tokio-console")]
    let console = Some(console_subscriber::spawn());
    #[cfg(not(feature = "tokio-console"))]
    let console: Option<tracing_subscriber::layer::Identity> = None;
    if chrome.is_none() && tracy.is_none() && console.is_none() {
        return;
    }
    let subscriber = tracing_subscriber::registry()
        .with(chrome)
        .with(tracy)
        .with(console);
    if let Err(e) = tracing::subscriber::set_global_default(subscriber) {
        eprintln!("skyelab: tracing not started: {e}");
    }
}

/// A new or emptied file only this user can read (0600 on unix).
fn private_file(path: &Path) -> std::io::Result<std::fs::File> {
    let mut open = std::fs::OpenOptions::new();
    open.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut open, 0o600);
    open.open(path)
}

/// Frames are averaged over this long: an idle window draws about once a second (the overlay's tick), so a
/// one-second window would often read 0.
const FRAME_WINDOW: Duration = Duration::from_secs(5);

/// Frame times of the last [`FRAME_WINDOW`]: each window's root render to its paint.
static FRAMES: Mutex<VecDeque<(Instant, Duration)>> = Mutex::new(VecDeque::new());

/// When `main` started, for the launch milestones below.
static LAUNCHED: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

/// The first thing `main` does: launch milestones are measured from here.
pub(crate) fn mark_launch() {
    LAUNCHED.get_or_init(Instant::now);
}

/// A launch milestone (`first frame`, `threads listed`), once each, to stderr and the debug log while debug mode
/// is on: the cold and warm start numbers.
pub(crate) fn launch_milestone(what: &'static str) {
    static SEEN: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    let Some(at) = LAUNCHED.get().filter(|_| debug::enabled()) else {
        return;
    };
    let mut seen = SEEN.lock().unwrap_or_else(PoisonError::into_inner);
    if seen.contains(&what) {
        return;
    }
    seen.push(what);
    let took = at.elapsed();
    eprintln!(
        "skyelab: {what} {:.1} ms after launch",
        took.as_secs_f64() * 1000.
    );
    debug::record(|| debug::Entry::new(debug::Kind::Frame, what).ms(took));
}

/// One frame drawn in `took`; a slow one goes to stderr and the debug log. Each frame is also a `frame` event
/// (field `ms`) in a `SKYELAB_TRACE` trace, so a run's frame times can be read back from the trace.
pub(crate) fn frame_drawn(took: Duration) {
    launch_milestone("first frame");
    tracing::info!(target: "skyelab::frame", ms = took.as_secs_f64() * 1000., "frame");
    let now = Instant::now();
    let mut frames = FRAMES.lock().unwrap_or_else(PoisonError::into_inner);
    while frames
        .front()
        .is_some_and(|(at, _)| now.duration_since(*at) > FRAME_WINDOW)
    {
        frames.pop_front();
    }
    frames.push_back((now, took));
    drop(frames);
    if took > SLOW_FRAME {
        let ms = took.as_secs_f64() * 1000.;
        eprintln!("skyelab: slow frame {ms:.1} ms (root render to paint)");
        debug::record(|| debug::Entry::new(debug::Kind::Frame, "slow frame").ms(took));
    }
}

/// Frames per second over the last 5 s, the last frame's time and the worst of those 5 s.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct FrameStats {
    pub fps: f64,
    pub last: Duration,
    pub worst: Duration,
}

pub(crate) fn frame_stats() -> FrameStats {
    let now = Instant::now();
    let frames = FRAMES.lock().unwrap_or_else(PoisonError::into_inner);
    let recent = frames
        .iter()
        .filter(|(at, _)| now.duration_since(*at) <= FRAME_WINDOW);
    FrameStats {
        fps: recent.clone().count() as f64 / FRAME_WINDOW.as_secs_f64(),
        last: frames.back().map(|(_, d)| *d).unwrap_or_default(),
        worst: recent.map(|(_, d)| *d).max().unwrap_or_default(),
    }
}

/// An invisible last child of the window root: its paint ends the frame whose root render began at `started`.
pub(crate) fn frame_probe(started: Instant) -> impl gpui::IntoElement {
    gpui::canvas(
        |_, _, _| {},
        move |_, _, _, _| frame_drawn(started.elapsed()),
    )
    .absolute()
    .size_0()
}

/// A child process of this app, for the dump.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct ChildProc {
    pub pid: u32,
    pub name: String,
    pub run_secs: u64,
}

/// This process's direct children right now (agents, shells, git): their names only, never args or env.
pub(crate) fn child_processes() -> Vec<ChildProc> {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
    let me = Pid::from_u32(std::process::id());
    let Ok(mut sys) = System::new() else {
        return Vec::new();
    };
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    let mut out: Vec<ChildProc> = sys
        .processes()
        .values()
        .filter(|p| p.parent() == Some(me))
        .map(|p| ChildProc {
            pid: p.pid().as_u32(),
            name: p.name().to_string_lossy().into_owned(),
            run_secs: p.run_time(),
        })
        .collect();
    out.sort_by_key(|c| c.pid);
    out
}

/// This process's resident memory in MB, rounded, if the OS says.
fn own_memory_mb() -> Option<u64> {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
    let me = Pid::from_u32(std::process::id());
    let mut sys = System::new().ok()?;
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[me]),
        true,
        ProcessRefreshKind::nothing().with_memory(),
    );
    sys.process(me).map(|p| p.memory() / 1_000_000)
}

/// The dump's JSON: what a leak or perf report needs, nothing a user typed.
pub(crate) fn snapshot(
    frames: FrameStats,
    renders: &[(&'static str, u64)],
    live: &[(&'static str, i64)],
    children: &[ChildProc],
    log: &[debug::Entry],
) -> serde_json::Value {
    let ms = |d: Duration| (d.as_secs_f64() * 10_000.).round() / 10.;
    serde_json::json!({
        "skyelab": env!("CARGO_PKG_VERSION"),
        "build": if cfg!(debug_assertions) { "debug" } else { "release" },
        "os": std::env::consts::OS,
        "debug_mode": debug::enabled(),
        "frames": {"fps": frames.fps, "last_ms": ms(frames.last), "worst_ms": ms(frames.worst)},
        "renders_per_second": renders.iter().map(|(k, v)| (k.to_string(), serde_json::json!(v))).collect::<serde_json::Map<_, _>>(),
        "live": live.iter().map(|(k, v)| (k.to_string(), serde_json::json!(v))).collect::<serde_json::Map<_, _>>(),
        "child_processes": children,
        "log": log,
    })
}

/// Writes the snapshot to `debug/state-<unix ms>.json` and returns its path.
pub(crate) fn dump_state() -> anyhow::Result<PathBuf> {
    use anyhow::Context as _;
    let mut value = snapshot(
        frame_stats(),
        &crate::debug_renders::last_second(),
        &debug::live_counts(),
        &child_processes(),
        &debug::recent(200),
    );
    // Resident memory, so a leak hunt sees growth as well as counts (docs/debugging.md).
    value["memory_mb"] = serde_json::json!(own_memory_mb());
    let dir = debug_dir().context("no platform data dir")?;
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let path = dir.join(format!("state-{at}.json"));
    // Private like the log: paths and process names are nobody else's business.
    let mut open = std::fs::OpenOptions::new();
    open.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut open, 0o600);
    std::io::Write::write_all(
        &mut open
            .open(&path)
            .with_context(|| format!("creating {}", path.display()))?,
        &serde_json::to_vec_pretty(&value)?,
    )
    .with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

/// GPUI's own frame-time overlay (painted into the scene, no view redraws) follows ours in builds with the
/// `gpui-profiler` feature; other builds have no such overlay.
pub(crate) fn set_gpui_overlay(on: bool, window: &mut Window) {
    #[cfg(feature = "gpui-profiler")]
    window.set_debug_frame_overlay_mode(match on {
        true => gpui::DebugFrameOverlayMode::Full,
        false => gpui::DebugFrameOverlayMode::Hidden,
    });
    #[cfg(not(feature = "gpui-profiler"))]
    let _ = (on, window);
}

#[cfg(test)]
#[path = "debug_mode_test.rs"]
mod tests;
