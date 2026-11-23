use gpui::TestAppContext;
use skyelab_mobile::{
    app::{Connect, MobileApp},
    source::{FakeSource, ThreadSource},
};

#[gpui::test]
fn a_draft_does_not_follow_you_to_another_thread(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let dir = tempfile::tempdir().expect("temp dir");
    let data = dir.path().to_path_buf();
    let (app, cx) =
        cx.add_window_view(|window, cx| MobileApp::new(data.clone(), fake(), window, cx));
    let env_input = app.read_with(cx, |a, _| a.env_input.clone());
    env_input.update_in(cx, |s, window, cx| {
        s.set_value("laptop.tail1234.ts.net", window, cx)
    });
    app.update_in(cx, |a, window, cx| a.connect(window, cx));
    let threads: Vec<i64> = app.read_with(cx, |a, _| a.threads().iter().map(|t| t.id).collect());

    app.update_in(cx, |a, window, cx| a.open_thread(threads[0], window, cx));
    let composer = app.read_with(cx, |a, _| a.composer.clone());
    composer.update_in(cx, |s, window, cx| s.set_value("half-written", window, cx));
    app.update(cx, |a, cx| a.back(cx));
    app.update_in(cx, |a, window, cx| a.open_thread(threads[1], window, cx));

    app.read_with(cx, |a, cx| assert_eq!(a.composer.read(cx).value(), ""));
}

fn fake() -> Connect {
    Box::new(|_: &str| Ok(Box::new(FakeSource::new()) as Box<dyn ThreadSource>))
}
