mod environment;
mod keymap;
mod palette;
mod remote;
mod settings;
#[cfg(test)]
mod test_wait;
mod ui;
mod updates;
#[cfg(test)]
mod updates_test;
mod verify_actions;
mod window_layout;
mod workspace;

use anyhow::Context as _;
use gpui::{
    App, AppContext as _, Bounds, WindowBounds, WindowKind, WindowOptions, actions, px, size,
};
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

/// The app's MCP server for agent orchestration: grants in `<data dir>/skyelab/run/<pid>`. `None` (with the reason
/// on stderr) means threads run without them.
fn orchestration(
    cx: &mut App,
    store: workspace::SharedStore,
    config: AccountsConfig,
) -> Option<flume::Receiver<workspace::orchestrate::OrchCall>> {
    let installed = (|| {
        // Local, not roaming: on Windows a roaming profile can live on a file share, and grant files hold tokens.
        let runs = dirs::data_local_dir()
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

/// Opens a workspace window when none is left (the dock, the menu).
struct FreshWindow(std::rc::Rc<dyn Fn(&mut App)>);

impl gpui::Global for FreshWindow {}

/// accounts.toml, or an empty config and the reason (a broken file binds nothing).
fn load_accounts(path: Option<&std::path::Path>) -> (AccountsConfig, Option<String>) {
    match path.map(AccountsConfig::load) {
        Some(Ok(c)) => (c, None),
        Some(Err(e)) => (AccountsConfig::default(), Some(format!("{e:#}"))),
        None => (AccountsConfig::default(), None),
    }
}

/// The macOS app menu (other OSes have no app menu bar): New Window, Check for Updates, Settings, Quit.
fn app_menus(cx: &mut App) {
    use gpui::{Menu, MenuItem};
    actions!(skyelab, [Quit]);
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.set_menus([Menu {
        name: "SkyeLab".into(),
        items: vec![
            MenuItem::action("New Window", workspace::NewWindow),
            MenuItem::action("Check for Updates…", updates::CheckForUpdates),
            MenuItem::action("Settings…", workspace::OpenSettings),
            MenuItem::separator(),
            MenuItem::action("Quit SkyeLab", Quit),
        ],
        disabled: false,
    }]);
    // Off macOS ctrl-q belongs to the shell (XON), and the window's close button quits.
    if cfg!(target_os = "macos") {
        cx.bind_keys([gpui::KeyBinding::new("cmd-q", Quit, None)]);
    }
}

/// `skyelab --mcp-proxy`: claude runs this as SkyeLab's MCP server (see skyelab_agent::mcp).
const MCP_PROXY_FLAG: &str = "--mcp-proxy";

fn main() {
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
    let (config, error) = load_accounts(path.as_deref());

    let app = gpui_platform::application().with_assets(ui::AppAssets);
    // A dock click with no window open opens one (macOS keeps running without windows).
    app.on_reopen(|cx| {
        if workspace::window_handles(cx).is_empty()
            && let Some(open) = cx.try_global::<FreshWindow>().map(|f| f.0.clone())
        {
            open(cx);
        }
    });
    app.run(move |cx: &mut App| {
        gpui_component::init(cx);
        // OS notifications need it (Windows' AppUserModelID); same id as the bundle's.
        cx.set_app_identity("dev.openskye.skyelab", "SkyeLab");
        if let Err(e) = ui::install_theme(cx) {
            eprintln!("SkyeLab theme not loaded, using the default: {e:#}");
        }
        settings::init(cx);
        skyelab_terminal::init(cx);
        workspace::init(cx);
        keymap::init(skyelab_core::keymap::default_path(), cx);
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
        let orchestrate = orchestration(cx, backend.store.clone(), config.clone());
        // The last frame when it still shows on a connected display, else most of the primary display.
        workspace::thread_extras::prefs::install(AppSettings::default_path(), cx);
        let saved = AppSettings::default_path()
            .and_then(|p| AppSettings::load(&p).ok())
            .map(|s| s.saved_windows())
            .unwrap_or_default();
        let displays: Vec<_> = cx.displays().iter().map(|d| d.bounds()).collect();
        let visible = cx.primary_display().map_or_else(
            || Bounds::centered(None, size(px(1200.), px(800.)), cx),
            |d| d.visible_bounds(),
        );
        workspace::set_hub(Some(calls), orchestrate, cx);
        let setup_tailnet = tailnet.clone();
        cx.set_global(workspace::WindowKit {
            options: std::rc::Rc::new(window_options),
            setup: std::rc::Rc::new(move |ws, window, cx| {
                ws.set_remote(remote.clone(), None, window, cx);
                let mut options = environment::client_options(setup_tailnet.clone());
                options.device_key = device_key.clone();
                let tailnet = setup_tailnet.clone();
                ws.set_environments(tailnet, AppSettings::default_path(), options, window, cx);
                // Every window saves its frame and page; all of them open at the next launch.
                ws.restore_layout(AppSettings::default_path(), None, window, cx);
            }),
        });
        updates::init(
            AppSettings::default_path(),
            updates::fetcher(
                std::env::var("SKYELAB_UPDATE_FAKE").ok(),
                cfg!(debug_assertions),
            ),
            cx,
        );
        app_menus(cx);
        // A window opened with none left (the dock, the menu): the same store, the accounts as on disk now.
        let fresh = {
            let (path, backend) = (path.clone(), backend.clone());
            move |cx: &mut App| {
                let (path, backend) = (path.clone(), backend.clone());
                let (config, error) = load_accounts(path.as_deref());
                let home = dirs::home_dir().unwrap_or_default();
                let at = Bounds::centered(None, size(px(1200.), px(800.)), cx);
                let opened = workspace::open_window(
                    at,
                    move |window, cx| {
                        workspace::Workspace::new(home, path, config, error, backend, window, cx)
                    },
                    cx,
                );
                if let Err(e) = opened {
                    eprintln!("failed to open a window: {e:#}");
                }
            }
        };
        let fresh: std::rc::Rc<dyn Fn(&mut App)> = std::rc::Rc::new(fresh);
        let on_menu = fresh.clone();
        cx.on_action(move |_: &workspace::NewWindow, cx| on_menu(cx));
        cx.set_global(FreshWindow(fresh));
        let opened = workspace::restore_windows(
            saved,
            AppSettings::default_path(),
            &displays,
            visible,
            move |n, window, cx| {
                let (folder, path, config) = (folder.clone(), path.clone(), config.clone());
                let (error, backend) = (error.clone(), backend.clone());
                let mut ws =
                    workspace::Workspace::new(folder, path, config, error, backend, window, cx);
                // The store's open error shows once, in the first window.
                ws.store_error = store_error.clone().filter(|_| n == 0).map(Into::into);
                ws
            },
            cx,
        );
        if let Err(e) = opened {
            eprintln!("failed to open window: {e:#}");
            cx.quit();
        }
        verify_actions::dispatch_verify_actions(cx);
        cx.activate(true);
    });
}
