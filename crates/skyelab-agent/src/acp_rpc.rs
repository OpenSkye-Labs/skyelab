//! ACP JSON-RPC helpers: request params, mode ids and error classification.

use crate::Mode;
use crate::acp::AcpConfig;
use crate::acp_asks::cancelled_reply;
use serde_json::{Value, json};

pub(crate) fn initialize_params(cfg: &AcpConfig) -> Value {
    json!({
        "protocolVersion": 1,
        "clientCapabilities": {
            "fs": {
                "readTextFile": false,
                "writeTextFile": false,
            },
            "terminal": false,
        },
        "clientInfo": {
            "name": cfg.client_name,
            "version": cfg.client_version,
        },
    })
}

pub(crate) fn acp_mode_id(mode: Mode) -> &'static str {
    match mode {
        Mode::Default => "default",
        Mode::Plan => "plan",
        Mode::AcceptEdits => "auto_edit",
        Mode::Bypass => "yolo",
    }
}

pub(crate) fn rpc_error_message(v: &Value) -> Option<String> {
    v.get("error").filter(|e| !e.is_null()).map(|e| {
        e["message"]
            .as_str()
            .unwrap_or("ACP request failed")
            .to_owned()
    })
}

pub(crate) fn is_auth_required_error(code: i64, msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    code == -32000
        || lower.contains("authentication required")
        || lower.contains("no browser is available")
        || lower.contains("sign in")
}

pub(crate) fn is_session_gone_error(code: i64, msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    code == -32002
        || lower.contains("not found")
        || lower.contains("no session")
        || lower.contains("does not exist")
}

pub(crate) fn current_model_from_result(result: &Value) -> Option<String> {
    if let Some(arr) = result["configOptions"].as_array() {
        for opt in arr {
            if opt["id"] == "model"
                && let Some(cur) = opt["currentValue"].as_str()
                && !cur.is_empty()
            {
                return Some(cur.to_owned());
            }
        }
    }
    result["models"]["currentModelId"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// Validates an incoming JSON-RPC 2.0 request `id`: scalar number or bounded string only.
pub(crate) fn valid_rpc_id(id: &Value) -> bool {
    match id {
        Value::Number(_) => true,
        Value::String(s) => !s.is_empty() && s.len() <= 256,
        _ => false,
    }
}

pub(crate) fn cancel_permission_reply(rpc_id: &Value) -> String {
    cancelled_reply(rpc_id)
}
