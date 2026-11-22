//! Acceptance: the thread screen draws a 10,000-event thread through `list` + `ListState`, building only the rows
//! on screen, and an appended event is spliced in rather than re-laying out the whole thread.

use std::time::Instant;

use gpui::{IntoElement as _, ListOffset, TestAppContext, VisualTestContext, point, px, size};
use skyelab_mobile::{
    app::{MobileApp, Screen},
    source::{Connection, ThreadSource},
};
use skyelab_protocol::{Project, Thread, ThreadEvent};

const N: i64 = 10_000;

fn prompt(seq: i64, text: &str) -> ThreadEvent {
    ThreadEvent {
        thread_id: 1,
        seq,
        kind: "prompt".into(),
        payload_json: serde_json::Value::String(text.into()).to_string(),
        created_at: 0,
    }
}

/// One project, one thread of `N` prompts; a sent message is appended like a pushed event.
struct LongSource(Vec<ThreadEvent>);

impl ThreadSource for LongSource {
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
            title: "long".into(),
            account_id: "work".into(),
            status: "idle".into(),
            created_at: 0,
            updated_at: 0,
        }]
    }
    // A slice: the screen reads it every frame, so it must not copy 10,000 events each time.
    fn events(&self, thread_id: i64) -> &[ThreadEvent] {
        if thread_id == 1 { &self.0 } else { &[] }
    }
    fn load_threads(&mut self, _: i64) {}
    fn watch(&mut self, _: i64) {}
    fn send_message(&mut self, _: i64, text: &str) {
        let seq = self.0.len() as i64 + 1;
        self.0.push(prompt(seq, text));
    }
    fn interrupt(&mut self, _: i64) {}
}

fn frame(app: &gpui::Entity<MobileApp>, cx: &mut VisualTestContext) -> usize {
    let before = app.read_with(cx, |a, _| a.rows_built);
    cx.draw(point(px(0.), px(0.)), size(px(390.), px(844.)), |_, _| {
        app.clone().into_any_element()
    });
    app.read_with(cx, |a, _| a.rows_built) - before
}

#[gpui::test]
fn a_10k_event_thread_renders_only_visible_rows_and_appends_by_splice(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let dir = tempfile::tempdir().expect("temp dir");
    let data = dir.path().to_path_buf();
    let connect: skyelab_mobile::app::Connect = Box::new(|_: &str| {
        let events = (1..=N)
            .map(|s| prompt(s, &format!("message {s}")))
            .collect();
        Ok(Box::new(LongSource(events)) as Box<dyn ThreadSource>)
    });
    let (app, cx) = cx.add_window_view(|window, cx| MobileApp::new(data, connect, window, cx));
    let env_input = app.read_with(cx, |a, _| a.env_input.clone());
    env_input.update_in(cx, |s, window, cx| {
        s.set_value("laptop.tail1234.ts.net", window, cx)
    });
    app.update_in(cx, |a, window, cx| a.connect(window, cx));
    app.update(cx, |a, cx| a.open_project(1, cx));
    app.update_in(cx, |a, window, cx| a.open_thread(1, window, cx));
    app.read_with(cx, |a, _| {
        assert_eq!(
            a.screen,
            Screen::Thread {
                project_id: 1,
                thread_id: 1
            }
        )
    });

    let t = Instant::now();
    let open = frame(&app, cx);
    let open_time = t.elapsed();
    app.read_with(cx, |a, _| {
        assert_eq!(a.events_list.item_count(), N as usize)
    });
    assert!(open > 0 && open < 100, "first frame built {open} rows");

    app.read_with(cx, |a, _| {
        a.events_list.scroll_to(ListOffset {
            item_ix: 5_000,
            offset_in_item: px(0.),
        })
    });
    frame(&app, cx);
    let composer = app.read_with(cx, |a, _| a.composer.clone());
    composer.update_in(cx, |s, window, cx| s.set_value("one more", window, cx));
    let t = Instant::now();
    app.update_in(cx, |a, window, cx| a.send(window, cx));
    let append = frame(&app, cx);
    let append_time = t.elapsed();
    app.read_with(cx, |a, _| {
        assert_eq!(a.events().len(), N as usize + 1);
        assert_eq!(a.events_list.item_count(), N as usize + 1);
        // A `reset` would drop the scroll anchor and every measured height.
        assert_eq!(a.events_list.logical_scroll_top().item_ix, 5_000);
        assert!(a.events_list.bounds_for_item(5_000).is_some());
    });
    assert!(append < 100, "append frame built {append} rows");
    eprintln!(
        "mobile 10k: first frame {open_time:?} ({open} rows), append+frame {append_time:?} ({append} rows)"
    );
}
