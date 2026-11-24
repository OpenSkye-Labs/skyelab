use super::super::{Tool, tools};
use super::{HtmlTool, MAX_HTML, MAX_TITLE, is_rendered_page};
use serde_json::json;

fn parse(name: &str, args: serde_json::Value) -> Result<Tool, String> {
    tools::parse(name, &args)
}

#[test]
fn render_parses_and_trims_the_title() {
    assert_eq!(
        parse(
            "html_render",
            json!({"html": "<p>hi</p>", "title": " Sales ", "height": 320})
        ),
        Ok(Tool::Html(HtmlTool::Render {
            html: "<p>hi</p>".into(),
            title: "Sales".into(),
            height: 320,
        }))
    );
}

#[test]
fn render_refuses_bad_input() {
    let ok = json!({"html": "<p>hi</p>", "title": "T", "height": 320});
    let with = |k: &str, v: serde_json::Value| {
        let mut a = ok.clone();
        a[k] = v;
        parse("html_render", a)
    };
    assert!(with("height", json!(79)).unwrap_err().contains("height"));
    assert!(with("height", json!(2001)).unwrap_err().contains("height"));
    assert!(with("height", json!(300.5)).is_err());
    assert!(with("height", json!("300")).is_err());
    assert!(with("title", json!("  ")).unwrap_err().contains("title"));
    assert!(with("title", json!("x".repeat(MAX_TITLE + 1))).is_err());
    assert!(with("html", json!("")).unwrap_err().contains("html"));
    assert!(with("html", json!("x".repeat(MAX_HTML + 1))).is_err());
    assert!(with("html", json!(["<p>"])).is_err());
    assert!(parse("html_render", json!({"html": "<p>hi</p>", "title": "T"})).is_err());
    assert!(with("html", json!("x".repeat(MAX_HTML))).is_ok());
}

#[test]
fn preview_defaults_and_bounds() {
    assert_eq!(
        parse("html_preview", json!({"html": "<p>hi</p>"})),
        Ok(Tool::Html(HtmlTool::Preview {
            html: "<p>hi</p>".into(),
            width: 728,
            dark: true,
        }))
    );
    assert_eq!(
        parse(
            "html_preview",
            json!({"html": "x", "width": 390, "appearance": "light"})
        ),
        Ok(Tool::Html(HtmlTool::Preview {
            html: "x".into(),
            width: 390,
            dark: false,
        }))
    );
    assert!(parse("html_preview", json!({"html": "x", "width": 239})).is_err());
    assert!(parse("html_preview", json!({"html": "x", "width": 1601})).is_err());
    assert!(parse("html_preview", json!({"html": "x", "appearance": "sepia"})).is_err());
    assert!(parse("html_preview", json!({"html": "x", "appearance": 1})).is_err());
}

#[test]
fn both_tools_are_listed() {
    let list = tools::list();
    let names: Vec<_> = list
        .as_array()
        .expect("a list")
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert!(names.contains(&"html_render") && names.contains(&"html_preview"));
}

#[test]
fn only_a_successful_skyelab_render_is_a_page() {
    let tool = |name: &str, result: Option<&str>, is_error| crate::timeline::ToolItem {
        id: "t".into(),
        name: name.into(),
        input: String::new(),
        raw: json!({}),
        result: result.map(Into::into),
        is_error,
    };
    assert!(is_rendered_page(&tool(
        "mcp__skyelab__html_render",
        Some("{}"),
        false
    )));
    assert!(!is_rendered_page(&tool(
        "mcp__skyelab__html_render",
        None,
        false
    )));
    assert!(!is_rendered_page(&tool(
        "mcp__skyelab__html_render",
        Some("no"),
        true
    )));
    assert!(!is_rendered_page(&tool(
        "mcp__other__html_render",
        Some("{}"),
        false
    )));
    assert!(!is_rendered_page(&tool(
        "mcp__skyelab__html_preview",
        Some("{}"),
        false
    )));
}

#[test]
fn viewport_height_units_are_refused_but_lookalikes_pass() {
    let render = |html: &str| {
        parse(
            "html_render",
            json!({"html": html, "title": "T", "height": 200}),
        )
    };
    for bad in [
        "<div style='height:100vh'>",
        "<style>.a{min-height: 50.5VH}</style>",
        "<style>.a{height:.5dvh}</style>",
        "<style>.a{height:calc(100svh - 8px)}</style>",
        "<div style=\"height:1lvh;\">",
    ] {
        let e = render(bad).expect_err(bad);
        assert!(e.contains("viewport height"), "{e}");
    }
    assert!(parse("html_preview", json!({"html": "<p style='height:9vh'>"})).is_err());
    for good in [
        "<p>vh is a unit</p>",
        "<img src='data:image/png;base64,AAA1vhBBB'>",
        "<p>a+1vh</p>",
        "<div style='height:100vw'>",
        "<p>100vh-like</p>",
        "<p>v1vhx</p>",
        "vh",
        "",
    ] {
        if !good.is_empty() {
            assert!(render(good).is_ok(), "{good}");
        }
    }
}
