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
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use gpui::{AnyWindowHandle, AppContext as _, AsyncApp, WindowOptions};
    use skyelab_browser::page::{self, Page, PageTheme};

    const PINK: [u8; 3] = [255, 0, 128];

    /// What the local server saw: one entry per connection, the request's path (or `(no request)`).
    type Hits = Arc<Mutex<Vec<String>>>;

    /// A local server that records every connection it gets.
    fn listen(hits: Hits) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(mut conn) = conn else { continue };
                let hits = hits.clone();
                // Its own thread: a preconnect that never sends must not hold up the next connection.
                std::thread::spawn(move || {
                    let _ = conn.set_read_timeout(Some(Duration::from_secs(1)));
                    let mut buf = [0u8; 2048];
                    let n = conn.read(&mut buf).unwrap_or(0);
                    let head = String::from_utf8_lossy(&buf[..n]);
                    let path = head
                        .lines()
                        .next()
                        .and_then(|line| line.split_whitespace().nth(1))
                        .unwrap_or("(no request)")
                        .to_owned();
                    hits.lock().expect("hits lock").push(path);
                });
            }
        });
        format!("127.0.0.1:{}", addr.port())
    }

    pub fn run() {
        let hits = Hits::default();
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
        hits: &Mutex<Vec<String>>,
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
                page::shoot(html, &theme, &[], 400, window, cx)
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
            Page::new(page::prepare(&probe, &theme, &[]), &[], window, cx)
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
        // Late connections (a preconnect's read timeout) land within a second.
        cx.background_executor().timer(Duration::from_secs(1)).await;
        let hits = hits
            .lock()
            .map_err(|_| anyhow::anyhow!("hits lock"))?
            .clone();
        let n = hits.len();
        anyhow::ensure!(
            n == 0,
            "the page reached the local server {n} times: {}",
            hits.join(", ")
        );

        // 3. The allowlist: an https image is refused even from an allowed host, and a script from a host not on the
        // list is refused and reported (only its host). `.example` never resolves, so nothing here reaches a network.
        let hosts = vec!["img.example".to_owned()];
        let html = "<script>document.addEventListener('securitypolicyviolation',function(e){\
            if(e.effectiveDirective.indexOf('img-src')===0)document.title='img refused'})</script>\
            <img src='https://img.example/a.png'><script src='https://cdn.notallowed.example/a.js'></script>\
            <div style='height:100px'></div>";
        let shot = window
            .update(cx, |_, window, cx| {
                page::shoot(html, &theme, &hosts, 400, window, cx)
            })?
            .await?;
        anyhow::ensure!(
            shot.blocked == ["cdn.notallowed.example"],
            "blocked {:?}",
            shot.blocked
        );
        // A page that replaces what the watch could call still reports plain ASCII: a lone surrogate never reaches
        // the engine's bridge (which aborts the app on a string it can't encode).
        let tamper = "<script>Array.prototype.join=function(){return '\\ud800'};\
            String.prototype.charCodeAt=function(){return 0xd800};String.fromCharCode=function(){return '\\ud800'};\
            String.prototype.indexOf=function(){return 0}</script><script src='https://x\\ud800.example/a.js'></script>\
            <div style='height:100px'></div>";
        let shot = window
            .update(cx, |_, window, cx| {
                page::shoot(tamper, &theme, &[], 400, window, cx)
            })?
            .await?;
        anyhow::ensure!(shot.blocked.len() <= 1, "tampered {:?}", shot.blocked);
        let page = window.update(cx, |_, window, cx| {
            Page::new(page::prepare(html, &theme, &hosts), &hosts, window, cx)
        })??;
        cx.background_executor().timer(Duration::from_secs(1)).await;
        let (tx, rx) = futures::channel::oneshot::channel();
        let tx = std::sync::Mutex::new(Some(tx));
        cx.update(|cx| {
            page.webview().read(cx).raw().evaluate_script_with_callback(
                "(function(){try{return document.title}catch(e){return ''}})()",
                move |raw| {
                    if let Some(tx) = tx.lock().ok().and_then(|mut t| t.take()) {
                        let _ = tx.send(raw);
                    }
                },
            )
        })?;
        let title = rx.await?;
        anyhow::ensure!(
            title == "\"img refused\"",
            "image from an allowed host: {title}"
        );

        println!("wkwebview_page: ok");
        Ok(())
    }
}
