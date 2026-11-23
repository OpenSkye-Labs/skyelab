//! The eval result cap holds inside the page, so a huge value never crosses wry's bridge into the app.

use super::*;

#[test]
fn wrap_caps_results_in_page() {
    let js = wrap("1");
    // `.length` of a string primitive is its own property: the page can't replace it.
    assert!(js.contains(&format!(".length > {MAX_EVAL_OUTPUT}")), "{js}");
    assert!(js.contains("too_big"), "{js}");
}

#[test]
fn decode_maps_too_big_to_the_limit_error() {
    let raw = serde_json::to_string(r#"{"too_big":70000}"#).expect("encode");
    let err = decode(&raw).expect_err("too big").to_string();
    assert!(err.contains("70000"), "{err}");
    assert!(err.contains(&MAX_EVAL_OUTPUT.to_string()), "{err}");
}

/// Runs `wrap(expression)` in JavaScriptCore with an https `location`, and returns what the page hands back.
#[cfg(target_os = "macos")]
fn run_in_jsc(expression: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("page.js");
    let page = format!("var location = {{ protocol: \"https:\" }};\n{}", wrap(expression));
    std::fs::write(&path, page).expect("write script");
    let out = std::process::Command::new("/usr/bin/osascript")
        .args(["-l", "JavaScript"])
        .arg(&path)
        .output()
        .expect("osascript runs");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).expect("utf-8").trim_end().to_owned()
}

#[cfg(target_os = "macos")]
#[test]
fn wrapper_refuses_huge_results_in_page() {
    let huge = run_in_jsc(&format!("\"x\".repeat({})", MAX_EVAL_OUTPUT + 1));
    assert!(huge.len() < 100, "the huge string stayed in the page: {} bytes", huge.len());
    let envelope: Value = serde_json::from_str(&huge).expect("json");
    assert!(envelope["too_big"].as_u64().is_some_and(|n| n as usize > MAX_EVAL_OUTPUT), "{huge}");

    // Just under the cap still comes back whole.
    let fits = run_in_jsc(&format!("\"x\".repeat({})", MAX_EVAL_OUTPUT - 20));
    let envelope: Value = serde_json::from_str(&fits).expect("json");
    assert_eq!(envelope["ok"].as_str().map(str::len), Some(MAX_EVAL_OUTPUT - 20));
}
