//! The socket listener, the grants (one-use tokens), and the JSON-RPC loop.

use super::{Caller, Host, Proxy, SOCKET_ENV, TOKEN_ENV, tools};
use anyhow::{Context as _, Result};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

/// Longest JSON-RPC line read; a longer one closes the connection.
const MAX_LINE: u64 = 1 << 20;
/// A token line is 64 hex chars; anything much longer is not one.
const MAX_TOKEN_LINE: u64 = 128;
/// The proxy sends its token right away; a silent connection is dropped.
const TOKEN_TIMEOUT: Duration = Duration::from_secs(5);
const PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

struct Entry {
    caller: Caller,
    config: PathBuf,
    /// A connection took this token; a second one is refused.
    used: bool,
}

struct Inner {
    dir: PathBuf,
    socket: PathBuf,
    grants: Mutex<HashMap<u64, (String, Entry)>>,
    next: AtomicU64,
    stop: AtomicBool,
}

impl Inner {
    fn grants(&self) -> MutexGuard<'_, HashMap<u64, (String, Entry)>> {
        // A panic while holding the map leaves plain data behind, still consistent.
        self.grants.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Listens on `<dir>/mcp.sock` until dropped.
pub struct Server {
    inner: Arc<Inner>,
}

/// One agent session's right to call the tools. Dropping it revokes the token (and cuts its connection) and
/// deletes its config file.
pub struct Grant {
    inner: Arc<Inner>,
    id: u64,
    token: String,
    config: PathBuf,
}

impl Server {
    /// Binds the socket in `dir`, made private (0700) first.
    pub fn start(dir: &Path, host: Arc<dyn Host>) -> Result<Server> {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .with_context(|| format!("making {} private", dir.display()))?;
        let socket = dir.join("mcp.sock");
        let _ = std::fs::remove_file(&socket); // a crashed run's leftover
        let listener = UnixListener::bind(&socket)
            .with_context(|| format!("listening on {}", socket.display()))?;
        let inner = Arc::new(Inner {
            dir: dir.to_owned(),
            socket,
            grants: Mutex::new(HashMap::new()),
            next: AtomicU64::new(0),
            stop: AtomicBool::new(false),
        });
        let accept = inner.clone();
        std::thread::Builder::new()
            .name("skyelab-mcp-accept".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    if accept.stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(stream) = stream else { continue };
                    let (inner, host) = (accept.clone(), host.clone());
                    let _ = std::thread::Builder::new()
                        .name("skyelab-mcp-conn".into())
                        .spawn(move || serve(stream, &inner, host.as_ref()));
                }
            })
            .context("spawning the MCP accept thread")?;
        Ok(Server { inner })
    }

    pub fn socket_path(&self) -> &Path {
        &self.inner.socket
    }

    /// Grants that are still live (their agents haven't ended).
    pub fn active_grants(&self) -> usize {
        self.inner.grants().len()
    }

    /// A new one-use token for `caller`, and a private `--mcp-config` file that runs `proxy` with it.
    pub fn grant(&self, caller: Caller, proxy: &Proxy) -> Result<Grant> {
        let token = random_token()?;
        let id = self.inner.next.fetch_add(1, Ordering::SeqCst);
        let config = self.inner.dir.join(format!("grant-{id}.json"));
        let socket = self
            .inner
            .socket
            .to_str()
            .context("the socket path is not UTF-8")?;
        let json = json!({
            "mcpServers": {
                "skyelab": {
                    "type": "stdio",
                    "command": proxy.program,
                    "args": proxy.args,
                    "env": { SOCKET_ENV: socket, TOKEN_ENV: token },
                }
            }
        });
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&config)
            .with_context(|| format!("creating {}", config.display()))?;
        let text = serde_json::to_string_pretty(&json).context("encoding the MCP config")?;
        file.write_all(text.as_bytes())
            .with_context(|| format!("writing {}", config.display()))?;
        self.inner.grants().insert(
            id,
            (
                token.clone(),
                Entry {
                    caller,
                    config: config.clone(),
                    used: false,
                },
            ),
        );
        Ok(Grant {
            inner: self.inner.clone(),
            id,
            token,
            config,
        })
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.inner.stop.store(true, Ordering::SeqCst);
        let _ = UnixStream::connect(&self.inner.socket); // wakes the accept loop so it sees `stop`
        let _ = std::fs::remove_file(&self.inner.socket);
    }
}

