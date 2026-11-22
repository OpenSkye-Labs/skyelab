//! An eval whose script throws a huge message must stay under the output cap.

use super::browser_agent::MAX_EVAL_OUTPUT;
use super::threads_test::test_backend;
use super::*;
use gpui::{TestAppContext, VisualTestContext};
use serde_json::{Value, json};
use skyelab_agent::mcp::BrowserTool;
use skyelab_browser::{Engine, EvalDone};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

/// What js::wrap's catch returns for `(() => { throw 'x'.repeat(1e6) })()`.
struct Thrower {
    url: RefCell<String>,
    loads: flume::Sender<String>,
}

impl Engine for Thrower {
    fn load(&self, url: &str, _: &gpui::App) -> anyhow::Result<()> {
        *self.url.borrow_mut() = url.to_owned();
        self.loads.send(url.to_owned())?;
        Ok(())
    }
    fn url(&self, _: &gpui::App) -> anyhow::Result<String> {
        Ok(self.url.borrow().clone())
    }
    fn eval(&self, _: &str, done: EvalDone, _: &gpui::App) -> anyhow::Result<()> {
        let envelope = json!({ "error": "x".repeat(1_000_000) }).to_string();
        done(serde_json::to_string(&envelope).expect("encode"));
        Ok(())
    }
}

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

#[gpui::test]
fn thrown_error_output_is_capped(cx: &mut TestAppContext) {
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
    ws.update(cx, |ws, _| {
        ws.browser_engine = Some(Rc::new(|| {
            let (loads, rx) = flume::unbounded();
            let page = Rc::new(Thrower {
                url: RefCell::default(),
                loads,
            });
            (page as Rc<dyn Engine>, rx)
        }))
    });
    call(
        &ws,
        1,
        BrowserTool::Open {
            url: "http://localhost:3000/".into(),
        },
        cx,
    )
    .expect("open");
    let result = call(
        &ws,
        1,
        BrowserTool::Eval {
            expression: "(() => { throw 'x'.repeat(1e6) })()".into(),
        },
        cx,
    );
    let text = match result {
        Ok(v) => v.to_string(),
        Err(e) => e,
    };
    assert!(
        text.len() <= MAX_EVAL_OUTPUT,
        "OUTPUT NOT BOUNDED: {} bytes handed back to the agent",
        text.len()
    );
}
