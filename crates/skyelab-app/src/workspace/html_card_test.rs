//! Which tool rows become page cards.
use super::page_of;
use crate::workspace::thread_model::ToolItem;
use serde_json::json;

fn tool(name: &str, result: Option<&str>, is_error: bool) -> ToolItem {
    let raw = json!({"html": "<p>hi</p>", "title": "Sales", "height": 240});
    ToolItem {
        id: "toolu_1".into(),
        name: name.into(),
        input: raw.to_string(),
        raw,
        result: result.map(Into::into),
        is_error,
    }
}

#[test]
fn only_a_finished_successful_skyelab_render_is_a_card() {
    let page = Some(("Sales", "<p>hi</p>", 240));
    assert_eq!(
        page_of(&tool("mcp__skyelab__html_render", Some("{}"), false)),
        page
    );
    assert_eq!(
        page_of(&tool("mcp__skyelab__html_render", None, false)),
        None,
        "still running"
    );
    assert_eq!(
        page_of(&tool("mcp__skyelab__html_render", Some("no"), true)),
        None,
        "refused"
    );
    assert_eq!(
        page_of(&tool("mcp__other__html_render", Some("{}"), false)),
        None
    );
    assert_eq!(
        page_of(&tool("mcp__skyelab__html_preview", Some("{}"), false)),
        None
    );
}

#[test]
fn the_page_theme_has_t3s_variables_for_both_modes() {
    gpui::TestAppContext::single().update(|cx| {
        gpui_component::init(cx);
        for dark in [false, true] {
            let theme = super::page_theme(dark, cx);
            assert_eq!(theme.dark, dark);
            let names: Vec<_> = theme.vars.iter().map(|v| v.0).collect();
            for name in [
                "--background",
                "--foreground",
                "--primary",
                "--accent",
                "--chart-1",
                "--chart-6",
                "--radius",
                "--font-sans",
                "--font-mono",
            ] {
                assert!(names.contains(&name), "{name}");
            }
            assert!(theme.vars[0].1.starts_with('#') && theme.vars[0].1.len() == 9);
        }
        let (light, dark) = (super::page_theme(false, cx), super::page_theme(true, cx));
        assert_ne!(
            light.vars[0], dark.vars[0],
            "light and dark backgrounds differ"
        );
    });
}

/// End to end: the fake claude's `chart` calls html_render through the real proxy and server; the call is
/// stored with the thread's events and draws as a card. `peek` (html_preview) answers, here with an error,
/// since a test window has no web engine.
#[gpui::test]
fn a_rendered_page_is_stored_and_drawn_as_a_card(cx: &mut gpui::TestAppContext) {
    use crate::workspace::delegation_test::{open, rig, send, start, thread_id, wait_result};
    use crate::workspace::thread_items::ItemsView as _;
    use crate::workspace::thread_model::Item;

    let (rig, accounts, config) = rig();
    let (ws, cx) = open(&rig, accounts, config, cx);
    let view = start(&ws, rig.work_a, "chart", cx);
    let thread = thread_id(&view, cx);
    let (is_error, text) = wait_result(cx, &rig.store, thread, 0);
    assert!(!is_error, "{text}");
    assert!(text.contains("Shown to the reader"), "{text}");
    crate::test_wait::wait_until(cx, "the card's tool row", |cx| {
        view.read_with(cx, |v, _| {
            v.items()
                .iter()
                .any(|i| matches!(i, Item::Tool(t) if page_of(t).is_some()))
        })
    });
    view.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    assert!(cx.debug_bounds("html-card").is_some(), "drawn as a card");

    send(&view, "peek", cx);
    let (is_error, _) = wait_result(cx, &rig.store, thread, 1);
    assert!(is_error, "no web engine in a test window");
}
