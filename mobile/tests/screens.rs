//! Acceptance: the environment name is validated and remembered, and the screens walk
//! picker → projects → threads → thread, where send appends and stop interrupts.

use gpui::TestAppContext;
use gpui_component::input::InputState;
use skyelab_mobile::{
    app::{MobileApp, Screen},
    env_store,
    source::FakeSource,
};

#[test]
fn environment_name_is_validated() {
    assert_eq!(
        env_store::validate("  Laptop.Tail1234.ts.net "),
        Ok("laptop.tail1234.ts.net".to_string())
    );
    for bad in [
        "",
        "laptop",
        "localhost",
        "100.64.0.1",
        "   ",
        "ws://laptop",
        "laptop:8080",
        "lap top",
        "-laptop",
        "laptop-",
        "a..b",
        ".laptop",
        "laptop.",
        &"a".repeat(64),
        &format!("{}.ts.net", "a.".repeat(130)),
    ] {
        assert!(
            env_store::validate(bad).is_err(),
            "{bad:?} must be rejected"
        );
    }
}

#[test]
fn environment_name_is_remembered_on_disk() {
    let dir = tempfile::tempdir().expect("temp dir");
    let data = dir.path().join("not-yet-created");
    assert_eq!(env_store::load(&data), None);
    env_store::save(&data, "laptop.tail1234.ts.net").expect("save");
    assert_eq!(
        env_store::load(&data).as_deref(),
        Some("laptop.tail1234.ts.net")
    );
    // A hand-edited, invalid file reads as nothing remembered rather than a bad dial target.
    std::fs::write(data.join(env_store::FILE), "ws://evil\n").expect("write");
    assert_eq!(env_store::load(&data), None);
}

#[gpui::test]
fn walks_picker_to_thread_then_sends_and_stops(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let dir = tempfile::tempdir().expect("temp dir");
    let data = dir.path().to_path_buf();
    let (app, cx) = cx.add_window_view(|window, cx| {
        MobileApp::new(data.clone(), Box::new(FakeSource::new()), window, cx)
    });
    let set = |input: gpui::Entity<InputState>, text: &str, cx: &mut gpui::VisualTestContext| {
        let text = text.to_string();
        input.update_in(cx, |s, window, cx| s.set_value(text, window, cx));
    };

    app.read_with(cx, |a, _| assert_eq!(a.screen, Screen::Picker));

    // An invalid name stays on the picker with an error and saves nothing.
    set(app.read_with(cx, |a, _| a.env_input.clone()), "ws://x", cx);
    app.update_in(cx, |a, window, cx| a.connect(window, cx));
    app.read_with(cx, |a, _| {
        assert_eq!(a.screen, Screen::Picker);
        assert!(a.error.is_some());
    });
    assert_eq!(env_store::load(&data), None);

    set(
        app.read_with(cx, |a, _| a.env_input.clone()),
        "laptop.tail1234.ts.net",
        cx,
    );
    app.update_in(cx, |a, window, cx| a.connect(window, cx));
    app.read_with(cx, |a, _| assert_eq!(a.screen, Screen::Projects));
    assert_eq!(
        env_store::load(&data).as_deref(),
        Some("laptop.tail1234.ts.net")
    );

    let project = app.read_with(cx, |a, _| a.projects()[0].id);
    app.update(cx, |a, cx| a.open_project(project, cx));
    app.read_with(cx, |a, _| {
        assert_eq!(
            a.screen,
            Screen::Threads {
                project_id: project
            }
        )
    });

    let thread = app.read_with(cx, |a, _| a.threads()[0].id);
    app.update_in(cx, |a, window, cx| a.open_thread(thread, window, cx));
    app.read_with(cx, |a, _| {
        assert_eq!(
            a.screen,
            Screen::Thread {
                project_id: project,
                thread_id: thread
            }
        )
    });
    let before = app.read_with(cx, |a, _| a.events().len());

    // Blank messages are not sent.
    set(app.read_with(cx, |a, _| a.composer.clone()), "   ", cx);
    app.update_in(cx, |a, window, cx| a.send(window, cx));
    app.read_with(cx, |a, _| assert_eq!(a.events().len(), before));

    set(
        app.read_with(cx, |a, _| a.composer.clone()),
        "run the tests",
        cx,
    );
    app.update_in(cx, |a, window, cx| a.send(window, cx));
    app.read_with(cx, |a, cx| {
        let events = a.events();
        assert_eq!(events.len(), before + 1);
        let last = events.last().expect("an event was appended");
        assert_eq!(last.kind, "user");
        assert!(last.payload_json.contains("run the tests"));
        assert!(a.composer.read(cx).value().is_empty(), "composer clears");
    });

    app.update(cx, |a, cx| a.stop(cx));
    app.read_with(cx, |a, _| {
        assert_eq!(
            a.events().last().map(|e| e.kind.as_str()),
            Some("interrupted")
        );
        assert_eq!(a.thread().map(|t| t.status).as_deref(), Some("interrupted"));
    });

    // Back walks up one level at a time, and stops at the picker.
    for expected in [
        Screen::Threads {
            project_id: project,
        },
        Screen::Projects,
        Screen::Picker,
        Screen::Picker,
    ] {
        app.update(cx, |a, cx| a.back(cx));
        app.read_with(cx, |a, _| assert_eq!(a.screen, expected));
    }
}

#[gpui::test]
fn remembered_environment_prefills_the_picker(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let dir = tempfile::tempdir().expect("temp dir");
    env_store::save(dir.path(), "laptop.tail1234.ts.net").expect("save");
    let data = dir.path().to_path_buf();
    let (app, cx) = cx.add_window_view(|window, cx| {
        MobileApp::new(data.clone(), Box::new(FakeSource::new()), window, cx)
    });
    app.read_with(cx, |a, cx| {
        assert_eq!(a.screen, Screen::Picker);
        assert_eq!(a.env_input.read(cx).value(), "laptop.tail1234.ts.net");
    });
}
