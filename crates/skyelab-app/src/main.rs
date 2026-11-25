// Release builds on Windows are GUI apps: no console window opens beside SkyeLab. Debug builds keep it for logs.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

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
mod startup;
#[cfg(test)]
mod test_wait;
mod ui;
mod updates;
#[cfg(test)]
mod updates_test;
mod verify_actions;
mod window_layout;
mod workspace;

use gpui::{App, AppContext as _, Bounds, actions, px, size};
use skyelab_core::{AccountsConfig, AppSettings};
#[cfg(test)]
use startup::store_path;
use startup::{
    MCP_PROXY_FLAG, agent_bin, claim_instance, device_key, load_accounts, open_store,
    orchestration, seed_dev_accounts, tailnet, window_options,
};
use std::sync::{Arc, Mutex};

/// Opens a workspace window when none is left (the dock, the menu).
struct FreshWindow(std::rc::Rc<dyn Fn(&mut App)>);

impl gpui::Global for FreshWindow {}

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
    // Before any child starts: a Dock launch has only the system PATH.
    skyelab_core::login_shell::start();
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
        cx.set_global(workspace::CursorBin(agent_bin(
            std::env::var_os("SKYELAB_CURSOR_BIN"),
            cfg!(debug_assertions),
            "cursor-agent",
        )));
        cx.set_global(workspace::AntigravityBin(
            match std::env::var_os("SKYELAB_ANTIGRAVITY_BIN") {
                Some(bin) if cfg!(debug_assertions) => bin.into(),
                _ => skyelab_agent::antigravity::default_bin(),
            },
        ));
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
        let device_key = skyelab_client::LazyDeviceKey::new(device_key);
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
                options.device_key = Some(device_key.clone());
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
