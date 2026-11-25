mod debug_mode;
mod debug_renders;
mod environment;
mod keymap;
mod palette;
#[cfg(test)]
#[path = "quit_key_test.rs"]
mod quit_key_test;
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
use skyelab_core::instance::APP_DIR;
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
    let path = dirs::config_dir()?.join(APP_DIR).join("device-key");
    skyelab_client::load_or_create_device_key(&path)
        .map_err(|e| eprintln!("skyelab: no device key in {}: {e}", path.display()))
        .ok()
}

/// The OS Tailscale, keeping this node's TLS certificate in a private folder next to the store.
fn tailnet() -> skyelab_tailnet::TailscaleCli {
    let cli = match std::env::var_os("SKYELAB_TAILSCALE_BIN") {
        // Debug builds only: a fake tailnet for live checks (tests/fixtures/fake-tailscale.sh).
        Some(bin) if cfg!(debug_assertions) => {
            skyelab_tailnet::TailscaleCli::with_binaries(vec![bin.into()])
        }
        _ => skyelab_tailnet::TailscaleCli::default(),
    };
    // Local, not roaming, app data: a private key must not sync to a Windows roaming profile.
    match dirs::data_local_dir() {
        Some(d) => cli.with_cert_dir(d.join(APP_DIR).join("tls")),
        None => cli,
    }
}

/// Where threads and projects persist: the platform data dir.
fn store_path() -> Option<std::path::PathBuf> {
    dirs::data_dir().map(|d| d.join(APP_DIR).join("skyelab.db"))
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
            .join(APP_DIR)
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

actions!(skyelab, [Quit]);

/// The app's own keys, then the user's keybindings file over them (the keymap keeps only the bindings made before it
/// starts, so nothing binds a key after this), then the menus, which show the keys bound by then.
fn init_keys(path: Option<std::path::PathBuf>, cx: &mut App) {
    cx.on_action(|_: &Quit, cx| workspace::quit_guard::request_quit(cx));
    // Off macOS ctrl-q belongs to the shell (XON), and the window's close button quits.
    if cfg!(target_os = "macos") {
        cx.bind_keys([gpui::KeyBinding::new("cmd-q", Quit, None)]);
    }
    keymap::init(path, cx);
    app_menus(cx);
}

/// The macOS app menu (other OSes have no app menu bar): New Window, Check for Updates, Settings, Quit.
fn app_menus(cx: &mut App) {
    use gpui::{Menu, MenuItem};
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
}

/// One SkyeLab per data dir (`skyelab_core::instance`). `None` (logged): the lock file couldn't be opened or locked
/// at all (not held by another copy), and the app opens anyway.
fn claim_instance() -> Option<skyelab_core::instance::Claim> {
    let dir = dirs::data_dir()?.join(APP_DIR);
    skyelab_core::instance::claim(&dir)
        .map_err(|e| eprintln!("skyelab: no single-instance lock: {e:#}"))
        .ok()
}

/// A second launch asked this copy to show itself: the first window comes forward, or one opens when none is left.
fn show_on_second_launch(instance: skyelab_core::instance::Instance, cx: &mut App) {
    let (tx, rx) = flume::unbounded::<()>();
    let spawned = std::thread::Builder::new()
        .name("skyelab-instance".into())
        .spawn(
            move || {
                while instance.wait_for_show().is_ok() && tx.send(()).is_ok() {}
            },
        );
    if let Err(e) = spawned {
        eprintln!("skyelab: a second launch can't reach this one: {e}");
        return;
    }
    cx.spawn(async move |cx| {
        while rx.recv_async().await.is_ok() {
            cx.update(|cx| {
                cx.activate(true);
                match workspace::window_handles(cx).first() {
                    Some(w) => _ = w.update(cx, |_, window, _| window.activate_window()),
                    None => {
                        if let Some(open) = cx.try_global::<FreshWindow>().map(|f| f.0.clone()) {
                            open(cx);
                        }
                    }
                }
            });
        }
    })
    .detach();
}

/// Debug builds keep their own config dir: the first one starts from a copy of the installed app's accounts (not its
/// store or settings), so bound folders keep their accounts.
fn seed_dev_accounts(dev: Option<&std::path::Path>) {
    let Some(dev) = dev.filter(|p| cfg!(debug_assertions) && !p.exists()) else {
        return;
    };
    let Some(installed) = dirs::config_dir().map(|d| d.join("skyelab").join("accounts.toml"))
    else {
        return;
    };
    if !installed.is_file() {
        return;
    }
    let copied = dev
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::copy(&installed, dev));
    if let Err(e) = copied {
        eprintln!(
            "skyelab: couldn't copy {} for the dev build: {e}",
            installed.display()
        );
    }
}

