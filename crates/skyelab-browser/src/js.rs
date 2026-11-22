//! wry's macOS bridge `unwrap()`s the JSON serialization of a script's raw result, so a script
//! returning a DOM node or a cycle would abort the app. The wrapper stringifies in the page, and a
//! template literal forces a string primitive even if the page replaced `JSON.stringify` or `String`.

use anyhow::{Context as _, anyhow};
use serde_json::Value;

/// Agents may only run scripts in http and https pages. Checked in the page the script runs in,
/// since the engine's URL can name a load still pending over a `file://` page. `location` can't be
/// redefined by the page, and a top-level throw stops the rest of the script.
const PAGE_GUARD: &str =
    "if (location.protocol !== \"http:\" && location.protocol !== \"https:\") throw 0;\n";

pub(crate) fn wrap(expression: &str) -> String {
    format!(
        "{PAGE_GUARD}(() => {{ try {{ const v = (\n{expression}\n); \
         return `${{JSON.stringify({{ ok: v === undefined ? null : v }})}}`; }} \
         catch (e) {{ return `${{JSON.stringify({{ error: `${{e}}` }})}}`; }} }})()"
    )
}

/// wry hands back the page's string JSON-encoded, so the envelope is encoded twice.
pub(crate) fn decode(raw: &str) -> anyhow::Result<Value> {
    if raw.is_empty() {
        return Err(anyhow!(
            "script refused: the page is not http or https, or the script did not compile"
        ));
    }
    let envelope_json: String = serde_json::from_str(raw)
        .with_context(|| format!("unexpected eval result from the webview: {raw:?}"))?;
    let mut envelope: Value =
        serde_json::from_str(&envelope_json).context("eval result is not JSON")?;
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
    }

    #[test]
    fn wrap_returns_template_literals() {
        let js = wrap("1");
        assert_eq!(js.matches("return `${JSON.stringify(").count(), 2, "{js}");
    }
}
