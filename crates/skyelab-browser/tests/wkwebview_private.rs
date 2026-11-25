//! A private tab's page in a real WKWebView: navigations to `file:` and to custom app schemes are refused, a
//! download saves nothing, and a script's popup reaches nothing. macOS only (it needs a window server).

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
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use anyhow::Context as _;
    use gpui::{AppContext as _, AsyncApp, Entity, WindowOptions};
    use skyelab_browser::BrowserView;

    /// A local server: `/` is a page, `/download` an attachment; every request path is logged.
    fn serve(hits: Arc<Mutex<Vec<String>>>, file: String) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(mut conn) = conn else { continue };
                let mut buf = [0u8; 4096];
                let n = conn.read(&mut buf).unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = head.split_whitespace().nth(1).unwrap_or("").to_owned();
                hits.lock().expect("hits").push(path.clone());
                let (kind, extra, body) = match path.as_str() {
                    "/download" => (
                        "application/octet-stream",
                        format!("Content-Disposition: attachment; filename=\"{file}\"\r\n"),
                        "SAVED".to_owned(),
                    ),
                    _ => (
                        "text/html",
                        String::new(),
                        "<!doctype html><title>Private page</title><p>hi</p>".to_owned(),
                    ),
                };
                let _ = write!(
                    conn,
                    "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        format!("http://{addr}/")
    }

    pub fn run() {
        let hits = Arc::new(Mutex::new(Vec::new()));
        let file = format!("skyelab-private-probe-{}.bin", std::process::id());
        let url = serve(hits.clone(), file.clone());
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(90));
            eprintln!("wkwebview_private: timed out");
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
                let view = cx.new(|cx| BrowserView::new_private(None, window, cx));
                browser = Some(view.clone());
                view
            })
            .expect("open a window");
            let browser = browser.expect("the window built its view");
            cx.spawn(async move |cx| {
                let code = match check(&browser, &url, &hits, &file, cx).await {
                    Ok(()) => 0,
                    Err(e) => {
                        eprintln!("wkwebview_private: {e:#}");
                        1
                    }
                };
                std::process::exit(code);
            })
            .detach();
        });
    }

    async fn eval(
        browser: &Entity<BrowserView>,
        expression: &str,
        cx: &mut AsyncApp,
    ) -> anyhow::Result<serde_json::Value> {
        let eval = cx.read_entity(browser, |b, cx| b.eval_js(expression, cx));
        eval.await.with_context(|| format!("eval {expression}"))
    }

    async fn wait(cx: &mut AsyncApp, ms: u64) {
        cx.background_executor()
            .timer(Duration::from_millis(ms))
            .await;
    }

    async fn check(
        browser: &Entity<BrowserView>,
        url: &str,
        hits: &Mutex<Vec<String>>,
        file: &str,
        cx: &mut AsyncApp,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            cx.read_entity(browser, |b, _| b.is_private()),
            "built private"
        );
        cx.update_entity(browser, |b, cx| b.navigate(url, cx))?;
        let until = std::time::Instant::now() + Duration::from_secs(60);
        let mut title = serde_json::Value::Null;
        while std::time::Instant::now() < until {
            if cx.read_entity(browser, |b, _| b.load_count()) > 0 {
                title = eval(browser, "document.title", cx).await?;
                if title == "Private page" {
                    break;
                }
            }
            wait(cx, 50).await;
        }
        anyhow::ensure!(title == "Private page", "document.title gave {title}");

        // Each of these must leave the page where it is.
        for target in [
            "file:///etc/hosts".to_owned(),
            "x-skyelab-probe://open".to_owned(),
            format!("{url}download"),
        ] {
            eval(
                browser,
                &format!(
                    "(setTimeout(function () {{ location.href = {}; }}, 0), 1)",
                    serde_json::to_string(&target)?
                ),
                cx,
            )
            .await?;
            wait(cx, 1500).await;
            let here = eval(browser, "location.href", cx)
                .await
                .with_context(|| format!("after going to {target}"))?;
            anyhow::ensure!(here == url, "{target}: the page went to {here}");
        }
        let downloads = std::path::Path::new(&std::env::var("HOME")?).join("Downloads");
        let saved = downloads.join(file);
        if saved.exists() {
            // Only this run's own probe file (its name has our pid); never leave it behind.
            let _ = std::fs::remove_file(&saved);
            anyhow::bail!("the download was saved to {}", downloads.display());
        }

        // A script's popup: WebKit or the handler stops it, and it never loads.
        eval(
            browser,
            &format!(
                "(setTimeout(function () {{ window.open({}); }}, 0), 1)",
                serde_json::to_string(&format!("{url}popup"))?
            ),
            cx,
        )
        .await?;
        wait(cx, 1500).await;
        let seen = hits.lock().expect("hits").clone();
        anyhow::ensure!(
            !seen.iter().any(|p| p == "/popup"),
            "the popup loaded: {seen:?}"
        );
        println!("wkwebview_private: ok ({seen:?})");
        Ok(())
    }
}
