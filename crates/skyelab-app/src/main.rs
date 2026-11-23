mod environment;
mod palette;
mod remote;
mod settings;
mod ui;
mod window_layout;
mod workspace;

#[cfg(unix)]
use anyhow::Context as _;
use gpui::{
    App, AppContext as _, Bounds, Focusable as _, WindowBounds, WindowKind, WindowOptions, px, size,
};
use gpui_component::Root;
use skyelab_core::{AccountsConfig, AppSettings};
use skyelab_store::Store;
use std::sync::{Arc, Mutex};

/// Debug builds only: `SKYELAB_VERIFY_TOPMOST=1` keeps windows above others so
/// `/loop-verify-ui` can capture them without moving the user's windows.
fn window_options(bounds: Bounds<gpui::Pixels>) -> WindowOptions {
    let topmost = cfg!(debug_assertions) && std::env::var_os("SKYELAB_VERIFY_TOPMOST").is_some();
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        kind: if topmost {
            WindowKind::PopUp
        } else {
            WindowKind::Normal
        },
        titlebar: Some(ui::titlebar_options()),
        window_min_size: Some(size(px(800.), px(560.))),
        ..Default::default()
    }
}

/// Long enough for each entry's result (a page loading, a tab switch) to be drawn.
const VERIFY_ACTION_GAP: std::time::Duration = std::time::Duration::from_secs(2);

/// Debug builds only: `SKYELAB_VERIFY_ACTIONS=skyelab::NewTerminal,keys:h i cmd-enter,..` runs each
/// entry in turn, [`VERIFY_ACTION_GAP`] apart so async work (store loads, page loads, new tabs) lands in between, because
/// `/loop-verify-ui` can't send input to a topmost window. `keys:` types keystrokes into the focus,
/// `click:x;y` clicks at a window point (entries are comma-separated, hence the `;`), `drag:x;y;x2;y2` drags with the
/// left button, and `scroll:x;y;lines` scrolls the wheel there (positive: up).
fn dispatch_verify_actions(window: &mut gpui::Window, cx: &mut App) {
    let Some(entries) = std::env::var("SKYELAB_VERIFY_ACTIONS")
        .ok()
        .filter(|_| cfg!(debug_assertions))
    else {
        return;
    };
    window
        .spawn(cx, async move |cx| {
            let entries: Vec<String> = entries.split(',').map(str::to_owned).collect();
            for entry in entries {
                cx.background_executor().timer(VERIFY_ACTION_GAP).await;
                let _ = cx.update(|window, cx| {
                    if let Some(keys) = entry.strip_prefix("keys:") {
                        for key in keys.split_whitespace() {
                            match gpui::Keystroke::parse_with_os(key, cx.platform_info().os) {
                                Ok(k) => _ = window.dispatch_keystroke(k, cx),
                                Err(_) => eprintln!("SKYELAB_VERIFY_ACTIONS: bad key {key}"),
                            }
                        }
                        return;
                    }
                    if let Some(at) = entry.strip_prefix("drag:") {
                        let n: Vec<f32> = at
                            .split(';')
                            .filter_map(|v| v.trim().parse().ok())
                            .collect();
                        match n[..] {
                            [x, y, x2, y2] => drag(
                                window,
                                cx,
                                gpui::point(gpui::px(x), gpui::px(y)),
                                gpui::point(gpui::px(x2), gpui::px(y2)),
                            ),
                            _ => eprintln!("SKYELAB_VERIFY_ACTIONS: bad drag {at}"),
                        }
                        return;
                    }
                    if let Some(at) = entry.strip_prefix("scroll:") {
                        let n: Vec<f32> = at
                            .split(';')
                            .filter_map(|v| v.trim().parse().ok())
                            .collect();
                        match n[..] {
                            [x, y, lines] => {
                                _ = window.dispatch_event(
                                    gpui::PlatformInput::Scroll(gpui::ScrollEvent {
                                        position: gpui::point(gpui::px(x), gpui::px(y)),
                                        delta: gpui::ScrollDelta::Lines(gpui::point(0., lines)),
                                        modifiers: Default::default(),
                                        touch_phase: Default::default(),
                                        source: Default::default(),
                                    }),
                                    cx,
                                )
                            }
                            _ => eprintln!("SKYELAB_VERIFY_ACTIONS: bad scroll {at}"),
                        };
                        return;
                    }
                    if let Some(at) = entry.strip_prefix("click:") {
                        match parse_point(at) {
                            Some(position) => click(window, cx, position),
                            None => eprintln!("SKYELAB_VERIFY_ACTIONS: bad point {at}"),
                        }
                        return;
                    }
                    match cx.build_action(&entry, None) {
                        Ok(action) => window.dispatch_action(action, cx),
                        Err(e) => eprintln!("SKYELAB_VERIFY_ACTIONS: {entry}: {e:#}"),
                    }
                });
            }
        })
        .detach();
}

