//! The socket listener, the grants (one-use tokens), and the JSON-RPC loop.

use super::{Caller, Host, Proxy, SOCKET_ENV, TOKEN_ENV, tools};
use anyhow::{Context as _, Result};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Longest JSON-RPC line read; a longer one closes the connection.
const MAX_LINE: u64 = 1 << 20;
/// A token line is 64 hex chars; anything much longer is not one.
const MAX_TOKEN_LINE: u64 = 128;
/// The proxy sends its token right away; a silent connection is dropped.
const TOKEN_TIMEOUT: Duration = Duration::from_secs(5);
const PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];
/// sun_path holds 104 bytes on macOS (108 on Linux), the terminating NUL included.
const MAX_SOCKET_PATH: usize = 103;

struct Entry {
    caller: Caller,
    config: PathBuf,
    /// A connection took this token; a second one is refused.
    used: bool,
    /// That connection, shut down when the grant drops.
    conn: Option<UnixStream>,
}

struct Inner {
    dir: PathBuf,
    socket: PathBuf,
    /// The short private dir holding `socket` when `<dir>/mcp.sock` is too long a path; removed on drop.
    socket_dir: Option<PathBuf>,
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
        // Emptied first: a crashed run with the same pid may have left grant files behind.
        match std::fs::remove_dir_all(dir) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                return Err(e).with_context(|| format!("clearing {}", dir.display()));
            }
            _ => {}
        }
        // Private from creation; the chmod after covers a dir that already existed with another mode.
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .with_context(|| format!("creating {}", dir.display()))?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .with_context(|| format!("making {} private", dir.display()))?;
        let (socket, socket_dir) = match dir.join("mcp.sock") {
            s if s.as_os_str().len() <= MAX_SOCKET_PATH => (s, None),
            _ => {
                let short = short_socket_dir(dir)?;
                (short.join("mcp.sock"), Some(short))
            }
        };
        let listener = UnixListener::bind(&socket)
            .with_context(|| format!("listening on {}", socket.display()))?;
        let inner = Arc::new(Inner {
            dir: dir.to_owned(),
            socket,
            socket_dir,
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
                    let Ok(stream) = stream else {
                        // EMFILE and friends: back off instead of spinning.
                        std::thread::sleep(Duration::from_millis(100));
                        continue;
                    };
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
                    conn: None,
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
        if let Some(d) = &self.inner.socket_dir {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}

/// A fresh dir for the socket when the run dir is too deep (a long HOME): under the temp dir, else `/tmp`.
/// Created exclusively with mode 0700, then checked to be a real dir, 0700 and owned like the run dir (ours),
/// so nobody else can have planted it or can reach the socket.
fn short_socket_dir(run_dir: &Path) -> Result<PathBuf> {
    use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _};
    let name = format!("skyelab-{}-{}", std::process::id(), &random_token()?[..16]);
    let tmp = std::env::temp_dir().join(&name);
    let dir = if tmp.join("mcp.sock").as_os_str().len() <= MAX_SOCKET_PATH {
        tmp
    } else {
        Path::new("/tmp").join(name)
    };
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&dir)
        .with_context(|| format!("creating {}", dir.display()))?;
    // A strict umask may have taken the owner's bits too.
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
        .with_context(|| format!("making {} private", dir.display()))?;
    let meta = std::fs::symlink_metadata(&dir).context("checking the socket dir")?;
    let owner = std::fs::metadata(run_dir)
        .context("checking the run dir")?
        .uid();
    if !meta.is_dir() || meta.permissions().mode() & 0o777 != 0o700 || meta.uid() != owner {
        anyhow::bail!("{} is not a private dir of ours", dir.display());
    }
    Ok(dir)
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
        let entry = self.inner.grants().remove(&self.id);
        if let Some(conn) = entry.and_then(|(_, e)| e.conn) {
            let _ = conn.shutdown(std::net::Shutdown::Both);
        }
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
fn claim(inner: &Inner, token: &str, conn: &UnixStream) -> Option<(u64, Caller)> {
    let mut grants = inner.grants();
    let (id, (_, entry)) = grants
        .iter_mut()
        .find(|(_, (t, e))| !e.used && same(t, token))?;
    entry.used = true;
    entry.conn = conn.try_clone().ok();
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

/// The token line, which must arrive whole within [`TOKEN_TIMEOUT`] of connecting, not per read: a client
/// dripping a byte at a time can't hold the connection.
fn read_token(reader: &mut BufReader<UnixStream>) -> Option<String> {
    let deadline = Instant::now() + TOKEN_TIMEOUT;
    let mut buf = Vec::new();
    loop {
        let left = deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())?;
        reader.get_ref().set_read_timeout(Some(left)).ok()?;
        let chunk = reader.fill_buf().ok()?;
        if chunk.is_empty() {
            return None; // EOF
        }
        let end = chunk.iter().position(|b| *b == b'\n');
        let take = end.map_or(chunk.len(), |i| i + 1);
        buf.extend_from_slice(&chunk[..take]);
        reader.consume(take);
        if buf.len() as u64 > MAX_TOKEN_LINE {
            return None;
        }
        if end.is_some() {
            return String::from_utf8(buf).ok();
        }
    }
}

fn serve(stream: UnixStream, inner: &Inner, host: &dyn Host) {
    let mut claimed = None;
    serve_until_closed(stream, inner, host, &mut claimed);
    // The grant holds a clone (to cut the connection); take it back so the socket really closes.
    if let Some(id) = claimed
        && let Some((_, entry)) = inner.grants().get_mut(&id)
    {
        entry.conn = None;
    }
}

fn serve_until_closed(
    stream: UnixStream,
    inner: &Inner,
    host: &dyn Host,
    claimed: &mut Option<u64>,
) {
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(stream);
    let Some(token) = read_token(&mut reader) else {
        return;
    };
    let Some((id, caller)) = claim(inner, token.trim_end(), &writer) else {
        return; // unknown, used or revoked: say nothing
    };
    *claimed = Some(id);
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
        if !inner.grants().contains_key(&id) {
            return; // revoked while the call ran (a long wait_thread): the answer goes nowhere
        }
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
                    to your own account. Drive a SkyeLab browser tab (http and https only) with the browser_ tools.",
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

/// Removes `<root>/<pid>` dirs of SkyeLab runs that are gone: a crash or a kill skips `Drop`, which would leave
/// dead sockets and config files of dead tokens behind.
pub fn remove_stale_runs(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        if pid != std::process::id() && !alive(pid) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// `unsafe` is denied, so no `libc::kill(pid, 0)`: the kill binary asks instead. A reused pid only keeps a dead dir.
fn alive(pid: u32) -> bool {
    std::process::Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .map_or(true, |s| s.success()) // can't ask: keep the dir rather than break a live run
}
