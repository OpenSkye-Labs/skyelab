//! The stdio side: what `claude` runs as its MCP server.

use super::transport;
use super::{SOCKET_ENV, TOKEN_ENV};
use anyhow::{Context as _, Result};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead as _, BufReader, Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

/// What the agent reads once SkyeLab's end of the connection is gone. There is no reconnecting: the token was
/// one-use, and an app that restarted has no live view of this thread to answer for.
const GONE: &str = "SkyeLab's tools are gone for this session: SkyeLab closed the connection (the app quit or \
    restarted, or this thread was stopped there). Nothing was done. To use them again, start the thread again from \
    SkyeLab.";

/// `skyelab --mcp-proxy`: the socket and token come from the env the `--mcp-config` file sets.
pub fn proxy_main() -> Result<()> {
    let socket = std::env::var_os(SOCKET_ENV).context("SKYELAB_MCP_SOCKET is not set")?;
    let token = std::env::var(TOKEN_ENV).context("SKYELAB_MCP_TOKEN is not set")?;
    run_proxy(
        Path::new(&socket),
        &token,
        std::io::stdin(),
        std::io::stdout(),
    )
}

enum Msg {
    /// A line from SkyeLab.
    Server(Vec<u8>),
    ServerClosed,
    /// A request that came after the connection was gone: (id, is a tool call).
    Unsent(Value, bool),
    InputEnded,
}

/// Requests sent to SkyeLab and not answered yet, by their id's JSON text.
#[derive(Default)]
struct Pending {
    gone: bool,
    ids: HashMap<String, (Value, bool)>,
}

fn lock(p: &Mutex<Pending>) -> MutexGuard<'_, Pending> {
    // A panic while holding it leaves plain data behind, still consistent.
    p.lock().unwrap_or_else(|e| e.into_inner())
}

/// Sends `token`, then copies `input` to the server and the server's lines to `output`. Returns once `input` ended
/// and every request has its answer. If the server closes first, the proxy answers the agent's requests itself
/// with [`GONE`] until `input` ends.
pub fn run_proxy(
    socket: &Path,
    token: &str,
    input: impl Read + Send + 'static,
    mut output: impl Write,
) -> Result<()> {
    let stream = transport::connect(socket)
        .with_context(|| format!("connecting to SkyeLab at {}", socket.display()))?;
    let mut upstream = stream.try_clone().context("cloning the connection")?;
    let closer = stream.try_clone().context("cloning the connection")?;
    writeln!(upstream, "{token}").context("sending the token")?;
    let pending = Arc::new(Mutex::new(Pending::default()));
    let (tx, rx) = flume::unbounded();

    let (sent, to_main) = (pending.clone(), tx.clone());
    std::thread::Builder::new()
        .name("skyelab-mcp-proxy-in".into())
        .spawn(move || {
            let mut input = BufReader::new(input);
            let mut line = Vec::new();
            while matches!(input.read_until(b'\n', &mut line), Ok(n) if n > 0) {
                let request = request_of(&line);
                let gone = {
                    let mut p = lock(&sent);
                    if let (false, Some((key, id, tool))) = (p.gone, &request) {
                        p.ids.insert(key.clone(), (id.clone(), *tool));
                    }
                    p.gone
                };
                if gone {
                    if let Some((_, id, tool)) = request {
                        let _ = to_main.send(Msg::Unsent(id, tool));
                    }
                } else {
                    // A failed write means the server closed: `ServerClosed` answers what is pending.
                    let _ = upstream.write_all(&line).and_then(|()| upstream.flush());
                }
                line.clear();
            }
            let _ = to_main.send(Msg::InputEnded);
        })
        .context("spawning the proxy's input thread")?;

    std::thread::Builder::new()
        .name("skyelab-mcp-proxy-out".into())
        .spawn(move || {
            let mut reader = BufReader::new(stream);
            loop {
                let mut line = Vec::new();
                match reader.read_until(b'\n', &mut line) {
                    Ok(n) if n > 0 => {
                        if tx.send(Msg::Server(line)).is_err() {
                            return;
                        }
                    }
                    _ => break,
                }
            }
            let _ = tx.send(Msg::ServerClosed);
        })
        .context("spawning the proxy's output thread")?;

    let (mut input_ended, mut closed) = (false, false);
    let mut write = |line: &[u8]| -> Result<()> {
        output.write_all(line).context("writing to claude")?;
        output.flush().context("writing to claude")
    };
    for msg in rx.iter() {
        match msg {
            Msg::Server(line) => {
                if let Some(key) = response_key(&line) {
                    lock(&pending).ids.remove(&key);
                }
                write(&line)?;
            }
            Msg::ServerClosed => {
                closed = true;
                let left = {
                    let mut p = lock(&pending);
                    p.gone = true;
                    std::mem::take(&mut p.ids)
                };
                for (id, tool) in left.into_values() {
                    write(&gone_answer(id, tool))?;
                }
            }
            Msg::Unsent(id, tool) => write(&gone_answer(id, tool))?,
            Msg::InputEnded => input_ended = true,
        }
        if input_ended && (closed || lock(&pending).ids.is_empty()) {
            break;
        }
    }
    // Tells the server we're done, and wakes the output thread.
    let _ = closer.shutdown(std::net::Shutdown::Both);
    Ok(())
}

/// A request's (id key, id, is a tool call); `None` for a notification, a response or a line that isn't JSON.
fn request_of(line: &[u8]) -> Option<(String, Value, bool)> {
    let msg: Value = serde_json::from_slice(line).ok()?;
    let method = msg.get("method")?.as_str()?;
    let id = msg.get("id")?.clone();
    Some((id.to_string(), id, method == "tools/call"))
}

fn response_key(line: &[u8]) -> Option<String> {
    let msg: Value = serde_json::from_slice(line).ok()?;
    Some(msg.get("id")?.to_string())
}

/// A tool call gets a tool error the agent reads; any other request a JSON-RPC error.
fn gone_answer(id: Value, tool: bool) -> Vec<u8> {
    let answer = if tool {
        json!({"jsonrpc": "2.0", "id": id, "result": {"content": [{"type": "text", "text": GONE}], "isError": true}})
    } else {
        json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32000, "message": GONE}})
    };
    format!("{answer}\n").into_bytes()
}
