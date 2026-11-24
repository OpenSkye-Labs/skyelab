//! Codex's token readings: `thread/tokenUsage/updated` carries the thread-wide `total` and the newest model call's
//! `last`.

use crate::{Event, Usage};
use serde_json::Value;

/// The thread's `total` from a `tokenUsage` reading. A resumed thread's total starts where it left off, so with no
/// earlier reading in this process (`base` unknown) the running turn began at `total - last`.
pub(super) fn reading(usage: &Value, base: &mut Option<Usage>) -> Option<Usage> {
    let read = |t: &Value| {
        Some(Usage {
            input: t["inputTokens"].as_u64()?,
            output: t["outputTokens"].as_u64()?,
        })
    };
    let total = read(&usage["total"])?;
    if base.is_none() {
        let last = read(&usage["last"]).unwrap_or_default();
        *base = Some(Usage {
            input: total.input.saturating_sub(last.input),
            output: total.output.saturating_sub(last.output),
        });
    }
    Some(total)
}

pub(super) fn context(usage: &Value) -> Option<Event> {
    let tokens = usage["last"]["totalTokens"].as_u64().filter(|t| *t > 0)?;
    Some(Event::Context {
        tokens: Some(tokens),
        window: usage["modelContextWindow"].as_u64().filter(|w| *w > 0),
        model: None,
    })
}
