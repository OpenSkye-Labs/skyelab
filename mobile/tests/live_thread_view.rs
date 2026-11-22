//! Acceptance: the thread screen shows events the daemon pushes, with no user action, because the app drains
//! the source's inbox in a running task.

pub mod common;

use std::time::{Duration, Instant};

use common::{Env, NODE_NAME, options};
use gpui::TestAppContext;
use skyelab_mobile::{app::MobileApp, remote::RemoteSource, source::ThreadSource};

#[gpui::test]
fn pushed_events_appear_on_the_thread_screen(cx: &mut TestAppContext) {
    // Real sockets and client threads wake the app's task.
    cx.executor().allow_parking();
    cx.update(gpui_component::init);
    let env = Env::start();
    let port = env.port();
    let dir = tempfile::tempdir().expect("temp dir");
    let data = dir.path().to_path_buf();
    let (app, cx) = cx.add_window_view(|window, cx| {
        let connect = Box::new(move |_: &str| {
            Ok(Box::new(RemoteSource::connect(NODE_NAME, options(port))?) as Box<dyn ThreadSource>)
        });
        MobileApp::new(data.clone(), connect, window, cx)
    });

    let env_input = app.read_with(cx, |a, _| a.env_input.clone());
    env_input.update_in(cx, |s, window, cx| {
        s.set_value("desktop.example.ts.net", window, cx)
    });
    app.update_in(cx, |a, window, cx| a.connect(window, cx));

    let wait = |cx: &mut gpui::VisualTestContext, what: &str, done: &dyn Fn(&MobileApp) -> bool| {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            cx.run_until_parked();
            if app.read_with(cx, |a, _| done(a)) {
                return;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    };

    wait(cx, "projects", &|a| !a.projects().is_empty());
    app.update(cx, |a, cx| a.open_project(env.project_id, cx));
    wait(cx, "threads", &|a| !a.threads().is_empty());
    app.update_in(cx, |a, window, cx| a.open_thread(env.thread_id, window, cx));
    wait(cx, "stored events", &|a| a.events().len() == 2);

    env.push(
        "agent",
        r#"{"Assistant":{"content":[{"type":"text","text":"pushed while idle"}]}}"#,
    );
    wait(cx, "the pushed event", &|a| a.events().len() == 3);
}
