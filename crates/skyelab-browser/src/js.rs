//! wry's macOS bridge `unwrap()`s the JSON serialization of a script's raw result, so a result like `NaN`
//! aborts the app. An agent's expression can break out of any wrapper made of text, so its script ([`wrap`]) runs
//! with no result callback and leaves its answer at [`SLOT`]. Then [`read_back`], which has no agent text in it,
//! hands that answer over: a well-formed string primitive, capped, whatever the page or the expression did.

use anyhow::{Context as _, anyhow};
use serde_json::Value;

/// Agents may only run scripts in http and https pages. Checked in the page the script runs in,
/// since the engine's URL can name a load still pending over a `file://` page. `location` can't be
/// redefined by the page, and a top-level throw stops the rest of the script.
const PAGE_GUARD: &str =
    "if (location.protocol !== \"http:\" && location.protocol !== \"https:\") throw 0;\n";

/// Where [`wrap`] leaves its answer for [`read_back`]. WebKit runs one webview's scripts in order, and the
/// expression runs to completion (a Promise is not awaited), so the read-back always follows its own script.
const SLOT: &str = "__skyelab_eval";

/// Largest eval result handed back, in bytes of JSON text. The page checks it in UTF-16 units (never more than
/// the bytes), so a huge value stays in the page; the app checks the bytes again.
pub const MAX_EVAL_OUTPUT: usize = 64 << 10;

/// A thrown message is cut to this many UTF-16 units, so its envelope fits under the cap even if JSON escapes
/// every unit (6 bytes for a control character).
const MAX_THROWN: usize = MAX_EVAL_OUTPUT / 8;

/// The agent's script: run it without a result callback, then run [`read_back`] for its answer. It ends in
/// `void 0` so engines that hand a no-callback script's value to the app anyway (WebView2) get nothing big.
pub(crate) fn wrap(expression: &str) -> String {
    format!(
        "{PAGE_GUARD}window.{SLOT} = (() => {{ try {{ const v = (\n{expression}\n); \
         return `${{JSON.stringify({{ ok: v === undefined ? null : v }})}}`; }} \
         catch (e) {{ return `${{JSON.stringify({{ error: `${{e}}`.slice(0, {MAX_THROWN}) }})}}`; }} }})();\nvoid 0;"
    )
}

/// Takes the answer [`wrap`] left and returns it as a string primitive, or "" (a refusal) when there is none.
/// Only syntax the page can't replace: `typeof`, `.length` and in-range indexing of a string primitive (an index
/// past the end reads `String.prototype`), comparisons. A lone surrogate is refused: wry turns the result into
/// UTF-8 and unwraps that. A page that isn't http or https gets nothing back, even if it filled the slot itself.
pub(crate) fn read_back() -> String {
    r#"(() => { if (location.protocol !== "http:" && location.protocol !== "https:") return "";
  let s; try { s = window.SLOT; delete window.SLOT; } catch (e) { return ""; }
  if (typeof s !== "string") return "";
  if (s.length > MAX) return `{"too_big":${s.length}}`;
  for (let i = 0; i < s.length; i++) {
    const c = s[i];
    if (c >= "\ud800" && c <= "\udbff" && i + 1 < s.length && s[i + 1] >= "\udc00" && s[i + 1] <= "\udfff") { i++; continue; }
    if (c >= "\ud800" && c <= "\udfff") return `{"error":"the result is not valid text"}`;
  }
  return s; })()"#
        .replace("SLOT", SLOT)
        .replace("MAX", &MAX_EVAL_OUTPUT.to_string())
}

/// wry hands back the page's string JSON-encoded, so the envelope is encoded twice. "" (no read-back result) and
/// an empty string (nothing left at the slot) are both a script that never reached the end of its wrapper.
pub(crate) fn decode(raw: &str) -> anyhow::Result<Value> {
    let envelope_json: String = match raw {
        "" => String::new(),
        raw => serde_json::from_str(raw)
            .with_context(|| format!("unexpected eval result from the webview: {raw:?}"))?,
    };
    if envelope_json.is_empty() {
        return Err(anyhow!(
            "script refused: the page is not http or https, or the script did not compile or did not run to its end"
        ));
    }
    let mut envelope: Value =
        serde_json::from_str(&envelope_json).context("eval result is not JSON")?;
    if let Some(n) = envelope.get("too_big") {
        return Err(anyhow!(
            "the result is {n} characters, over the {MAX_EVAL_OUTPUT}-byte limit; return less (a slice or a summary)"
        ));
    }
    if let Some(error) = envelope.get("error") {
        return Err(anyhow!(
            "script threw: {}",
            error.as_str().unwrap_or_default()
        ));
    }
    Ok(envelope
        .get_mut("ok")
        .map(Value::take)
        .unwrap_or(Value::Null))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_ok_value() {
        let raw = serde_json::to_string(r#"{"ok":{"title":"Example Domain"}}"#).expect("encode");
        assert_eq!(
            decode(&raw).expect("ok"),
            serde_json::json!({"title": "Example Domain"})
        );
    }

    #[test]
    fn surfaces_script_errors() {
        let raw = serde_json::to_string(r#"{"error":"ReferenceError: x is not defined"}"#)
            .expect("encode");
        let err = decode(&raw).expect_err("threw").to_string();
        assert!(err.contains("ReferenceError"), "{err}");
    }

    #[test]
    fn rejects_unwrapped_results() {
        // A syntax error or a page-sabotaged wrapper comes back as "" or a bare value.
        assert!(decode("").is_err());
        assert!(decode("42").is_err());
    }

    #[test]
    fn wrap_survives_trailing_line_comment() {
        let js = wrap("document.title // the title");
        assert!(js.contains("\ndocument.title // the title\n)"), "{js}");
    }

    #[test]
    fn wrap_checks_the_running_page_scheme_before_anything_else() {
        // `WKWebView.URL` reports a pending load, so only the page itself knows where the script runs.
        // A top-level throw aborts the whole script, including anything appended by breaking out.
        let js = wrap("1)})(); steal(); (() => {(1");
        assert!(js.starts_with(PAGE_GUARD), "{js}");
        // The only parenthesis is the `if`'s: no calls the page could replace.
        assert_eq!(PAGE_GUARD.matches('(').count(), 1, "{PAGE_GUARD}");
        assert!(PAGE_GUARD.starts_with("if ("), "{PAGE_GUARD}");
    }

    #[test]
    fn empty_result_is_a_refusal() {
        let err = decode("").expect_err("guard threw").to_string();
        assert!(err.contains("http or https"), "{err}");
        // The read-back found nothing at the slot.
        let err = decode("\"\"").expect_err("nothing left").to_string();
        assert!(err.contains("http or https"), "{err}");
    }

    #[test]
    fn wrap_returns_template_literals() {
        let js = wrap("1");
        assert_eq!(js.matches("`${JSON.stringify(").count(), 2, "{js}");
    }

    #[test]
    fn read_back_has_no_calls_the_page_could_replace() {
        let js = read_back();
        // The only parentheses: the IIFE, its call, `catch (e)`, and control flow.
        for name in [
            "JSON",
            "String",
            "Object",
            "charCodeAt",
            "isWellFormed",
            "test(",
        ] {
            assert!(!js.contains(name), "{name} in {js}");
        }
        assert!(
            js.contains(&format!("s.length > {MAX_EVAL_OUTPUT}")),
            "{js}"
        );
    }
}

#[cfg(test)]
#[path = "js_cap_test.rs"]
mod cap_tests;
