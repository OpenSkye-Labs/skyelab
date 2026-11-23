//! Image and PDF bytes don't belong in the store: a screenshot echoed in a tool result is ~1.4 MB of base64 per row.
//! Each one is replaced by a placeholder that keeps its media type, decoded size and a short SHA-256.

use anyhow::{Context, Result};
use base64::Engine as _;
use rusqlite::{Transaction, params};
use serde_json::{Map, Value, json};

/// Below this a payload can't hold an image worth dropping, so it isn't parsed.
const MIN_LEN: usize = 1024;
/// Shorter strings are left alone: they are ids or tiny icons, not worth a placeholder.
const MIN_DATA: usize = 64;
/// A payload with none of these can't hold a shape `omit_here` strips, so it isn't parsed.
const MARKERS: [&str; 4] = ["base64", "\"image\"", "\"imageGeneration\"", "\"blob\""];

/// `payload` with every base64 image (or document) replaced by a placeholder, or `None` when there is none.
/// Handles the shapes agents send: Claude's `source: {type: base64, data}`, MCP's `{type: image, data, mimeType}`
/// and `{type: resource, resource: {blob}}`, Codex's `{type: image, url: "data:…;base64,…"}` and its
/// `{type: imageGeneration, result}`. Only strings that decode as base64 are dropped.
pub(crate) fn omit_images(payload: &str) -> Option<String> {
    if payload.len() < MIN_LEN || !MARKERS.iter().any(|m| payload.contains(m)) {
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
    {
        return strip(source, "data");
    }
    match obj.get("type").and_then(Value::as_str) {
        Some("image") => strip(obj, "data") || strip_data_url(obj),
        Some("imageGeneration") => strip(obj, "result"),
        Some("resource") => match obj.get_mut("resource") {
            Some(Value::Object(resource)) => strip(resource, "blob"),
            _ => false,
        },
        _ => false,
    }
}

/// Replaces `obj[key]`, when it is base64, with its size and hash.
fn strip(obj: &mut Map<String, Value>, key: &str) -> bool {
    let Some((bytes, sha)) = obj.get(key).and_then(Value::as_str).and_then(describe) else {
        return false;
    };
    obj.remove(key);
    obj.insert("omitted_bytes".into(), json!(bytes));
    obj.insert("sha256".into(), json!(sha));
    true
}

fn strip_data_url(obj: &mut Map<String, Value>) -> bool {
    let Some((media, (bytes, sha))) = obj
        .get("url")
        .and_then(Value::as_str)
        .and_then(|u| u.strip_prefix("data:")?.split_once(";base64,"))
        .and_then(|(media, data)| Some((media.to_owned(), describe(data)?)))
    else {
        return false;
    };
    obj.remove("url");
    obj.insert("media_type".into(), json!(media));
    obj.insert("omitted_bytes".into(), json!(bytes));
    obj.insert("sha256".into(), json!(sha));
    true
}

/// Decoded size and the first 16 hex digits of the bytes' SHA-256, if `data` is base64 long enough to drop.
fn describe(data: &str) -> Option<(usize, String)> {
    if data.len() < MIN_DATA {
        return None;
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.trim())
        .ok()?;
    let digest = ring::digest::digest(&ring::digest::SHA256, &bytes);
    let hex: String = digest.as_ref()[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Some((bytes.len(), hex))
}

/// Schema v12: rewrites the events an older build stored with image bytes, a batch at a time so a large store
/// isn't read into memory at once. Returns how many rows changed.
pub(crate) fn omit_stored_images(tx: &Transaction) -> Result<usize> {
    const BATCH: i64 = 200;
    let mut select = tx.prepare(
        "SELECT thread_id, seq, payload_json FROM events
         WHERE (thread_id, seq) > (?1, ?2) AND length(payload_json) >= ?3
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
