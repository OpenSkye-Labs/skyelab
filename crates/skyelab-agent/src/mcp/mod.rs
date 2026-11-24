//! SkyeLab's own MCP server, so an agent can delegate to other threads and drive its own browser tab.
//!
//! `claude` spawns the proxy (`skyelab --mcp-proxy`) from the `--mcp-config` file a [`Grant`] writes. The proxy
//! connects to the app (a unix socket in a private (0700) dir, or on Windows a per-run named pipe only this user can
//! open), sends its one-use token, then pipes stdio both ways. The server speaks just enough MCP (JSON-RPC 2.0, one
//! message per line) for tools: `initialize`, `ping`, `tools/list` and `tools/call`. Every tool input is validated
//! here; the [`Host`] (the app) decides the rest.

mod browser;
mod proxy;
mod server;
mod tools;
mod transport;

pub use browser::BrowserTool;
pub use proxy::{proxy_main, run_proxy};
pub use server::{Grant, Server, remove_stale_runs};
pub use tools::Tool;
pub(crate) use transport::create_private_file;

use serde_json::Value;
use std::path::PathBuf;

/// The proxy's env: where the app listens (a socket path, or a pipe name on Windows), and its one-use token.
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

    /// [`Host::call`], with `granted` saying whether the caller's grant still stands, so a long call (a wait) can
    /// stop once the agent is gone.
    fn call_granted(
        &self,
        caller: &Caller,
        tool: Tool,
        granted: &dyn Fn() -> bool,
    ) -> Result<Value, String> {
        let _ = granted;
        self.call(caller, tool)
    }
}

/// The command `claude` runs as the MCP server.
#[derive(Debug, Clone)]
pub struct Proxy {
    pub program: PathBuf,
    pub args: Vec<String>,
}
