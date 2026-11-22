//! Review fixes: open and navigate wait for the new page, and Take over or Close ends the agent's hold on a
//! tab even mid-call.

use super::panels::{BrowserPanel, CloseBrowser};
use super::threads_test::test_backend;
use super::*;
use gpui::{TestAppContext, VisualTestContext};
use serde_json::{Value, json};
use skyelab_agent::mcp::BrowserTool;
use skyelab_browser::{Engine, EvalDone};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

/// A page whose loads commit only when the test says so, and whose scripts finish only when it says so.
#[derive(Default)]
struct Page {
    url: RefCell<String>,
    loads: RefCell<Option<flume::Sender<String>>>,
    hold: Cell<bool>,
    held: RefCell<Vec<String>>,
    load_calls: Cell<usize>,
    evals: RefCell<Vec<EvalDone>>,
}

impl Page {
    fn commit_held(&self) {
        for url in self.held.borrow_mut().drain(..) {
            if let Some(tx) = &*self.loads.borrow() {
                tx.send(url).expect("send");
            }
        }
    }

    fn finish_eval(&self, value: Value) {
        let done = self.evals.borrow_mut().pop().expect("an eval is running");
        done(serde_json::to_string(&json!({ "ok": value }).to_string()).expect("encode"));
    }
}

impl Engine for Page {
    fn load(&self, url: &str, _: &gpui::App) -> anyhow::Result<()> {
        *self.url.borrow_mut() = url.to_owned();
        self.load_calls.set(self.load_calls.get() + 1);
        self.held.borrow_mut().push(url.to_owned());
        if !self.hold.get() {
            self.commit_held();
        }
        Ok(())
    }

    fn url(&self, _: &gpui::App) -> anyhow::Result<String> {
        Ok(self.url.borrow().clone())
    }

    fn eval(&self, _: &str, done: EvalDone, _: &gpui::App) -> anyhow::Result<()> {
        self.evals.borrow_mut().push(done);
        Ok(())
    }
}

fn setup(cx: &mut TestAppContext) -> (Entity<Workspace>, Rc<Page>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::settings::init(cx);
        skyelab_terminal::init(cx);
        init(cx);
    });
    let folder = std::env::temp_dir();
    let (ws, cx) = cx.add_window_view(|window, cx| {
        Workspace::new(
            folder,
            None,
            AccountsConfig::default(),
            None,
            test_backend(),
            window,
            cx,
        )
    });
    let page = Rc::new(Page::default());
    let shared = page.clone();
    ws.update(cx, |ws, _| {
        ws.browser_engine = Some(Rc::new(move || {
            let (tx, rx) = flume::unbounded();
            *shared.loads.borrow_mut() = Some(tx);
            (shared.clone() as Rc<dyn Engine>, rx)
        }))
    });
    (ws, page, cx)
}

fn start(
    ws: &Entity<Workspace>,
    tool: BrowserTool,
    cx: &mut VisualTestContext,
) -> flume::Receiver<Result<Value, String>> {
    let (reply, answer) = flume::bounded(1);
    ws.update_in(cx, |ws, window, cx| {
        ws.browser_call(1, tool, reply, window, cx)
    });
    answer
}

/// Runs the UI for `ticks` 50 ms steps, returning the answer if one came.
fn pump(
    answer: &flume::Receiver<Result<Value, String>>,
    ticks: usize,
    cx: &mut VisualTestContext,
) -> Option<Result<Value, String>> {
    for _ in 0..ticks {
        cx.run_until_parked();
        if let Ok(r) = answer.try_recv() {
            return Some(r);
        }
        cx.executor().advance_clock(Duration::from_millis(50));
    }
    None
}

fn open(url: &str) -> BrowserTool {
    BrowserTool::Open { url: url.into() }
}

fn agent_tab(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> Entity<BrowserPanel> {
    ws.read_with(cx, |ws, _| ws.browsers.iter().find_map(|b| b.upgrade()))
        .expect("a tab")
}

#[gpui::test]
fn reused_open_and_navigate_wait_for_the_new_page(cx: &mut TestAppContext) {
    let (ws, page, cx) = setup(cx);
    let a = start(&ws, open("http://localhost:3000/a"), cx);
    assert_eq!(
        pump(&a, 5, cx).expect("answer").expect("open")["loaded"],
        json!(true)
    );

    page.hold.set(true);
    for tool in [
        open("http://localhost:3000/b"),
        BrowserTool::Navigate {
            url: "http://localhost:3000/c".into(),
        },
    ] {
        let answer = start(&ws, tool, cx);
        assert!(
            pump(&answer, 5, cx).is_none(),
            "answered before the page committed"
        );
        page.commit_held();
        let done = pump(&answer, 5, cx).expect("answer").expect("ok");
        assert_eq!(done["loaded"], json!(true), "{done}");
    }

    // A page that never commits: the call still answers, saying so.
    let answer = start(&ws, open("http://localhost:3000/d"), cx);
    let done = pump(&answer, 400, cx).expect("answer").expect("ok");
    assert_eq!(done["loaded"], json!(false), "{done}");
}

#[gpui::test]
fn take_over_reloads_and_drops_the_eval_in_flight(cx: &mut TestAppContext) {
    let (ws, page, cx) = setup(cx);
    let a = start(&ws, open("http://localhost:3000/"), cx);
    pump(&a, 5, cx).expect("answer").expect("open");

    let eval = start(
        &ws,
        BrowserTool::Eval {
            expression: "document.title".into(),
        },
        cx,
    );
    assert!(pump(&eval, 2, cx).is_none(), "script still running");
    let loads = page.load_calls.get();
    agent_tab(&ws, cx).update(cx, |p, cx| p.take_over(cx));
    assert_eq!(
        page.load_calls.get(),
        loads + 1,
        "take over reloads the page"
    );

    page.finish_eval(json!("secret the user typed"));
    let err = pump(&eval, 5, cx)
        .expect("answer")
        .expect_err("user owns it now");
    assert!(err.contains("took over"), "{err}");
}

#[gpui::test]
fn closing_the_tab_while_it_loads_answers_with_an_error(cx: &mut TestAppContext) {
    let (ws, page, cx) = setup(cx);
    page.hold.set(true);
    let answer = start(&ws, open("http://localhost:3000/"), cx);
    assert!(pump(&answer, 2, cx).is_none());
    let tab = agent_tab(&ws, cx);
    tab.update(cx, |_, cx| cx.emit(CloseBrowser));
    drop(tab);
    let err = pump(&answer, 5, cx)
        .expect("answer")
        .expect_err("tab closed");
    assert!(err.contains("closed"), "{err}");
}
