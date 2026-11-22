//! SkyeLab's own MCP server, so an agent can delegate to other threads and drive its own browser tab.
//!
//! `claude` spawns the proxy (`skyelab --mcp-proxy`) from the `--mcp-config` file a [`Grant`] writes. The proxy
//! connects to the app's unix socket in a private (0700) dir, sends its one-use token, then pipes stdio both
//! ways. The server speaks just enough MCP (JSON-RPC 2.0, one message per line) for tools: `initialize`, `ping`,
//! `tools/list` and `tools/call`. Every tool input is validated here; the [`Host`] (the app) decides the rest.
//!
//! unix only: std has no named pipes.

mod browser;
mod proxy;
mod server;
mod tools;

pub use browser::BrowserTool;
pub use proxy::{proxy_main, run_proxy};
pub use server::{Grant, Server, remove_stale_runs};
pub use tools::Tool;

use serde_json::Value;
use std::path::PathBuf;

/// The proxy's env: where the app listens, and its one-use token.
pub const SOCKET_ENV: &str = "SKYELAB_MCP_SOCKET";
pub const TOKEN_ENV: &str = "SKYELAB_MCP_TOKEN";

/// The thread whose agent is calling: the grant it was launched with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller {
    pub thread_id: i64,
    /// The account the caller's agent runs under.
    pub account_id: String,
}

/// Answers tool calls. `Err` is shown to the agent as a tool error.
pub trait Host: Send + Sync {
    fn call(&self, caller: &Caller, tool: Tool) -> Result<Value, String>;
}

/// The command `claude` runs as the MCP server.
#[derive(Debug, Clone)]
pub struct Proxy {
    pub program: PathBuf,
    pub args: Vec<String>,
}
