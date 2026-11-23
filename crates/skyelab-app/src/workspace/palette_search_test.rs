//! Acceptance (package D): the palette finds threads of every project by their message text.

use super::manage_harness::rig;
use super::threads_test::wait_until;
use crate::palette::OPEN_PALETTE_KEY;
use gpui::TestAppContext;
use std::path::Path;

#[gpui::test]
fn palette_finds_threads_by_message_text_in_any_project(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    let other = {
        let mut s = r.f.store.lock().expect("lock");
        let p = s
            .create_project("other", Path::new("/elsewhere"))
            .expect("project");
        let t = s
            .create_thread(&skyelab_store::NewThread {
                project_id: p.id,
                title: "three",
                cwd: Path::new("/elsewhere"),
                account_id: "acme",
                status: "idle",
            })
            .expect("thread");
        s.append_event(
            t.id,
            "agent",
            r#"{"Assistant":{"content":[{"type":"text","text":"the walrus module"}]}}"#,
        )
        .expect("append");
        t.id
    };
    r.ws.update(cx, |ws, cx| ws.load_projects(None, cx));
    cx.run_until_parked();

    let search = r.ws.read_with(cx, |ws, _| ws.palette_search());
    let labels = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let out = labels.clone();
    cx.update(|_, cx| {
        let task = search("walrus".into(), cx);
        cx.spawn(async move |_| {
            *out.borrow_mut() = task.await.iter().map(|e| e.label.to_string()).collect();
        })
        .detach();
    });
    cx.run_until_parked();
    let labels = labels.borrow().clone();
    assert_eq!(labels, ["Open thread: three · other"]);

    cx.simulate_keystrokes(OPEN_PALETTE_KEY);
    cx.run_until_parked();
    cx.simulate_input("walrus module");
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    let ws = r.ws.clone();
    wait_until(cx, "the found thread opened", |cx| {
        ws.read_with(cx, |ws, cx| ws.open_view(other, cx).is_some())
    });
}
