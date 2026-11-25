//! Codex reports a server that needs a sign-in as a failed startup: an expired one by its failureReason, a missing
//! one only in the error text that says to run `codex mcp login`. Both read as needs-auth (the card's Sign in).

use super::items::mcp_status;
use crate::Event;
use serde_json::json;

fn status(p: serde_json::Value) -> String {
    match mcp_status(&p) {
        Some(Event::Mcp { servers, .. }) => servers[0].status.clone(),
        other => panic!("not an MCP report: {other:?}"),
    }
}

#[test]
fn codex_mcp_sign_in_failure_reads_as_needs_auth() {
    let expired = json!({ "name": "linear", "status": "failed", "failureReason": "reauthenticationRequired" });
    assert_eq!(status(expired), "needs-auth");
    let missing = json!({
        "name": "linear", "status": "failed",
        "error": "The linear MCP server is not logged in. Run `codex mcp login linear`."
    });
    assert_eq!(status(missing), "needs-auth");
    let other = json!({ "name": "linear", "status": "failed", "error": "connection refused" });
    assert_eq!(status(other), "failed");
}
