//! A visual reply's page in a real WKWebView: `shoot` measures and snapshots a hidden page, and the locked page
//! runs its scripts but reaches nothing: fetch, XHR, WebSocket, an http image, a frame, a popup and a navigation
//! to a local server all fail (the server sees no request). macOS and Windows (WebView2), which need a window
//! server; Linux's WebKitGTK path is cross-checked only.

#[cfg(not(any(target_os = "macos", windows)))]
fn main() {}

#[cfg(any(target_os = "macos", windows))]
fn main() {
    real::run();
}

#[cfg(any(target_os = "macos", windows))]
mod real {
    use std::io::Read as _;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use gpui::{AnyWindowHandle, AppContext as _, AsyncApp, WindowOptions};
    use skyelab_browser::page::{self, Page, PageTheme};

    const PINK: [u8; 3] = [255, 0, 128];

    /// A local server that counts every connection it gets.
    fn listen(hits: Arc<AtomicUsize>) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(mut conn) = conn else { continue };
                hits.fetch_add(1, Ordering::SeqCst);
                let mut buf = [0u8; 2048];
                let _ = conn.read(&mut buf);
            }
        });
        format!("127.0.0.1:{}", addr.port())
    }

    pub fn run() {
        let hits = Arc::new(AtomicUsize::new(0));
        let host = listen(hits.clone());
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(90));
            eprintln!("wkwebview_page: timed out");
            std::process::exit(2);
        });
        gpui_platform::application().run(move |cx| {
            gpui_component::init(cx);
            let options = WindowOptions {
                // WebView2 paints nothing in a hidden window, and a visual reply's window is always shown.
                show: cfg!(windows),
                focus: false,
                ..WindowOptions::default()
            };
            let window = cx
                .open_window(options, |_, cx| cx.new(|_| gpui::Empty))
                .expect("open a window");
            let window: AnyWindowHandle = window.into();
            cx.spawn(async move |cx| {
                let code = match check(window, &host, &hits, cx).await {
                    Ok(()) => 0,
                    Err(e) => {
                        eprintln!("wkwebview_page: {e:#}");
                        1
                    }
                };
                std::process::exit(code);
            })
            .detach();
        });
    }

    async fn check(
        window: AnyWindowHandle,
        host: &str,
        hits: &AtomicUsize,
        cx: &mut AsyncApp,
    ) -> anyhow::Result<()> {
        let theme = PageTheme {
            dark: false,
            vars: vec![],
        };
        // 1. The hidden page measures as its content (300 px plus the 8 px gutters) and snapshots in its colour.
        let html = "<div style='height:300px;background:rgb(255,0,128)'></div>";
        let shot = window
            .update(cx, |_, window, cx| {
                page::shoot(html, &theme, 400, window, cx)
            })?
            .await?;
        anyhow::ensure!(shot.content_height == 316, "height {}", shot.content_height);
        let png = shot.to_png()?;
        let image = image::load_from_memory(&png)?.to_rgba8();
        let pixel = image.get_pixel(image.width() / 2, image.height() / 2).0;
        anyhow::ensure!(
            pixel[..3].iter().zip(PINK).all(|(g, w)| g.abs_diff(w) <= 8),
            "centre pixel {pixel:?}"
        );

        // 2. The locked page: its script runs, and none of its requests leave.
        let probe = format!(
            "<img src='http://{host}/img'><iframe src='http://{host}/frame'></iframe>\
             <link rel='stylesheet' href='http://{host}/css'>\
             <script>\
             window.__start=location.href;\
             try{{fetch('http://{host}/fetch').catch(function(){{}})}}catch(e){{}}\
             try{{var x=new XMLHttpRequest();x.open('GET','http://{host}/xhr');x.send()}}catch(e){{}}\
             try{{new WebSocket('ws://{host}/ws')}}catch(e){{}}\
             try{{window.open('http://{host}/open')}}catch(e){{}}\
             try{{navigator.sendBeacon('http://{host}/beacon')}}catch(e){{}}\
             setTimeout(function(){{location.href='http://{host}/nav'}},50);\
             document.title='ran';\
             </script>"
        );
        let page = window.update(cx, |_, window, cx| {
            Page::new(page::prepare(&probe, &theme), window, cx)
        })??;
        cx.background_executor().timer(Duration::from_secs(2)).await;
        let (tx, rx) = futures::channel::oneshot::channel();
        let tx = std::sync::Mutex::new(Some(tx));
        cx.update(|cx| {
            page.webview().read(cx).raw().evaluate_script_with_callback(
                "(function(){try{return document.title==='ran'&&location.href===window.__start}catch(e){return false}})()",
                move |raw| {
                    if let Some(tx) = tx.lock().ok().and_then(|mut t| t.take()) {
                        let _ = tx.send(raw);
                    }
                },
            )
        })?;
        let ran = rx.await?;
        anyhow::ensure!(
            ran == "true",
            "the script ran and the page stayed put: {ran}"
        );
        let n = hits.load(Ordering::SeqCst);
        anyhow::ensure!(n == 0, "the page reached the local server {n} times");
        println!("wkwebview_page: ok");
        Ok(())
    }
}
