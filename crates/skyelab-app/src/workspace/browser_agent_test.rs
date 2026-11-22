//! The browser tools against a fake page engine, through skyelab-browser's real agent path (URL check, page
//! guard, decode), and once end to end: a fake claude calls them through the real proxy and server.

use super::browser_agent::{BROWSER_TIMEOUT, MAX_EVAL_OUTPUT};
use super::delegation_test::{open, rig, send, start, thread_id, wait_result, wait_until};
use super::panels::{BrowserPanel, CloseBrowser};
use super::threads_test::test_backend;
use super::*;
use gpui::{TestAppContext, VisualTestContext};
use serde_json::{Value, json};
use skyelab_agent::mcp::BrowserTool;
use skyelab_browser::{Engine, EvalDone};
use std::cell::RefCell;
use std::io::{Read as _, Write as _};
use std::rc::Rc;
use std::time::Duration;

/// A page that answers a few expressions the way a browser would, and runs the page guard itself: on a
/// non-http(s) page the script "throws", which wry reports as an empty result.
struct FakePage {
    url: RefCell<String>,
    title: RefCell<String>,
    loads: flume::Sender<String>,
    scripts: RefCell<Vec<String>>,
    /// Scripts that never finish (the `hang` expression).
    hung: RefCell<Vec<EvalDone>>,
}

impl Engine for FakePage {
    fn load(&self, url: &str, _: &gpui::App) -> anyhow::Result<()> {
        *self.title.borrow_mut() = fetch_title(url).unwrap_or_else(|| "Fake page".into());
        *self.url.borrow_mut() = url.to_owned();
        self.loads.send(url.to_owned())?;
        Ok(())
    }

    fn url(&self, _: &gpui::App) -> anyhow::Result<String> {
        Ok(self.url.borrow().clone())
    }

    fn eval(&self, js: &str, done: EvalDone, _: &gpui::App) -> anyhow::Result<()> {
        self.scripts.borrow_mut().push(js.to_owned());
        let url = self.url.borrow().clone();
        if !(url.starts_with("http:") || url.starts_with("https:")) {
            done(String::new());
            return Ok(());
        }
        let value = match expression(js) {
            "document.title" => json!(self.title.borrow().clone()),
            "big" => json!("x".repeat(MAX_EVAL_OUTPUT + 1)),
            "hang" => {
                self.hung.borrow_mut().push(done);
                return Ok(());
            }
            other => json!(format!("evaluated {other}")),
        };
        let envelope = json!({ "ok": value }).to_string();
        done(serde_json::to_string(&envelope).expect("encode"));
        Ok(())
    }
}

/// The expression inside skyelab-browser's wrapper, which puts it on its own lines.
fn expression(js: &str) -> &str {
    js.split('\n').nth(2).unwrap_or_default()
}

/// The `<title>` of a page on a local test server.
fn fetch_title(url: &str) -> Option<String> {
    let addr = url.strip_prefix("http://127.0.0.1:")?.split('/').next()?;
    let mut conn = std::net::TcpStream::connect(format!("127.0.0.1:{addr}")).ok()?;
    write!(conn, "GET / HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n").ok()?;
    let mut body = String::new();
    conn.read_to_string(&mut body).ok()?;
    let start = body.find("<title>")? + "<title>".len();
    let end = body[start..].find("</title>")? + start;
    Some(body[start..end].to_owned())
}

/// Serves one test page on 127.0.0.1 until the test ends; returns its URL.
fn serve_page(title: &str) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let page = format!("<!doctype html><title>{title}</title><h1>{title}</h1>");
    std::thread::spawn(move || {
        for mut conn in listener.incoming().flatten() {
            let mut buf = [0u8; 1024];
            let _ = conn.read(&mut buf);
            let _ = write!(
                conn,
                "HTTP/1.0 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{page}",
                page.len()
            );
        }
    });
    format!("http://127.0.0.1:{port}/")
}

type Pages = Rc<RefCell<Vec<Rc<FakePage>>>>;

/// Agent tabs in `ws` get fake pages, collected in the returned list.
fn fake_pages(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> Pages {
    let pages: Pages = Rc::default();
    let made = pages.clone();
    ws.update(cx, |ws, _| {
        ws.browser_engine = Some(Rc::new(move || {
            let (loads, rx) = flume::unbounded();
            let page = Rc::new(FakePage {
                url: RefCell::default(),
                title: RefCell::default(),
                loads,
                scripts: RefCell::default(),
                hung: RefCell::default(),
            });
            made.borrow_mut().push(page.clone());
            (page as Rc<dyn Engine>, rx)
        }))
    });
    pages
}

fn workspace(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::settings::init(cx);
        skyelab_terminal::init(cx);
        init(cx);
    });
    let folder = std::env::temp_dir();
    let (ws, cx) = cx.add_window_view(|window, cx| {
        let ws = Workspace::new(
            folder,
            None,
            AccountsConfig::default(),
            None,
            test_backend(),
            window,
            cx,
        );
        window.focus(&ws.focus, cx);
        ws
    });
    cx.run_until_parked();
    (ws, cx)
}

