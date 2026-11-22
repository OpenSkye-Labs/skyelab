mod settings;
mod workspace;

use gpui::{
    App, AppContext as _, Bounds, Focusable as _, KeyBinding, WindowBounds, WindowKind,
    WindowOptions, actions, px, size,
};
use gpui_component::Root;
use skyelab_browser::BrowserView;
use skyelab_core::AccountsConfig;

actions!(skyelab, [OpenBrowser]);

// `KeyBinding::new` can't resolve `secondary-`, so pick per OS.
const OPEN_BROWSER_KEY: &str = if cfg!(target_os = "macos") {
    "cmd-alt-b"
} else {
    "ctrl-alt-b"
};

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

/// Debug builds only: `SKYELAB_VERIFY_ACTIONS=skyelab::NewTerminal,..` dispatches those actions on the
/// first frame, because `/loop-verify-ui` can't send input to a topmost window.
fn dispatch_verify_actions(window: &mut gpui::Window) {
    let Some(names) = std::env::var("SKYELAB_VERIFY_ACTIONS")
        .ok()
        .filter(|_| cfg!(debug_assertions))
    else {
        return;
    };
    window.on_next_frame(move |window, cx| {
        for name in names.split(',') {
            match cx.build_action(name, None) {
                Ok(action) => window.dispatch_action(action, cx),
                Err(e) => eprintln!("SKYELAB_VERIFY_ACTIONS: {name}: {e:#}"),
            }
        }
    });
}

/// Demo: a browser window, e.g. `SKYELAB_BROWSER_URL=localhost:5173` for a dev server.
fn open_browser(cx: &mut App) -> anyhow::Result<()> {
    let url = std::env::var("SKYELAB_BROWSER_URL").unwrap_or_else(|_| "https://example.com".into());
    let bounds = Bounds::centered(None, size(px(1000.), px(700.)), cx);
    cx.open_window(window_options(bounds), |window, cx| {
        let view = cx.new(|cx| BrowserView::new(&url, window, cx));
        cx.new(|cx| Root::new(view, window, cx))
    })?;
    Ok(())
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
        cx.bind_keys([KeyBinding::new(OPEN_BROWSER_KEY, OpenBrowser, None)]);
        cx.on_action(|_: &OpenBrowser, cx| {
            if let Err(e) = open_browser(cx) {
                eprintln!("failed to open browser: {e:#}");
            }
        });
        // later: the folder comes from the selected project, not home.
        let folder = dirs::home_dir().unwrap_or_default();
        let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
        let opened = cx.open_window(window_options(bounds), |window, cx| {
            let view =
                cx.new(|cx| workspace::Workspace::new(folder, path, config, error, window, cx));
            let focus = view.read(cx).focus_handle(cx);
            window.focus(&focus, cx);
            dispatch_verify_actions(window);
            cx.new(|cx| Root::new(view, window, cx))
        });
        if let Err(e) = opened {
            eprintln!("failed to open window: {e:#}");
            cx.quit();
        }
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_browser_binding_parses() {
        // Panics on a keystroke string GPUI can't parse.
        KeyBinding::new(OPEN_BROWSER_KEY, OpenBrowser, None);
    }
}
