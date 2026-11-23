//! An agent's `eval_js` in a real WKWebView, through wry's own bridge: whatever the script does, the app keeps
//! running and the agent gets a value or an error. wry aborts the process on a result it can't serialize, so a
//! failure here is the test binary dying. macOS only (it needs a window server); elsewhere it does nothing.

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
    use skyelab_browser::BrowserView;

    /// Leaves the wrapper's IIFE and makes `value` the script's own last value, which wry's callback used to serialize.
    fn break_out(value: &str) -> String {
        format!("0); return 0; }} catch (e) {{}} }})(); {value}; var _ = (() => {{ try {{ (0")
    }

    const HOSTILE: &[&str] = &[
        "NaN",
        "Infinity",
        "-Infinity",
        "undefined",
        "(() => 1)",
        "(() => { const o = {}; o.o = o; return o; })()",
        "document",
        "window",
        "({ get x() { throw new Error('getter'); } })",
        "(() => { throw { toString() { throw 1; } }; })()",
        "(() => { throw 42; })()",
        "\"\\ud800\"",
        "\"a\\udc00b\"",
    ];

    /// One page on 127.0.0.1 (agent scripts only run in http and https pages).
    fn serve_page() -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(mut conn) = conn else { continue };
                let mut buf = [0u8; 2048];
                let _ = conn.read(&mut buf);
                let body = "<!doctype html><title>Eval page</title><p>hi</p>";
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
        // The whole run, page load included, takes a few seconds; never hang a test run.
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(90));
            eprintln!("wkwebview_eval: timed out");
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
                        eprintln!("wkwebview_eval: {e:#}");
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
        eval.await
    }

    async fn check(
        browser: &Entity<BrowserView>,
        url: &str,
        cx: &mut AsyncApp,
    ) -> anyhow::Result<()> {
        cx.update_entity(browser, |b, cx| b.navigate(url, cx))?;
        while cx.read_entity(browser, |b, _| b.load_count()) == 0 {
            cx.background_executor()
                .timer(Duration::from_millis(50))
                .await;
        }
        // Started fires before the document parses; wait for the title.
        let mut title = serde_json::Value::Null;
        for _ in 0..100 {
            title = eval(browser, "document.title", cx).await?;
            if title == "Eval page" {
                break;
            }
            cx.background_executor()
                .timer(Duration::from_millis(50))
                .await;
        }
        anyhow::ensure!(title == "Eval page", "document.title gave {title}");
        anyhow::ensure!(eval(browser, "1 + 1", cx).await? == 2, "1 + 1");

        for expression in HOSTILE {
            for script in [expression.to_string(), break_out(expression)] {
                // Ok or Err are both fine: the point is that the process is still here to see it.
                let result = eval(browser, &script, cx).await;
                println!("{script:?} -> {result:?}");
            }
        }
        // Breaking out can also plant a value for the read-back; that is only ever handed back as a string.
        let planted = eval(
            browser,
            &break_out("window.__skyelab_eval = \"\\ud800\""),
            cx,
        )
        .await;
        println!("planted lone surrogate -> {planted:?}");
        anyhow::ensure!(
            planted
                .as_ref()
                .is_err_and(|e| format!("{e:#}").contains("not valid text")),
            "planted: {planted:?}"
        );
        // Index 1 past the end of "\ud800" reads String.prototype, which the agent can fill.
        let prototype = eval(
            browser,
            &break_out(
                "Object.defineProperty(String.prototype, '1', { get() { return '\\udc00'; }, configurable: true }); \
                 window.__skyelab_eval = '\\ud800'",
            ),
            cx,
        )
        .await;
        println!("prototype-paired lone surrogate -> {prototype:?}");
        anyhow::ensure!(prototype.is_err(), "prototype: {prototype:?}");

        // Breaking out leaves the number 0 at the slot, not a string: a refusal, not a value.
        let err = eval(browser, &break_out("NaN"), cx)
            .await
            .expect_err("broke out");
        anyhow::ensure!(
            format!("{err:#}").contains("did not run to its end"),
            "{err:#}"
        );
        // Inside the wrapper, values JSON can't hold come back as JSON's own answer.
        anyhow::ensure!(
            eval(browser, "NaN", cx).await? == serde_json::Value::Null,
            "NaN"
        );
        anyhow::ensure!(
            eval(browser, "({ get x() { throw new Error('getter'); } })", cx)
                .await
                .is_err(),
            "getter"
        );
        // And the page still answers afterwards.
        anyhow::ensure!(
            eval(browser, "document.title", cx).await? == "Eval page",
            "title after"
        );
        println!("wkwebview_eval: ok");
        Ok(())
    }
}