/// One browser tool call as `thread`, answered on the UI thread.
fn call(
    ws: &Entity<Workspace>,
    thread: i64,
    tool: BrowserTool,
    cx: &mut VisualTestContext,
) -> Result<Value, String> {
    let (reply, answer) = flume::bounded(1);
    ws.update_in(cx, |ws, window, cx| {
        ws.browser_call(thread, tool, reply, window, cx)
    });
    for _ in 0..400 {
        cx.run_until_parked();
        if let Ok(result) = answer.try_recv() {
            return result;
        }
        cx.executor().advance_clock(Duration::from_millis(50));
    }
    panic!("no answer");
}

fn open_url(url: &str) -> BrowserTool {
    BrowserTool::Open { url: url.into() }
}

fn eval(expression: &str) -> BrowserTool {
    BrowserTool::Eval {
        expression: expression.into(),
    }
}

fn tabs(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> Vec<Entity<BrowserPanel>> {
    ws.read_with(cx, |ws, _| {
        ws.browsers.iter().filter_map(|b| b.upgrade()).collect()
    })
}

fn owner(tab: &Entity<BrowserPanel>, cx: &mut VisualTestContext) -> Option<i64> {
    tab.read_with(cx, |p, _| p.agent.as_ref().map(|a| a.thread_id))
}

#[gpui::test]
fn tools_drive_own_tab(cx: &mut TestAppContext) {
    let (ws, cx) = workspace(cx);
    let pages = fake_pages(&ws, cx);

    let opened = call(&ws, 1, open_url("http://localhost:3000/"), cx).expect("open");
    assert_eq!(opened["reused"], json!(false), "{opened}");
    assert_eq!(opened["loaded"], json!(true), "{opened}");
    let all = tabs(&ws, cx);
    assert_eq!(all.len(), 1);
    assert_eq!(owner(&all[0], cx), Some(1), "the tab belongs to thread 1");

    let again = call(&ws, 1, open_url("https://example.com/"), cx).expect("reopen");
    assert_eq!(again["reused"], json!(true), "{again}");
    assert_eq!(tabs(&ws, cx).len(), 1, "one tab per thread: reused");
    assert_eq!(*pages.borrow()[0].url.borrow(), "https://example.com/");

    call(
        &ws,
        1,
        BrowserTool::Navigate {
            url: "localhost:5173".into(),
        },
        cx,
    )
    .expect("navigate");
    let url = call(&ws, 1, BrowserTool::Url, cx).expect("url");
    assert_eq!(url, json!({"url": "http://localhost:5173/"}));

    let title = call(&ws, 1, eval("document.title"), cx).expect("eval");
    assert_eq!(title, json!("Fake page"));
    let script = pages.borrow()[0].scripts.borrow().last().cloned();
    assert!(
        script.is_some_and(|s| s.starts_with("if (location.protocol")),
        "the in-page guard runs first"
    );
}

#[gpui::test]
fn file_urls_and_pages_refused(cx: &mut TestAppContext) {
    let (ws, cx) = workspace(cx);
    let pages = fake_pages(&ws, cx);

    for url in ["file:///etc/hosts", "/etc/hosts", "javascript:alert(1)"] {
        let err = call(&ws, 1, open_url(url), cx).expect_err(url);
        assert!(err.contains("http"), "{err}");
    }
    assert!(tabs(&ws, cx).is_empty(), "a refused URL opens no tab");

    call(&ws, 1, open_url("http://localhost:3000/"), cx).expect("open");
    let err = call(
        &ws,
        1,
        BrowserTool::Navigate {
            url: "file:///etc/hosts".into(),
        },
        cx,
    )
    .expect_err("navigate to a file");
    assert!(err.contains("http"), "{err}");
    let page = pages.borrow()[0].clone();
    assert_eq!(*page.url.borrow(), "http://localhost:3000/");

    // The page ends up on a local file (the user typed one into the URL bar).
    *page.url.borrow_mut() = "file:///Users/me/.ssh/config".into();
    let scripts_before = page.scripts.borrow().len();
    assert!(
        call(&ws, 1, BrowserTool::Url, cx).is_err(),
        "url of a file page"
    );
    let err = call(&ws, 1, eval("document.body.innerText"), cx).expect_err("eval in a file page");
    assert!(err.contains("http"), "{err}");
    assert_eq!(
        page.scripts.borrow().len(),
        scripts_before,
        "no script reaches a file page"
    );
}

#[gpui::test]
fn other_threads_tab_refused(cx: &mut TestAppContext) {
    let (ws, cx) = workspace(cx);
    let pages = fake_pages(&ws, cx);
    call(&ws, 1, open_url("http://localhost:3000/"), cx).expect("open");

    for tool in [
        BrowserTool::Url,
        eval("document.cookie"),
        BrowserTool::Navigate {
            url: "https://example.com/".into(),
        },
    ] {
        let err = call(&ws, 2, tool.clone(), cx).expect_err("another thread's tab");
        assert!(err.contains("browser_open"), "{tool:?}: {err}");
    }
    assert!(pages.borrow()[0].scripts.borrow().is_empty());
    assert_eq!(*pages.borrow()[0].url.borrow(), "http://localhost:3000/");

    let opened = call(&ws, 2, open_url("https://example.com/"), cx).expect("own tab");
    assert_eq!(
        opened["reused"],
        json!(false),
        "never reuses thread 1's tab"
    );
    let owners: Vec<_> = tabs(&ws, cx).iter().map(|t| owner(t, cx)).collect();
    assert_eq!(owners, [Some(1), Some(2)]);
}

#[gpui::test]
fn user_tab_refused(cx: &mut TestAppContext) {
    let (ws, cx) = workspace(cx);
    fake_pages(&ws, cx);
    cx.dispatch_action(OpenBrowser);
    cx.run_until_parked();
    let user = tabs(&ws, cx);
    assert_eq!(user.len(), 1);
    assert_eq!(owner(&user[0], cx), None, "a user tab has no agent");

    for tool in [BrowserTool::Url, eval("document.title")] {
        let err = call(&ws, 1, tool, cx).expect_err("the user's tab");
        assert!(err.contains("browser_open"), "{err}");
    }
    call(&ws, 1, open_url("http://localhost:3000/"), cx).expect("open");
    assert_eq!(tabs(&ws, cx).len(), 2, "the agent got its own tab");
    assert_eq!(owner(&user[0], cx), None, "the user's tab stays the user's");
}

#[gpui::test]
fn take_over_detaches(cx: &mut TestAppContext) {
    let (ws, cx) = workspace(cx);
    fake_pages(&ws, cx);
    call(&ws, 1, open_url("http://localhost:3000/"), cx).expect("open");
    let tab = tabs(&ws, cx).remove(0);

    tab.update(cx, |p, cx| p.take_over(cx));
    assert_eq!(owner(&tab, cx), None, "the indicator goes with the owner");
    let err = call(&ws, 1, eval("document.title"), cx).expect_err("taken over");
    assert!(err.contains("took it over"), "{err}");
    let reopened = call(&ws, 1, open_url("http://localhost:3000/"), cx).expect("open again");
    assert_eq!(
        reopened["reused"],
        json!(false),
        "a new tab, not the user's now"
    );
    assert_eq!(owner(&tab, cx), None);

    // Close removes the agent's tab.
    let agent_tab = tabs(&ws, cx).pop().expect("new tab");
    let weak = agent_tab.downgrade();
    agent_tab.update(cx, |_, cx| cx.emit(CloseBrowser));
    drop(agent_tab);
    cx.run_until_parked();
    assert!(weak.upgrade().is_none(), "closed tab dropped");
    assert!(call(&ws, 1, BrowserTool::Url, cx).is_err());
}

#[gpui::test]
fn eval_timeout_and_output_cap(cx: &mut TestAppContext) {
    let (ws, cx) = workspace(cx);
    fake_pages(&ws, cx);
    call(&ws, 1, open_url("http://localhost:3000/"), cx).expect("open");

    let err = call(&ws, 1, eval("big"), cx).expect_err("too big");
    assert!(err.contains(&MAX_EVAL_OUTPUT.to_string()), "{err}");

    let (reply, answer) = flume::bounded(1);
    ws.update_in(cx, |ws, window, cx| {
        ws.browser_call(1, eval("hang"), reply, window, cx)
    });
    cx.run_until_parked();
    assert!(answer.try_recv().is_err(), "still running");
    cx.executor()
        .advance_clock(BROWSER_TIMEOUT + Duration::from_secs(1));
    cx.run_until_parked();
    let err = answer.try_recv().expect("answered").expect_err("timed out");
    assert!(err.contains("did not finish"), "{err}");
}

#[gpui::test]
fn fake_claude_opens_and_evals_through_proxy(cx: &mut TestAppContext) {
    let page = serve_page("SkyeLab agent test");
    let (rig, accounts, config) = rig();
    let (ws, cx) = open(&rig, accounts, config, cx);
    fake_pages(&ws, cx);
    let store = rig.store.clone();

    let view = start(
        &ws,
        rig.work_a,
        &format!(r#"mcp:browser_open:{{"url":"{page}"}}"#),
        cx,
    );
    let id = thread_id(&view, cx);
    // The new tab answers once its page commits, on a UI timer.
    let mut opened = None;
    wait_until(cx, "browser_open's answer", |cx| {
        cx.executor().advance_clock(Duration::from_millis(50));
        opened = super::delegation_test::tool_result(&store, id, 0);
        opened.is_some()
    });
    let (err, text) = opened.expect("waited");
    assert!(!err, "browser_open refused: {text}");
    let owners: Vec<_> = tabs(&ws, cx).iter().map(|t| owner(t, cx)).collect();
    assert_eq!(owners, [Some(id)], "the thread owns its tab");

    send(
        &view,
        r#"mcp:browser_eval:{"expression":"document.title"}"#,
        cx,
    );
    let (err, text) = wait_result(cx, &store, id, 1);
    assert!(!err, "browser_eval refused: {text}");
    assert!(text.contains("SkyeLab agent test"), "{text}");
}
