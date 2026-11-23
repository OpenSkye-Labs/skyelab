use super::super::{Tool, tools};
use super::{BrowserTool, MAX_EXPRESSION, MAX_URL};
use serde_json::json;

fn parse(name: &str, args: serde_json::Value) -> Result<Tool, String> {
    tools::parse(name, &args)
}

#[test]
fn browser_tools_parse() {
    assert_eq!(
        parse("browser_open", json!({"url": "http://localhost:3000/"})),
        Ok(Tool::Browser(BrowserTool::Open {
            url: "http://localhost:3000/".into()
        }))
    );
    assert_eq!(
        parse("browser_navigate", json!({"url": "https://example.com"})),
        Ok(Tool::Browser(BrowserTool::Navigate {
            url: "https://example.com".into()
        }))
    );
    assert_eq!(
        parse("browser_url", serde_json::Value::Null),
        Ok(Tool::Browser(BrowserTool::Url))
    );
    assert_eq!(
        parse("browser_eval", json!({"expression": "document.title"})),
        Ok(Tool::Browser(BrowserTool::Eval {
            expression: "document.title".into()
        }))
    );
}

#[test]
fn browser_inputs_are_bounded_and_typed() {
    let long_url = format!("https://example.com/{}", "a".repeat(MAX_URL));
    for (name, args) in [
        ("browser_open", json!({})),
        ("browser_open", json!({"url": 5})),
        ("browser_open", json!({"url": "   "})),
        ("browser_open", json!({"url": long_url})),
        ("browser_navigate", json!({"url": null})),
        ("browser_eval", json!({"expression": ""})),
        ("browser_eval", json!({"expression": ["document.title"]})),
        (
            "browser_eval",
            json!({"expression": "1".repeat(MAX_EXPRESSION + 1)}),
        ),
        ("browser_eval", json!("document.title")),
    ] {
        assert!(
            parse(name, args.clone()).is_err(),
            "{name} {args} must be refused"
        );
    }
    assert!(
        parse(
            "browser_eval",
            json!({"expression": "1".repeat(MAX_EXPRESSION)})
        )
        .is_ok()
    );
}

#[test]
fn browser_tools_are_listed() {
    let list = tools::list();
    let names: Vec<&str> = list
        .as_array()
        .expect("array")
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    for n in [
        "browser_open",
        "browser_navigate",
        "browser_url",
        "browser_eval",
    ] {
        assert!(names.contains(&n), "{n} missing from {names:?}");
    }
    assert!(!names.contains(&"browser_screenshot"), "not implemented");
}
