use gpui::{App, Bounds, Context, Window, WindowBounds, WindowOptions, div, prelude::*, px, size};
use gpui_component::{ActiveTheme as _, Root};
use skyelab_core::AccountsConfig;

struct Shell {
    config: AccountsConfig,
    error: Option<String>,
}

impl Render for Shell {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .size_full()
            .p_4()
            .flex()
            .flex_col()
            .gap_2()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(div().text_xl().child("SkyeLab"))
            .children(
                self.error
                    .clone()
                    .map(|e| div().text_color(theme.danger).child(e)),
            )
            .children(
                self.config
                    .bindings
                    .iter()
                    .map(|b| div().child(format!("{}  →  {}", b.path.display(), b.account))),
            )
    }
}

fn main() {
    let (config, error) = match AccountsConfig::default_path().map(|p| AccountsConfig::load(&p)) {
        Some(Ok(c)) => (c, None),
        Some(Err(e)) => (AccountsConfig::default(), Some(format!("{e:#}"))),
        None => (
            AccountsConfig::default(),
            Some("no config directory on this OS".into()),
        ),
    };

    gpui_platform::application().run(move |cx: &mut App| {
        gpui_component::init(cx);
        let bounds = Bounds::centered(None, size(px(900.), px(600.)), cx);
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                let shell = cx.new(|_| Shell { config, error });
                cx.new(|cx| Root::new(shell, window, cx))
            },
        );
        if let Err(e) = opened {
            eprintln!("failed to open window: {e:#}");
            cx.quit();
        }
        cx.activate(true);
    });
}
