//! An agent's `screenshot` of a real WKWebView: a page painted one known colour comes back as a PNG of that
//! colour, no wider than the cap. The page is served over http, since the agent path refuses `data:` pages.
//! macOS only (it needs a window server); elsewhere it does nothing.

#[cfg(not(target_os = "macos"))]
fn main() {}

#[cfg(target_os = "macos")]
fn main() {
    mac::run();
}

#[cfg(target_os = "macos")]
mod mac {
    use std::io::{Read as _, Write as _};
    use std::time::Duration;

    use gpui::{AppContext as _, AsyncApp, Entity, WindowOptions};
    use skyelab_browser::{BrowserView, MAX_SHOT_WIDTH, encode_png};

    /// The page's colour: rgb(255, 0, 128).
    const PINK: [u8; 3] = [255, 0, 128];

    fn serve_page() -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(mut conn) = conn else { continue };
                let mut buf = [0u8; 2048];
                let _ = conn.read(&mut buf);
                let body = "<!doctype html><title>Shot page</title>\
                    <style>html,body{margin:0;height:100%;background:rgb(255,0,128)}</style>";
                let _ = write!(
                    conn,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        format!("http://{addr}/")
    }

    pub fn run() {
        let url = serve_page();
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(90));
            eprintln!("wkwebview_screenshot: timed out");
            std::process::exit(2);
        });
        gpui_platform::application().run(move |cx| {
            gpui_component::init(cx);
            let options = WindowOptions {
                show: false,
                focus: false,
                ..WindowOptions::default()
            };
            let mut browser = None;
            cx.open_window(options, |window, cx| {
                let view = cx.new(|cx| BrowserView::new_agent(window, cx));
                browser = Some(view.clone());
                view
            })
            .expect("open a window");
            let browser = browser.expect("the window built its view");
            cx.spawn(async move |cx| {
                let code = match check(&browser, &url, cx).await {
                    Ok(()) => 0,
                    Err(e) => {
                        eprintln!("wkwebview_screenshot: {e:#}");
                        1
                    }
                };
                std::process::exit(code);
            })
            .detach();
        });
    }

    async fn check(
        browser: &Entity<BrowserView>,
        url: &str,
        cx: &mut AsyncApp,
    ) -> anyhow::Result<()> {
        let early = cx.read_entity(browser, |b, cx| b.screenshot(cx)).await;
        anyhow::ensure!(
            early
                .as_ref()
                .is_err_and(|e| format!("{e:#}").contains("not loaded")),
            "a screenshot before any load: {early:?}"
        );
        cx.update_entity(browser, |b, cx| b.navigate(url, cx))?;
        // Started fires before the page paints: retry until the colour shows.
        let mut last = String::new();
        for _ in 0..100 {
            cx.background_executor()
                .timer(Duration::from_millis(100))
                .await;
            let raw = match cx.read_entity(browser, |b, cx| b.screenshot(cx)).await {
                Ok(raw) => raw,
                Err(e) => {
                    last = format!("{e:#}");
                    continue;
                }
            };
            let shot = encode_png(&raw)?;
            anyhow::ensure!(shot.width <= MAX_SHOT_WIDTH, "{} px wide", shot.width);
            let png = image::load_from_memory_with_format(&shot.png, image::ImageFormat::Png)?;
            let pixel = png.to_rgba8().get_pixel(shot.width / 2, shot.height / 2).0;
            let close = pixel[..3]
                .iter()
                .zip(PINK)
                .all(|(got, want)| got.abs_diff(want) <= 8);
            if close {
                println!(
                    "wkwebview_screenshot: ok ({}x{}, {} bytes)",
                    shot.width,
                    shot.height,
                    shot.png.len()
                );
                return Ok(());
            }
            last = format!("centre pixel {pixel:?}");
        }
        anyhow::bail!("never saw the page's colour: {last}")
    }
}