/// `skyelab --mcp-proxy`: claude runs this as SkyeLab's MCP server (see skyelab_agent::mcp).
const MCP_PROXY_FLAG: &str = "--mcp-proxy";

/// `--features dhat-heap`: dhat counts every allocation (docs/debugging.md). Never in a default build.
#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() {
    debug_mode::mark_launch();
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
    let instance = match claim_instance() {
        Some(skyelab_core::instance::Claim::Running) => {
            eprintln!(
                "SkyeLab is already running on this data dir; it was asked to show a window."
            );
            return;
        }
        Some(skyelab_core::instance::Claim::Unreachable(e)) => {
            eprintln!("SkyeLab is already running on this data dir but didn't answer: {e:#}");
            return;
        }
        Some(skyelab_core::instance::Claim::First(i)) => Some(i),
        None => None,
    };
    let path = AccountsConfig::default_path();
    seed_dev_accounts(path.as_deref());
    let (config, error) = load_accounts(path.as_deref());
    let switches = skyelab_core::debug::Switches::from_env();
    debug_mode::start_profilers(&switches);

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
        debug_mode::init(&switches, AppSettings::default_path(), cx);
        debug_mode::finish_on_quit(cx);
        skyelab_terminal::init(cx);
        workspace::init(cx);
        workspace::folder_watch::init(cx);
        init_keys(skyelab_core::keymap::default_path(), cx);
        // The fake https remote's CA (`skyelab-git`'s `fake_https_remote` example), trusted by push and fetch only.
        #[cfg(debug_assertions)]
        if let Some(ca) = std::env::var_os("SKYELAB_GIT_CA_FILE") {
            skyelab_git::trust_test_ca(std::path::Path::new(&ca));
        }
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
        workspace::quit_guard::install(AppSettings::default_path(), cx);
        workspace::onboarding::first_run::install(AppSettings::default_path(), cx);
        ui::motion::follow_reduce_motion(cx);
        ui::themes::load(AppSettings::default_path(), cx);
        workspace::scheduled::install(AppSettings::default_path(), cx);
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
        let fake_dir = std::env::var_os("SKYELAB_UPDATE_FAKE_DIR").map(std::path::PathBuf::from);
        updates::init(
            AppSettings::default_path(),
            updates::fetcher(
                std::env::var("SKYELAB_UPDATE_FAKE").ok(),
                fake_dir.clone(),
                cfg!(debug_assertions),
            ),
            cx,
        );
        updates::set_installer(
            updates::installer(
                std::env::var_os("SKYELAB_UPDATE_FAKE_APP").map(std::path::PathBuf::from),
                cfg!(debug_assertions),
                updates::downloader(fake_dir, cfg!(debug_assertions)),
            ),
            cx,
        );
        updates::set_agent_fetch(
            updates::agent_fetcher(
                std::env::var("SKYELAB_AGENT_LATEST_FAKE").ok(),
                cfg!(debug_assertions),
            ),
            cx,
        );
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
        // A scheduled run that comes due with no window open opens one the same way.
        workspace::scheduled::set_opener(fresh.clone(), cx);
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
        if let Some(instance) = instance {
            show_on_second_launch(instance, cx);
        }
        verify_actions::dispatch_verify_actions(cx);
        cx.activate(true);
    });
}
