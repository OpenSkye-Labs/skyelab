mod settings;
mod workspace;

use gpui::{
    App, AppContext as _, Bounds, Focusable as _, WindowBounds, WindowKind, WindowOptions, px, size,
};
use gpui_component::Root;
use skyelab_core::AccountsConfig;

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

/// Debug builds only: `SKYELAB_VERIFY_ACTIONS=skyelab::NewTerminal,..` dispatches those actions from the
/// first frame on, two seconds apart, because `/loop-verify-ui` can't send input to a topmost window.
fn dispatch_verify_actions(window: &mut gpui::Window) {
    let Some(names) = std::env::var("SKYELAB_VERIFY_ACTIONS")
        .ok()
        .filter(|_| cfg!(debug_assertions))
    else {
        return;
    };
    window.on_next_frame(move |window, cx| {
        // Two seconds apart, so each action's result (a page loading, a tab switch) gets drawn first.
        window
            .spawn(cx, async move |cx| {
                for name in names.split(',') {
                    let _ = cx.update(|window, cx| match cx.build_action(name, None) {
                        Ok(action) => window.dispatch_action(action, cx),
                        Err(e) => eprintln!("SKYELAB_VERIFY_ACTIONS: {name}: {e:#}"),
                    });
                    cx.background_executor()
                        .timer(std::time::Duration::from_secs(2))
                        .await;
                }
            })
            .detach();
    });
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