impl Grant {
    pub fn token(&self) -> &str {
        &self.token
    }

    /// The file to pass as `--mcp-config`. Deleted once the agent's proxy connects.
    pub fn config_path(&self) -> &Path {
        &self.config
    }
}

impl Drop for Grant {
    fn drop(&mut self) {
        self.inner.grants().remove(&self.id);
        let _ = std::fs::remove_file(&self.config);
    }
}

fn random_token() -> Result<String> {
    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .context("reading /dev/urandom")?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Equal-time comparison, so response timing doesn't reveal how much of a token matched.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// Takes the grant `token` names, once: marks it used and deletes its config file.
fn claim(inner: &Inner, token: &str) -> Option<(u64, Caller)> {
    let mut grants = inner.grants();
    let (id, (_, entry)) = grants
        .iter_mut()
        .find(|(_, (t, e))| !e.used && same(t, token))?;
    entry.used = true;
    let _ = std::fs::remove_file(&entry.config);
    Some((*id, entry.caller.clone()))
}

/// Reads one line of at most `max` bytes. `None` at EOF, on an error, or for an oversized line.
fn read_line(reader: &mut BufReader<UnixStream>, max: u64) -> Option<String> {
    let mut buf = Vec::new();
    match reader.by_ref().take(max + 1).read_until(b'\n', &mut buf) {
        Ok(0) | Err(_) => None,
        Ok(n) if n as u64 > max => None,
        Ok(_) => String::from_utf8(buf).ok(),
    }
}

fn serve(stream: UnixStream, inner: &Inner, host: &dyn Host) {
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    let _ = stream.set_read_timeout(Some(TOKEN_TIMEOUT));
    let mut reader = BufReader::new(stream);
    let Some(token) = read_line(&mut reader, MAX_TOKEN_LINE) else {
        return;
    };
    let Some((id, caller)) = claim(inner, token.trim_end()) else {
        return; // unknown, used or revoked: say nothing
    };
    let _ = reader.get_ref().set_read_timeout(None);
    // shortcut: one request at a time per connection, so a wait_thread delays the agent's parallel calls; spawn per request if that hurts.
    while let Some(line) = read_line(&mut reader, MAX_LINE) {
        if !inner.grants().contains_key(&id) {
            return; // the agent ended; its connection goes with it
        }
        if line.trim().is_empty() {
            continue;
        }
        let Some(answer) = respond(host, &caller, &line) else {
            continue;
        };
        if writeln!(writer, "{answer}")
            .and_then(|()| writer.flush())
            .is_err()
        {
            return;
        }
    }
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// The answer to one JSON-RPC message, or `None` for a notification.
fn respond(host: &dyn Host, caller: &Caller, line: &str) -> Option<Value> {
    let Ok(msg) = serde_json::from_str::<Value>(line) else {
        return Some(rpc_error(Value::Null, -32700, "parse error"));
    };
    let Some(method) = msg.get("method").and_then(Value::as_str) else {
        return Some(rpc_error(Value::Null, -32600, "invalid request"));
    };
    let id = msg.get("id")?.clone(); // no id: a notification, never answered
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => {
            let asked = params["protocolVersion"].as_str().unwrap_or_default();
            let version = PROTOCOL_VERSIONS
                .iter()
                .find(|v| **v == asked)
                .unwrap_or(&PROTOCOL_VERSIONS[0]);
            json!({
                "protocolVersion": version,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "skyelab", "version": env!("CARGO_PKG_VERSION")},
                "instructions": "Delegate work to other SkyeLab threads. You can only reach projects bound \
                    to your own account.",
            })
        }
        "ping" => json!({}),
        "tools/list" => json!({"tools": tools::list()}),
        "tools/call" => {
            let name = params["name"].as_str().unwrap_or_default();
            let answer = tools::parse(name, &params["arguments"])
                .and_then(|tool| host.call(caller, tool))
                .map(|v| v.to_string());
            let (text, is_error) = match answer {
                Ok(text) => (text, false),
                Err(e) => (e, true),
            };
            json!({"content": [{"type": "text", "text": text}], "isError": is_error})
        }
        _ => return Some(rpc_error(id, -32601, "method not found")),
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}
