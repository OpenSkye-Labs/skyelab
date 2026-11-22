mod settings;

use gpui::{
    App, AppContext as _, Bounds, Focusable as _, KeyBinding, WindowBounds, WindowKind,
    WindowOptions, actions, px, size,
};
use gpui_component::Root;
use skyelab_browser::BrowserView;
use skyelab_core::AccountsConfig;
use skyelab_terminal::{SpawnOptions, Terminal, TerminalView};

actions!(skyelab, [OpenTerminal, OpenBrowser]);

// `KeyBinding::new` can't resolve `secondary-`, so pick per OS.
const OPEN_TERMINAL_KEY: &str = if cfg!(target_os = "macos") {
    "cmd-alt-t"
} else {
    "ctrl-alt-t"
};

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

/// Demo: a terminal window running the login shell in the home dir.
fn open_terminal(cx: &mut App) -> anyhow::Result<()> {
    let cwd = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("no home directory"))?;
    let (terminal, events) = Terminal::spawn(SpawnOptions {
        cwd,
        command: None,
        env: Default::default(),
        cols: 80,
        lines: 24,
    })?;
    let bounds = Bounds::centered(None, size(px(800.), px(500.)), cx);
    cx.open_window(window_options(bounds), |window, cx| {
        let view = cx.new(|cx| TerminalView::new(terminal, events, cx));
        let focus = view.read(cx).focus_handle(cx);
        focus.focus(window, cx);
        cx.new(|cx| Root::new(view, window, cx))
    })?;
    Ok(())
}

/// Demo: a browser window. `SKYELAB_BROWSER_URL` overrides the start page.
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
        cx.bind_keys([
            KeyBinding::new(OPEN_TERMINAL_KEY, OpenTerminal, None),
            KeyBinding::new(OPEN_BROWSER_KEY, OpenBrowser, None),
        ]);
        cx.on_action(|_: &OpenBrowser, cx| {
            if let Err(e) = open_browser(cx) {
                eprintln!("failed to open browser: {e:#}");
            }
        });
        cx.on_action(|_: &OpenTerminal, cx| {
            if let Err(e) = open_terminal(cx) {
                eprintln!("failed to open terminal: {e:#}");
            }
        });
        let bounds = Bounds::centered(None, size(px(900.), px(700.)), cx);
        let opened = cx.open_window(window_options(bounds), |window, cx| {
            let view = cx.new(|cx| settings::Settings::new(path, config, error, window, cx));
            let focus = view.read(cx).focus_handle(cx);
            window.focus(&focus, cx);
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
    fn open_terminal_binding_parses() {
        // Panics on a keystroke string GPUI can't parse.
        KeyBinding::new(OPEN_TERMINAL_KEY, OpenTerminal, None);
    }

    #[test]
    fn open_browser_binding_parses() {
        KeyBinding::new(OPEN_BROWSER_KEY, OpenBrowser, None);
    }
}
