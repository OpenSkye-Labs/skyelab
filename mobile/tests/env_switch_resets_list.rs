//! Another environment's thread with the same id opens at its own top, not at the scroll left on the last one.

use gpui::{IntoElement as _, ListOffset, TestAppContext, VisualTestContext, point, px, size};
use skyelab_mobile::{
    app::MobileApp,
    source::{Connection, ThreadSource},
};
use skyelab_protocol::{Project, Thread, ThreadEvent};

struct Src(Vec<ThreadEvent>);

impl ThreadSource for Src {
    fn connection(&self) -> Connection {
        Connection::Connected
    }
    fn error(&self) -> Option<String> {
        None
    }
    fn projects(&self) -> Vec<Project> {
        vec![Project {
            id: 1,
            name: "app".into(),
            path: "/src/app".into(),
        }]
    }
    fn threads(&self, _: i64) -> Vec<Thread> {
        vec![Thread {
            id: 1,
            project_id: 1,
            title: "t".into(),
            account_id: "work".into(),
            account_label: None,
            status: "idle".into(),
            pinned: false,
            preview: None,
            created_at: 0,
            updated_at: 0,
        }]
    }
    fn events(&self, id: i64) -> &[ThreadEvent] {
        if id == 1 { &self.0 } else { &[] }
    }
    fn load_threads(&mut self, _: i64) {}
    fn watch(&mut self, _: i64) {}
    fn send_message(&mut self, _: i64, _: &str) {}
    fn interrupt(&mut self, _: i64) {}
}

fn events(n: i64, tag: &str) -> Vec<ThreadEvent> {
    (1..=n)
        .map(|seq| ThreadEvent {
            thread_id: 1,
            seq,
            kind: "prompt".into(),
            payload_json: serde_json::Value::String(format!("{tag} {seq}")).to_string(),
            created_at: 0,
        })
        .collect()
}

fn frame(app: &gpui::Entity<MobileApp>, cx: &mut VisualTestContext) {
    cx.draw(point(px(0.), px(0.)), size(px(390.), px(844.)), |_, _| {
        app.clone().into_any_element()
    });
}

#[gpui::test]
fn another_environments_thread_1_opens_at_the_top(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let dir = tempfile::tempdir().expect("temp dir");
    let connect: skyelab_mobile::app::Connect = Box::new(|name: &str| {
        let evs = if name.starts_with("laptop") {
            events(10_000, "A")
        } else {
            events(10_001, "B")
        };
        Ok(Box::new(Src(evs)) as Box<dyn ThreadSource>)
    });
    let (app, cx) =
        cx.add_window_view(|w, cx| MobileApp::new(dir.path().to_path_buf(), connect, w, cx));
    let input = app.read_with(cx, |a, _| a.env_input.clone());
    for (host, scroll) in [
        ("laptop.tail1234.ts.net", true),
        ("desktop.tail1234.ts.net", false),
    ] {
        input.update_in(cx, |s, w, cx| s.set_value(host, w, cx));
        app.update_in(cx, |a, w, cx| a.connect(w, cx));
        app.update(cx, |a, cx| a.open_project(1, cx));
        app.update_in(cx, |a, w, cx| a.open_thread(1, w, cx));
        frame(&app, cx);
        if scroll {
            app.read_with(cx, |a, _| {
                a.events_list.scroll_to(ListOffset {
                    item_ix: 5_000,
                    offset_in_item: px(0.),
                })
            });
            frame(&app, cx);
            // Thread -> Threads -> Projects -> Picker
            for _ in 0..3 {
                app.update(cx, |a, cx| a.back(cx));
            }
        }
    }
    let top = app.read_with(cx, |a, _| a.events_list.logical_scroll_top().item_ix);
    assert_eq!(
        top, 0,
        "stale scroll from the other environment: top item {top}"
    );
}
