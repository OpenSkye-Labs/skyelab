//! A failed load in a real WKWebView: a refused connection shows the pane's error (wry reports no failures; the
//! navigation delegate proxy in `skyelab-webkit-sys` does), and Retry, once a server answers, loads the page and
//! clears it, which also proves the proxy still forwards wry's own delegate calls. User and private tabs both.
//! macOS only (it needs a window server).

mod support;

#[cfg(not(target_os = "macos"))]
fn main() {
    support::listed();
}

#[cfg(target_os = "macos")]
fn main() {
    if !support::listed() {
        mac::run();
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::io::{Read as _, Write as _};
    use std::time::{Duration, Instant};

    use gpui::{AppContext as _, AsyncApp, Entity, WindowOptions};
    use skyelab_browser::BrowserView;

    /// A port nothing listens on (bound, then let go).
    fn closed_port() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        listener.local_addr().expect("addr").port()
    }

    fn serve(port: u16) {
        let listener =
            std::net::TcpListener::bind(("127.0.0.1", port)).expect("bind the freed port");
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(mut conn) = conn else { continue };
                let mut buf = [0u8; 4096];
                let _ = conn.read(&mut buf);
                let body = "<!doctype html><title>Up</title><p>up</p>";
                let _ = write!(
                    conn,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
    }

    pub fn run() {
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(120));
            eprintln!("wkwebview_load_error: timed out");
            std::process::exit(2);
        });
        gpui_platform::application().run(move |cx| {
            gpui_component::init(cx);
            cx.spawn(async move |cx| {
                let mut code = 0;
                for private in [false, true] {
                    if let Err(e) = check(private, cx).await {
                        eprintln!("wkwebview_load_error (private: {private}): {e:#}");
                        code = 1;
                    }
                }
                std::process::exit(code);
            })
            .detach();
        });
    }

    async fn until(
        cx: &mut AsyncApp,
        what: &str,
        mut done: impl FnMut(&mut AsyncApp) -> bool,
    ) -> anyhow::Result<()> {
        let start = Instant::now();
        while !done(cx) {
            anyhow::ensure!(
                start.elapsed() < Duration::from_secs(45),
                "waited for {what}"
            );
            cx.background_executor()
                .timer(Duration::from_millis(100))
                .await;
        }
        Ok(())
    }

    async fn check(private: bool, cx: &mut AsyncApp) -> anyhow::Result<()> {
        let port = closed_port();
        let url = format!("http://localhost:{port}/");
        let options = WindowOptions {
            show: false,
            focus: false,
            ..WindowOptions::default()
        };
        let mut browser: Option<Entity<BrowserView>> = None;
        let open = url.clone();
        cx.update(|cx| {
            cx.open_window(options, |window, cx| {
                let view = cx.new(|cx| match private {
                    true => BrowserView::new_private(Some(&open), window, cx),
                    false => BrowserView::new(Some(&open), window, cx),
                });
                browser = Some(view.clone());
                view
            })
        })?;
        let browser = browser.expect("the window built its view");

        until(cx, "the refused load's error", |cx| {
            cx.read_entity(&browser, |b, _| b.load_error().is_some())
        })
        .await?;
        let error = cx
            .read_entity(&browser, |b, _| b.load_error().cloned())
            .expect("an error");
        anyhow::ensure!(error.host == format!("localhost:{port}"), "host: {error:?}");
        anyhow::ensure!(
            error.reason == "the connection was refused",
            "reason: {error:?}"
        );
        anyhow::ensure!(
            error.hint.is_some(),
            "a local address gets the dev server hint: {error:?}"
        );
        anyhow::ensure!(error.url.starts_with(&url), "Retry's address: {error:?}");

        serve(port);
        cx.update_entity(&browser, |b, cx| b.retry(cx));
        until(cx, "the page to commit after Retry", |cx| {
            cx.read_entity(&browser, |b, _| {
                b.load_error().is_none() && b.load_count() > 0
            })
        })
        .await
    }
}
