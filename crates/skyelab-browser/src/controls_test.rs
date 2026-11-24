use super::{external_url, find_script, zoom_step};

#[test]
fn zoom_steps() {
    assert_eq!(zoom_step(1.0, true), 1.1);
    assert_eq!(zoom_step(1.0, false), 0.9);
    assert_eq!(zoom_step(5.0, true), 5.0, "stops at the top");
    assert_eq!(zoom_step(0.25, false), 0.25, "stops at the bottom");
    assert_eq!(
        zoom_step(1.05, true),
        1.1,
        "an odd value snaps to the next step up"
    );
    assert_eq!(zoom_step(1.05, false), 1.0, "and to the next step down");
    assert_eq!(zoom_step(9.0, false), 5.0, "out of range comes back in");
}

/// The JS string literal in a find script, decoded back.
fn literal(script: &str) -> String {
    let start = script.find('"').expect("a string literal");
    let mut de = serde_json::Deserializer::from_str(&script[start..]).into_iter::<String>();
    de.next().expect("one literal").expect("valid JSON string")
}

#[test]
fn find_script_escapes() {
    for q in [
        r#"a"b"#,
        r"back\slash",
        "</script><b>",
        "line\nbreak\u{2028}",
    ] {
        let js = find_script(q, false, true);
        assert_eq!(literal(&js), q, "{js}");
    }
    let back = find_script("x", true, false);
    assert!(back.contains("true, true"), "backwards and wrap: {back}");
    assert!(
        !back.contains("collapseToStart"),
        "a step keeps the selection: {back}"
    );
    assert!(
        find_script("x", false, true).contains("collapseToStart"),
        "a new query starts at the match"
    );
    assert!(
        find_script("", false, true).contains("removeAllRanges"),
        "an empty query clears it"
    );
}

#[test]
fn external_urls_are_web_only() {
    assert!(external_url("http://localhost:3000/").is_some());
    assert!(external_url("https://example.com/a?b").is_some());
    assert!(external_url("file:///Users/me/run.command").is_none());
    assert!(external_url("about:blank").is_none());
    assert!(external_url("javascript:alert(1)").is_none());
    assert!(external_url("").is_none());
}

/// wry's macOS bridge unwraps the JSON encoding of a script's result, so a page whose `window.find` returns `NaN`
/// (or throws) would abort the app: the script must hand back a boolean it made itself.
#[test]
fn find_script_returns_its_own_boolean() {
    for js in [
        find_script("x", false, true),
        find_script("x", true, false),
        find_script("", false, true),
    ] {
        assert!(
            js.contains("=== true") || js.contains("return false; })()"),
            "{js}"
        );
        assert!(js.contains("catch"), "a throwing page answers false: {js}");
    }
}
