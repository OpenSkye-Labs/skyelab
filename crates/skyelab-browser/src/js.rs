//! The `eval_js` bridge: wrap an expression so the page always returns a JSON string, then decode it.
//!
//! wry's macOS bridge `unwrap()`s the JSON serialization of the raw result, so a script returning
//! a DOM node or a cycle would abort the app. Stringifying in the page keeps the native value a string.

use anyhow::{Context as _, anyhow};
use serde_json::Value;

/// Wraps a JS expression; the page returns `JSON.stringify({ok})` or `JSON.stringify({error})`.
pub(crate) fn wrap(expression: &str) -> String {
    format!(
        "(() => {{ try {{ const v = (\n{expression}\n); \
         return JSON.stringify({{ ok: v === undefined ? null : v }}); }} \
         catch (e) {{ return JSON.stringify({{ error: String(e) }}); }} }})()"
    )
}

/// Decodes what wry hands back for a [`wrap`]ped script: a JSON string holding our JSON envelope.
pub(crate) fn decode(raw: &str) -> anyhow::Result<Value> {
    let inner: String = serde_json::from_str(raw)
        .with_context(|| format!("unexpected eval result from the webview: {raw:?}"))?;
    let mut envelope: Value = serde_json::from_str(&inner).context("eval result is not JSON")?;
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
        // The bridge hands back "" when it has nothing (pending page, native error).
        assert!(decode("").is_err());
        assert!(decode("42").is_err());
    }

    #[test]
    fn wrap_keeps_expression_on_its_own_lines() {
        // A trailing `// comment` in the expression must not swallow the closing paren.
        let js = wrap("document.title // the title");
        assert!(js.contains("\ndocument.title // the title\n)"), "{js}");
    }
}
