//! Own UI kit, stage 4a (.agent/plans/2026-11-28-own-ui-kit-small-pieces.md), criteria 1 and 2: the OS notification
//! for a background window is ours. It posts the toast's title and message under a tag of ours (a key replaces in
//! place), and a click on it runs the toast's handler in its window; a tag that isn't ours runs nothing.

use super::toast::{self, Toast};
use gpui::{
    Context, IntoElement, Render, SharedString, SystemNotificationResponse, TestAppContext,
    VisualTestContext, Window, div,
};
use std::{cell::Cell, rc::Rc};

/// A plain window: no gpui-component `Root` around it.
struct Host;
impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

fn host(cx: &mut TestAppContext) -> &mut VisualTestContext {
    cx.update(|cx| {
        // What lib.rs runs at start: gpui-component's init, the app identity, then our response handler (the later
        // one wins, so ours must come after theirs).
        super::init(cx);
        super::install_theme(cx).expect("every theme file parses");
    });
    let (_, cx) = cx.add_window_view(|_, _| Host);
    cx
}

fn post(cx: &mut VisualTestContext, t: Toast) {
    cx.update(|window, cx| toast::push_system(window, t, cx));
    cx.run_until_parked();
}

fn counting(t: Toast, n: &Rc<Cell<usize>>) -> Toast {
    let n = n.clone();
    t.on_click(move |_, _| n.set(n.get() + 1))
}

fn click(cx: &mut VisualTestContext, tag: SharedString) {
    cx.simulate_system_notification_response(SystemNotificationResponse {
        tag,
        action_id: None,
    });
    cx.run_until_parked();
}

/// The app starts through `ui::init`, and nothing re-runs gpui-component's init after it: that would put its response
/// handler back in place of ours, and a click on a notification would do nothing.
#[test]
fn the_app_starts_through_ui_init_alone() {
    let lib = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"))
        .expect("lib.rs");
    assert!(lib.contains("ui::init(cx);"), "lib.rs calls ui::init");
    let theirs = concat!("gpui_component", "::init(");
    assert!(
        !lib.contains(theirs),
        "lib.rs leaves gpui-component's init to ui::init"
    );
}

#[gpui::test]
fn posts_title_and_message_in_a_window_without_their_root(cx: &mut TestAppContext) {
    let cx = host(cx);
    post(cx, Toast::new("first").title("Thread completed"));
    post(cx, Toast::new("Only a message"));
    let shown = cx.shown_system_notifications();
    let pairs: Vec<_> = shown
        .iter()
        .map(|n| (n.title.to_string(), n.body.to_string()))
        .collect();
    assert_eq!(
        pairs,
        [
            ("Thread completed".to_string(), "first".to_string()),
            ("Only a message".to_string(), String::new()),
        ]
    );
    assert!(shown.iter().all(|n| n.actions.is_empty()));
}

#[gpui::test]
fn a_key_replaces_in_place_and_no_key_never_does(cx: &mut TestAppContext) {
    let cx = host(cx);
    post(cx, Toast::new("one").key("thread-1"));
    post(cx, Toast::new("two").key("thread-1"));
    let delivered = cx.delivered_system_notifications();
    assert_eq!(delivered.len(), 1, "{delivered:?}");
    assert_eq!(
        delivered[0].body.as_ref(),
        "",
        "no title: the message is the title"
    );
    assert_eq!(delivered[0].title.as_ref(), "two");
    post(cx, Toast::new("other thread").key("thread-2"));
    assert_eq!(
        cx.delivered_system_notifications().len(),
        2,
        "another key is another notification"
    );

    post(cx, Toast::new("a"));
    post(cx, Toast::new("a"));
    let shown = cx.shown_system_notifications();
    assert_eq!(shown.len(), 5);
    assert_eq!(shown[0].tag, shown[1].tag, "same key, same tag");
    assert_ne!(shown[3].tag, shown[4].tag, "no key: each its own");
    assert_ne!(shown[1].tag, shown[3].tag);
}

#[gpui::test]
fn a_click_runs_its_handler_in_its_window_and_retracts_it(cx: &mut TestAppContext) {
    let cx = host(cx);
    let clicks = Rc::new(Cell::new(0));
    post(cx, counting(Toast::new("done").key("thread-7"), &clicks));
    let tag = cx.shown_system_notifications()[0].tag.clone();
    cx.deactivate_window();
    cx.run_until_parked();
    assert!(!cx.update(|window, _| window.is_window_active()));

    click(cx, tag.clone());
    assert_eq!(clicks.get(), 1, "the toast's click handler ran");
    assert!(
        cx.update(|window, _| window.is_window_active()),
        "its window came forward"
    );
    assert!(
        cx.dismissed_system_notifications().contains(&tag),
        "retracted from the notification center"
    );

    click(cx, tag);
    assert_eq!(
        clicks.get(),
        1,
        "a second report of the same click runs nothing"
    );
}

#[gpui::test]
fn a_tag_that_is_not_ours_runs_nothing(cx: &mut TestAppContext) {
    let cx = host(cx);
    let clicks = Rc::new(Cell::new(0));
    post(cx, counting(Toast::new("done").key("k"), &clicks));
    let ours = cx.shown_system_notifications()[0].tag.clone();
    for foreign in ["k", "thread-7", "gpui-component/notification/k"] {
        assert_ne!(ours.as_ref(), foreign);
        click(cx, foreign.into());
    }
    assert_eq!(clicks.get(), 0);
    assert!(cx.dismissed_system_notifications().is_empty());
}

#[gpui::test]
fn only_the_newest_hundred_are_remembered(cx: &mut TestAppContext) {
    let cx = host(cx);
    let (first, last) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
    post(cx, counting(Toast::new("oldest").key("n-0"), &first));
    for i in 1..100 {
        post(cx, Toast::new("middle").key(format!("n-{i}")));
    }
    post(cx, counting(Toast::new("newest").key("n-100"), &last));
    let shown = cx.shown_system_notifications();
    assert_eq!(shown.len(), 101);
    click(cx, shown[0].tag.clone());
    click(cx, shown[100].tag.clone());
    assert_eq!(first.get(), 0, "pushed out by the hundred after it");
    assert_eq!(last.get(), 1);
}
