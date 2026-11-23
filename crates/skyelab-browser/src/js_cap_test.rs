//! The eval result cap holds inside the page, so a huge value never crosses wry's bridge into the app, and
//! the read-back only ever hands over well-formed text.

use super::*;

#[test]
fn read_back_caps_results_in_page() {
    let js = read_back();
    // `.length` of a string primitive is its own property: the page can't replace it.
    assert!(
        js.contains(&format!("s.length > {MAX_EVAL_OUTPUT}")),
        "{js}"
    );
    assert!(js.contains("too_big"), "{js}");
}

#[test]
fn decode_maps_too_big_to_the_limit_error() {
    let raw = serde_json::to_string(r#"{"too_big":70000}"#).expect("encode");
    let err = decode(&raw).expect_err("too big").to_string();
    assert!(err.contains("70000"), "{err}");
    assert!(err.contains(&MAX_EVAL_OUTPUT.to_string()), "{err}");
}

/// Runs `wrap(expression)` then the read-back in JavaScriptCore with an https `location`, and returns what the
/// read-back hands over.
#[cfg(target_os = "macos")]
fn run_in_jsc(expression: &str) -> String {
    run_scripts_in_jsc(&wrap(expression))
}

#[cfg(target_os = "macos")]
fn run_scripts_in_jsc(script: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("page.js");
    let page = format!(
        "var location = {{ protocol: \"https:\" }}; var window = this;\n{script}\n;{}",
        read_back()
    );
    std::fs::write(&path, page).expect("write script");
    let out = std::process::Command::new("/usr/bin/osascript")
        .args(["-l", "JavaScript"])
        .arg(&path)
        .output()
        .expect("osascript runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .expect("utf-8")
        .trim_end()
        .to_owned()
}

#[cfg(target_os = "macos")]
#[test]
fn wrapper_refuses_huge_results_in_page() {
    let huge = run_in_jsc(&format!("\"x\".repeat({})", MAX_EVAL_OUTPUT + 1));
    assert!(
        huge.len() < 100,
        "the huge string stayed in the page: {} bytes",
        huge.len()
    );
    let envelope: Value = serde_json::from_str(&huge).expect("json");
    assert!(
        envelope["too_big"]
            .as_u64()
            .is_some_and(|n| n as usize > MAX_EVAL_OUTPUT),
        "{huge}"
    );

    // Just under the cap still comes back whole.
    let fits = run_in_jsc(&format!("\"x\".repeat({})", MAX_EVAL_OUTPUT - 20));
    let envelope: Value = serde_json::from_str(&fits).expect("json");
    assert_eq!(
        envelope["ok"].as_str().map(str::len),
        Some(MAX_EVAL_OUTPUT - 20)
    );
}

#[cfg(target_os = "macos")]
#[test]
fn wrapper_cuts_a_huge_thrown_message_in_page() {
    let thrown = run_in_jsc(&format!(
        "(() => {{ throw \"x\".repeat({}); }})()",
        MAX_EVAL_OUTPUT * 4
    ));
    let envelope: Value = serde_json::from_str(&thrown).expect("json");
    assert_eq!(envelope["error"].as_str().map(str::len), Some(MAX_THROWN));
}

#[cfg(target_os = "macos")]
#[test]
fn read_back_refuses_what_wry_cannot_carry() {
    // A broken-out script can plant anything at the slot; only well-formed strings come back.
    let lone = run_scripts_in_jsc("window.__skyelab_eval = \"a\\ud800b\";");
    assert_eq!(lone, r#"{"error":"the result is not valid text"}"#);
    let trailing = run_scripts_in_jsc("window.__skyelab_eval = \"a\\udc00\";");
    assert_eq!(trailing, r#"{"error":"the result is not valid text"}"#);
    let pair = run_scripts_in_jsc("window.__skyelab_eval = \"\\ud83d\\ude00\";");
    assert_eq!(pair, "\u{1F600}");
    for planted in ["NaN", "{}", "undefined", "({ toString() { return 'x'; } })"] {
        let got = run_scripts_in_jsc(&format!("window.__skyelab_eval = {planted};"));
        assert_eq!(got, "", "{planted}");
    }
    // Nothing left: the slot is cleared by the read.
    assert_eq!(run_scripts_in_jsc(""), "");
}
