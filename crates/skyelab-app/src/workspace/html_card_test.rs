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
// The fake claude and the MCP socket are unix only.
#[cfg(unix)]
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

#[gpui::test]
fn html_width_changes_target_width_and_snapshot_cache_key(cx: &mut gpui::TestAppContext) {
    use gpui::{Context, IntoElement, Render, Window, div, px, size};
    use skyelab_core::HtmlWidth;

    struct Blank;
    impl Render for Blank {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    cx.update(gpui_component::init);
    let (_, cx) = cx.add_window_view(|_, _| Blank);
    cx.simulate_resize(size(px(1200.), px(700.)));
    cx.run_until_parked();

    let (std_w, wide_w, full_w) = cx.update(|window, _| {
        (
            super::target_width(HtmlWidth::Standard, window),
            super::target_width(HtmlWidth::Wide, window),
            super::target_width(HtmlWidth::Full, window),
        )
    });
    assert_eq!(std_w, 728);
    assert_eq!(wide_w, 960);
    assert!(
        (240..=1600).contains(&full_w) && full_w > wide_w,
        "Full width at 1200 px viewport should exceed Wide (960) and clamp to 240..=1600; got {full_w}"
    );

    // Narrow viewport clamps Full cleanly to the 240..=1600 range.
    cx.simulate_resize(size(px(200.), px(700.)));
    cx.run_until_parked();
    let narrow_full = cx.update(|window, _| super::target_width(HtmlWidth::Full, window));
    assert_eq!(narrow_full, 240);

    // Width changes produce distinct snapshot cache keys so cards re-render at the new width.
    let theme = cx.update(|_, cx| super::page_theme(true, cx));
    let hosts = vec!["cdn.jsdelivr.net".to_owned()];
    let k_std = super::shot_key("toolu_1", "<p>chart</p>", &theme, &hosts, 728);
    let k_wide = super::shot_key("toolu_1", "<p>chart</p>", &theme, &hosts, 960);
    let k_full = super::shot_key("toolu_1", "<p>chart</p>", &theme, &hosts, 1280);
    assert_ne!(k_std, k_wide);
    assert_ne!(k_wide, k_full);
    assert_ne!(k_std, k_full);
}
