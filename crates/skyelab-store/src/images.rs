//! Image and PDF bytes don't belong in the store: a screenshot echoed in a tool result is ~1.4 MB of base64 per row.
//! Each one is replaced by a placeholder that keeps its media type, decoded size and a short SHA-256.

use anyhow::{Context, Result};
use base64::Engine as _;
use rusqlite::{Transaction, params};
use serde_json::{Map, Value, json};

/// Below this a payload can't hold an image worth dropping, so it isn't parsed.
const MIN_LEN: usize = 1024;

/// `payload` with every base64 image (or document) replaced by a placeholder, or `None` when there is none.
/// Handles the three shapes agents send: Claude's `source: {type: base64, data}`, MCP's `{type: image, data,
/// mimeType}` and Codex's `{type: image, url: "data:…;base64,…"}`.
pub(crate) fn omit_images(payload: &str) -> Option<String> {
    if payload.len() < MIN_LEN || !(payload.contains("base64") || payload.contains("\"image\"")) {
        return None;
    }
    let mut v: Value = serde_json::from_str(payload).ok()?;
    walk(&mut v).then(|| v.to_string())
}

fn walk(v: &mut Value) -> bool {
    match v {
        Value::Array(items) => items.iter_mut().fold(false, |hit, i| walk(i) | hit),
        Value::Object(obj) => {
            let here = omit_here(obj);
            obj.values_mut().fold(here, |hit, i| walk(i) | hit)
        }
        _ => false,
    }
}

fn omit_here(obj: &mut Map<String, Value>) -> bool {
    if let Some(Value::Object(source)) = obj.get_mut("source")
        && source.get("type").and_then(Value::as_str) == Some("base64")
        && let Some(Value::String(data)) = source.remove("data")
    {
        let (bytes, sha) = describe(&data);
        source.insert("omitted_bytes".into(), json!(bytes));
        source.insert("sha256".into(), json!(sha));
        return true;
    }
    if obj.get("type").and_then(Value::as_str) != Some("image") {
        return false;
    }
    if let Some(Value::String(data)) = obj.get("data") {
        let (bytes, sha) = describe(data);
        obj.remove("data");
        obj.insert("omitted_bytes".into(), json!(bytes));
        obj.insert("sha256".into(), json!(sha));
        return true;
    }
    if let Some(Value::String(url)) = obj.get("url")
        && let Some((media, data)) = url
            .strip_prefix("data:")
            .and_then(|u| u.split_once(";base64,"))
    {
        let (media, (bytes, sha)) = (media.to_owned(), describe(data));
        obj.remove("url");
        obj.insert("media_type".into(), json!(media));
        obj.insert("omitted_bytes".into(), json!(bytes));
        obj.insert("sha256".into(), json!(sha));
        return true;
    }
    false
}

/// Decoded size and the first 16 hex digits of the bytes' SHA-256 (of the text, if it isn't valid base64).
fn describe(data: &str) -> (usize, String) {
    let decoded = base64::engine::general_purpose::STANDARD.decode(data.trim());
    let bytes = decoded.as_deref().unwrap_or(data.as_bytes());
    let digest = ring::digest::digest(&ring::digest::SHA256, bytes);
    let hex: String = digest.as_ref()[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    (bytes.len(), hex)
}

/// Schema v12: rewrites the events an older build stored with image bytes, a batch at a time so a large store
/// isn't read into memory at once. Returns how many rows changed.
pub(crate) fn omit_stored_images(tx: &Transaction) -> Result<usize> {
    const BATCH: i64 = 200;
    let mut select = tx.prepare(
        "SELECT thread_id, seq, payload_json FROM events
         WHERE (thread_id, seq) > (?1, ?2) AND length(payload_json) >= ?3
           AND (instr(payload_json, 'base64') > 0 OR instr(payload_json, '\"image\"') > 0)
         ORDER BY thread_id, seq LIMIT ?4",
    )?;
    let mut update =
        tx.prepare("UPDATE events SET payload_json = ?3 WHERE thread_id = ?1 AND seq = ?2")?;
    let (mut after, mut changed) = ((i64::MIN, i64::MIN), 0);
    loop {
        let rows: Vec<(i64, i64, String)> = select
            .query_map(params![after.0, after.1, MIN_LEN as i64, BATCH], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?
            .collect::<rusqlite::Result<_>>()
            .context("reading events with images")?;
        let Some(last) = rows.last() else {
            return Ok(changed);
        };
        after = (last.0, last.1);
        for (thread, seq, payload) in &rows {
            if let Some(slim) = omit_images(payload) {
                update.execute(params![thread, seq, slim])?;
                changed += 1;
            }
        }
    }
}
