mod settings;
mod workspace;

use gpui::{
    App, AppContext as _, Bounds, Focusable as _, WindowBounds, WindowKind, WindowOptions, px, size,
};
use gpui_component::Root;
use skyelab_core::AccountsConfig;
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
        ..Default::default()
    }
}

/// Debug builds only: `SKYELAB_VERIFY_ACTIONS=skyelab::NewTerminal,keys:h i cmd-enter,..` runs each
/// entry in turn, 400ms apart so async work (store loads, new tabs) lands in between, because
/// `/loop-verify-ui` can't send input to a topmost window. `keys:` types keystrokes into the focus.
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
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(400))
                    .await;
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
                    match cx.build_action(&entry, None) {
                        Ok(action) => window.dispatch_action(action, cx),
                        Err(e) => eprintln!("SKYELAB_VERIFY_ACTIONS: {entry}: {e:#}"),
                    }
                });
            }
        })
        .detach();
}

/// The agent binary: `claude`, or in debug builds `SKYELAB_CLAUDE_BIN` (a fake, for verification).
fn claude_bin(env: Option<std::ffi::OsString>, debug: bool) -> std::path::PathBuf {
    match env {
        Some(bin) if debug => bin.into(),
        _ => "claude".into(),
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

fn main() {
    let path = AccountsConfig::default_path();
    let (config, error) = match path.as_deref().map(AccountsConfig::load) {
        Some(Ok(c)) => (c, None),
        Some(Err(e)) => (AccountsConfig::default(), Some(format!("{e:#}"))),
        None => (AccountsConfig::default(), None),
    };

    gpui_platform::application().run(move |cx: &mut App| {
        gpui_component::init(cx);
        settings::init(cx);
        skyelab_terminal::init(cx);
        workspace::init(cx);
        // Home until a project is selected.
        let folder = dirs::home_dir().unwrap_or_default();
        let (store, store_error) = open_store();
        let backend = workspace::Backend {
            store: Arc::new(Mutex::new(store)),
            claude_bin: claude_bin(
                std::env::var_os("SKYELAB_CLAUDE_BIN"),
                cfg!(debug_assertions),
            ),
        };
        let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
        let opened = cx.open_window(window_options(bounds), |window, cx| {
            let view = cx.new(|cx| {
                let mut ws =
                    workspace::Workspace::new(folder, path, config, error, backend, window, cx);
                ws.store_error = store_error.map(Into::into);
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
