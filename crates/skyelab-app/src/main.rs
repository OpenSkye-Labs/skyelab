mod settings;

use gpui::{App, AppContext as _, Bounds, WindowBounds, WindowOptions, px, size};
use gpui_component::Root;
use skyelab_core::AccountsConfig;

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
        let bounds = Bounds::centered(None, size(px(900.), px(700.)), cx);
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|cx| settings::Settings::new(path, config, error, window, cx));
                let focus = gpui::Focusable::focus_handle(view.read(cx), cx);
                window.focus(&focus, cx);
                cx.new(|cx| Root::new(view, window, cx))
            },
        );
        if let Err(e) = opened {
            eprintln!("failed to open window: {e:#}");
            cx.quit();
        }
        cx.activate(true);
    });
}