/// `x;y` in window points, as a screenshot of the window shows them at 1x.
fn parse_point(at: &str) -> Option<gpui::Point<gpui::Pixels>> {
    let (x, y) = at.split_once(';')?;
    Some(gpui::point(
        gpui::px(x.trim().parse().ok()?),
        gpui::px(y.trim().parse().ok()?),
    ))
}

/// A left click at `position`: move there, press, release.
fn click(window: &mut gpui::Window, cx: &mut App, position: gpui::Point<gpui::Pixels>) {
    use gpui::{Modifiers, MouseButton, PlatformInput as P};
    let (button, modifiers) = (MouseButton::Left, Modifiers::default());
    window.dispatch_event(
        P::MouseMove(gpui::MouseMoveEvent {
            position,
            pressed_button: None,
            modifiers,
        }),
        cx,
    );
    window.dispatch_event(
        P::MouseDown(gpui::MouseDownEvent {
            button,
            position,
            modifiers,
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
    window.dispatch_event(
        P::MouseUp(gpui::MouseUpEvent {
            button,
            position,
            modifiers,
            click_count: 1,
        }),
        cx,
    );
}

/// A left-button drag from `from` to `to`: press, move with the button held, release.
fn drag(
    window: &mut gpui::Window,
    cx: &mut App,
    from: gpui::Point<gpui::Pixels>,
    to: gpui::Point<gpui::Pixels>,
) {
    use gpui::{Modifiers, MouseButton, PlatformInput as P};
    let (button, modifiers) = (MouseButton::Left, Modifiers::default());
    let events = [
        P::MouseMove(gpui::MouseMoveEvent {
            position: from,
            pressed_button: None,
            modifiers,
        }),
        P::MouseDown(gpui::MouseDownEvent {
            button,
            position: from,
            modifiers,
            click_count: 1,
            first_mouse: false,
        }),
        P::MouseMove(gpui::MouseMoveEvent {
            position: to,
            pressed_button: Some(button),
            modifiers,
        }),
        P::MouseUp(gpui::MouseUpEvent {
            button,
            position: to,
            modifiers,
            click_count: 1,
        }),
    ];
    for event in events {
        window.dispatch_event(event, cx);
    }
}

/// An agent binary: `name`, or in debug builds the env override (`SKYELAB_CLAUDE_BIN`,
/// `SKYELAB_CODEX_BIN`: a fake, for verification).
fn agent_bin(env: Option<std::ffi::OsString>, debug: bool, name: &str) -> std::path::PathBuf {
    match env {
        Some(bin) if debug => bin.into(),
        _ => name.into(),
    }
}

/// This install's device key for connecting to other machines, which approve it. Release builds on macOS keep it
/// in the keychain, where another program (an agent) can't read it without the user's yes. Debug builds and other
/// OSes use a 0600 file in the config dir, so a throwaway `HOME` never touches the login keychain. `None` (logged)
/// means other machines can't approve this one.
fn device_key() -> Option<String> {
    #[cfg(target_os = "macos")]
    if !cfg!(debug_assertions) {
        return skyelab_client::keychain_device_key()
            .map_err(|e| eprintln!("skyelab: no device key from the keychain: {e}"))
            .ok();
    }
    let path = dirs::config_dir()?.join("skyelab").join("device-key");
    skyelab_client::load_or_create_device_key(&path)
        .map_err(|e| eprintln!("skyelab: no device key in {}: {e}", path.display()))
        .ok()
}

/// The OS Tailscale, keeping this node's TLS certificate in a private folder next to the store.
fn tailnet() -> skyelab_tailnet::TailscaleCli {
    let cli = skyelab_tailnet::TailscaleCli::default();
    // Local, not roaming, app data: a private key must not sync to a Windows roaming profile.
    match dirs::data_local_dir() {
        Some(d) => cli.with_cert_dir(d.join("skyelab").join("tls")),
        None => cli,
    }
}

/// Where threads and projects persist: the platform data dir.
fn store_path() -> Option<std::path::PathBuf> {
    dirs::data_dir().map(|d| d.join("skyelab").join("skyelab.db"))
}

/// Opens the store, or an in-memory one (with the reason) so the app still runs.
fn open_store() -> (Store, Option<String>) {
    let opened = store_path()
        .ok_or_else(|| anyhow::anyhow!("no platform data dir"))
        .and_then(|path| {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
                // The store holds prompts and tool output, which can include secrets.
                #[cfg(unix)]
                std::fs::set_permissions(dir, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;
            }
            Store::open(&path)
        });
    match opened {
        Ok(store) => (store, None),
        Err(e) => (
            Store::open_in_memory().expect("an in-memory SQLite database always opens"),
            Some(format!("{e:#}. Nothing is saved this run.")),
        ),
    }
}

/// The app's MCP server for agent orchestration, on unix: `<data dir>/skyelab/run/<pid>`. `None` (with the reason
/// on stderr) means threads run without them.
#[cfg(unix)]
fn orchestration(
    cx: &mut App,
    store: workspace::SharedStore,
    config: AccountsConfig,
) -> Option<flume::Receiver<workspace::orchestrate::OrchCall>> {
    let installed = (|| {
        let runs = dirs::data_dir()
            .context("no platform data dir")?
            .join("skyelab")
            .join("run");
        skyelab_agent::mcp::remove_stale_runs(&runs);
        let run = runs.join(std::process::id().to_string());
        let proxy = skyelab_agent::mcp::Proxy {
            program: std::env::current_exe().context("finding the skyelab binary")?,
            args: vec![MCP_PROXY_FLAG.into()],
        };
        workspace::orchestrate::install(cx, &run, store, config, proxy)
    })();
    installed
        .map_err(|e| eprintln!("skyelab: agent orchestration is off: {e:#}"))
        .ok()
}

/// `skyelab --mcp-proxy`: claude runs this as SkyeLab's MCP server (see skyelab_agent::mcp).
#[cfg(unix)]
const MCP_PROXY_FLAG: &str = "--mcp-proxy";

fn main() {
    #[cfg(unix)]
    if std::env::args_os()
        .nth(1)
        .is_some_and(|a| a == MCP_PROXY_FLAG)
    {
        if let Err(e) = skyelab_agent::mcp::proxy_main() {
            eprintln!("skyelab --mcp-proxy: {e:#}");
            std::process::exit(1);
        }
        return;
    }
    let path = AccountsConfig::default_path();
    let (config, error) = match path.as_deref().map(AccountsConfig::load) {
        Some(Ok(c)) => (c, None),
        Some(Err(e)) => (AccountsConfig::default(), Some(format!("{e:#}"))),
        None => (AccountsConfig::default(), None),
    };

    gpui_platform::application()
        .with_assets(gpui_kit_assets::AllAssets)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);
            // OS notifications need it (Windows' AppUserModelID); same id as the bundle's.
            cx.set_app_identity("dev.openskye.skyelab", "SkyeLab");
            if let Err(e) = ui::install_theme(cx) {
                eprintln!("SkyeLab theme not loaded, using the default: {e:#}");
            }
            settings::init(cx);
            skyelab_terminal::init(cx);
            workspace::init(cx);
            cx.set_global(workspace::GhBin(match std::env::var_os("SKYELAB_GH_BIN") {
                Some(bin) if cfg!(debug_assertions) => bin.into(),
                _ => "gh".into(),
            }));
            cx.set_global(workspace::CodexBin(agent_bin(
                std::env::var_os("SKYELAB_CODEX_BIN"),
                cfg!(debug_assertions),
                "codex",
            )));
            if let Some(path) = &path {
                cx.set_global(workspace::AccountsFile(path.clone()));
            }
            // Home until a project is selected.
            let folder = dirs::home_dir().unwrap_or_default();
            let (mut store, store_error) = open_store();
            // Threads whose delete the last run didn't see out were purged on open: their checkpoints go too.
            let purged: Vec<_> = store
                .take_purged()
                .into_iter()
                .map(|t| {
                    let project = store.project(t.project_id).ok().flatten().map(|p| p.path);
                    (t, project)
                })
                .collect();
            cx.background_spawn(async move {
                for (thread, project) in purged {
                    workspace::thread_checkpoints::forget(&thread, project.as_deref());
                }
            })
            .detach();
            let slot = remote::DaemonSlot::default();
            remote::push_events(&mut store, slot.clone());
            let store = Arc::new(Mutex::new(store));
            let (agents, calls) = remote::UiAgents::new();
            let tailnet: skyelab_daemon::SharedTailnet = Arc::new(tailnet());
            let device_key = device_key();
            let host = remote::Host {
                settings_path: AppSettings::default_path(),
                tailnet: tailnet.clone(),
                config: skyelab_daemon::Config::default(),
                backend: Arc::new(skyelab_daemon::StoreBackend::shared(
                    store.clone(),
                    agents,
                    path.clone(),
                )),
                slot,
            };
            let remote = cx.new(|cx| remote::Remote::new(host, cx));
            let backend = workspace::Backend {
                store,
                claude_bin: agent_bin(
                    std::env::var_os("SKYELAB_CLAUDE_BIN"),
                    cfg!(debug_assertions),
                    "claude",
                ),
            };
            #[cfg(unix)]
            let orchestrate = orchestration(cx, backend.store.clone(), config.clone());
            // The last frame when it still shows on a connected display, else most of the primary display.
            let saved = AppSettings::default_path()
                .and_then(|p| AppSettings::load(&p).ok())
                .and_then(|s| s.window);
            let displays: Vec<_> = cx.displays().iter().map(|d| d.bounds()).collect();
            let visible = cx.primary_display().map_or_else(
                || Bounds::centered(None, size(px(1200.), px(800.)), cx),
                |d| d.visible_bounds(),
            );
            let bounds = window_layout::initial_bounds(saved.as_ref(), &displays, visible);
            let opened = cx.open_window(window_options(bounds), |window, cx| {
                let view = cx.new(|cx| {
                    let mut ws =
                        workspace::Workspace::new(folder, path, config, error, backend, window, cx);
                    ws.store_error = store_error.map(Into::into);
                    ws.set_remote(remote, calls, window, cx);
                    let mut options = environment::client_options(tailnet.clone());
                    options.device_key = device_key.clone();
                    ws.set_environments(tailnet, AppSettings::default_path(), options, window, cx);
                    ws.restore_layout(AppSettings::default_path(), saved.as_ref(), window, cx);
                    #[cfg(unix)]
                    if let Some(o) = orchestrate {
                        ws.set_orchestrator(o, window, cx);
                    }
                    ws
                });
                let focus = view.read(cx).focus_handle(cx);
                window.focus(&focus, cx);
                dispatch_verify_actions(window, cx);
                cx.new(|cx| Root::new(view, window, cx))
            });
            if let Err(e) = opened {
                eprintln!("failed to open window: {e:#}");
                cx.quit();
            }
            cx.activate(true);
        });
}
