//! The client side of SkyeLab's remote protocol: a WebSocket connection to a `skyelab-daemon` on the user's tailnet.
//!
//! Tokio runs on the client's own thread. The public API is sync: each RPC returns a [`Pending`] (block with
//! [`Pending::recv`], or await [`Pending::recv_async`] on any executor, GPUI's included), and connection state and
//! pushed events arrive on [`Client::updates`]. The client reconnects by itself with backoff and keeps the
//! connection alive with pings.
//!
//! Server trust, both required: the host (a MagicDNS name) is resolved first and only addresses inside Tailscale's
//! ranges are dialed ([`is_tailnet_ip`]), so a hostile DNS (the VPN off) can't point the client at a public or LAN
//! server; then the connection is `wss://` only, and the server's certificate must chain to the Mozilla roots and
//! name exactly the host dialled. The second check is what stops another carrier-grade-NAT subscriber, whose 100.x
//! address passes the first. There is no plaintext mode outside `cargo test`.
//!
//! Events: only watched threads ([`Client::watch`]) are delivered, each event exactly once and in seq order. A gap
//! in the pushed stream, a reconnect, or a server that dropped us for lagging is filled from the store with
//! `thread_events_since`, page by page.

mod conn;
mod device_key;
mod guard;
mod options;
pub mod release;
#[cfg(test)]
mod release_test;
#[cfg(any(test, feature = "test-support"))]
pub mod testing;
#[cfg(test)]
mod tests;
mod tls;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

pub use conn::forward::Forward;
pub use device_key::{LazyDeviceKey, load_or_create_device_key};
#[cfg(target_os = "macos")]
pub use device_key::{keychain_device_key, keychain_device_key_in};
pub use guard::is_tailnet_ip;
pub use options::{Options, PeerVerifier};
use skyelab_protocol::{Call, Reply};
pub use skyelab_protocol::{
    ErrorCode, Hello, PermissionAnswer, Project, RpcError, ShellChange, ShellDelta, ShellSnapshot,
    Thread, ThreadEvent, Version, is_magicdns_name,
};

/// [`Client::subscribe_shell`]'s answer: the environment's projects and threads, then each change after them, in
/// seq order. `changes` ends when the connection drops or a change was missed: subscribe again for a new snapshot.
pub struct ShellFeed {
    pub snapshot: ShellSnapshot,
    pub changes: flume::Receiver<ShellChange>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("not connected to the environment")]
    NotConnected,
    /// The connection dropped before the reply. The request is never re-sent: it may have run.
    #[error("the connection dropped before the reply")]
    ConnectionLost,
    #[error("no reply in time")]
    Timeout,
    #[error("{message} ({code:?})", code = .0.code, message = .0.message)]
    Rpc(RpcError),
    /// Fatal: the client stops and doesn't reconnect.
    #[error("the environment speaks another protocol version: {0}")]
    UnsupportedVersion(String),
    #[error("the environment sent a reply of the wrong kind")]
    UnexpectedReply,
    #[error("the client has stopped")]
    Stopped,
    /// Fatal: this install's device key couldn't be read (on macOS, the keychain prompt was denied). A new client
    /// tries again.
    #[error("couldn't read this device's key, so no machine can approve it: {0}")]
    DeviceKey(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Update {
    /// A connection passed hello. Watched threads are being resynced, or with
    /// [`Options::approve_each_connection`], will be once [`Client::approve`] approves this hello.
    Connected(Hello),
    /// The connection is gone (or never came up), with why. The client retries after a backoff.
    Disconnected(Disconnect),
    Event(ThreadEvent),
    /// The client stopped for good.
    Fatal(Error),
}

/// Why a connection went away or never came up. Match on `kind`; `message` is for people (and is `Display`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disconnect {
    pub kind: DisconnectKind,
    pub message: String,
}

impl std::fmt::Display for Disconnect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Add a `_` arm when matching: new kinds may come.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DisconnectKind {
    /// TCP connected, then the server hung up before sending a single byte: its gate (whois) turned this device
    /// away. A daemon that is too busy to check us, or whose whois call failed, hangs up the same way.
    Refused,
    /// The server's certificate didn't verify (chain, expiry, or not exactly the dialled name).
    Certificate,
    /// The machine hasn't approved this device's key yet; the user is asked there, and the client keeps trying
    /// every few seconds. The message names the code that machine shows.
    NotApproved,
    /// The user turned this device down on that machine. The client keeps trying, slowly.
    Denied,
    /// The [`Options::verify_peer`] check refused the dialled address: this machine's Tailscale says it isn't one of
    /// the user's own untagged devices, or couldn't say (whois failed or timed out). Nothing was sent to it.
    NotYourDevice,
    /// The hello named another node than this machine's Tailscale reports for the dialled address.
    IdentityMismatch,
    /// Anything else: resolving, dialling, timeouts, a dropped connection, a malformed reply.
    Other,
}

/// One [`Client::watch`]. Pass it to [`Client::unwatch`] to end that watch, and only that one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[must_use = "unwatch with this id, or the thread stays watched"]
pub struct WatchId {
    pub thread_id: i64,
    n: u64,
}

/// The reply to one request.
#[must_use = "a request's result arrives here"]
pub struct Pending<T>(flume::Receiver<Result<T, Error>>);

impl<T> Pending<T> {
    pub fn recv(self) -> Result<T, Error> {
        self.0.recv().unwrap_or(Err(Error::Stopped))
    }

    pub fn recv_timeout(self, timeout: Duration) -> Result<T, Error> {
        match self.0.recv_timeout(timeout) {
            Ok(result) => result,
            Err(flume::RecvTimeoutError::Timeout) => Err(Error::Timeout),
            Err(flume::RecvTimeoutError::Disconnected) => Err(Error::Stopped),
        }
    }

    pub async fn recv_async(self) -> Result<T, Error> {
        self.0.recv_async().await.unwrap_or(Err(Error::Stopped))
    }
}

/// A connection to one environment. Dropping it closes the connection; the thread exits on its own.
///
/// A request made while disconnected fails with [`Error::NotConnected`] (one made just as a connection comes up
/// may go out on it instead).
pub struct Client {
    commands: flume::Sender<conn::Command>,
    /// Set (under its lock) once the connection task has ended and failed what was left in `commands`.
    stopped: Arc<Mutex<bool>>,
    updates: flume::Receiver<Update>,
    hello: Arc<Mutex<Option<Hello>>>,
    next_watch: AtomicU64,
    device_key: Option<LazyDeviceKey>,
    /// The MagicDNS name dialled, for errors that name the machine.
    host: String,
}

impl Client {
    /// Starts connecting to `host`, the environment's MagicDNS name (`machine.tailnet.ts.net`), in the background.
    /// Never blocks on the network. An IP address is refused: a certificate can't vouch for one.
    pub fn connect(host: impl Into<String>, options: Options) -> std::io::Result<Self> {
        let invalid = |msg: String| std::io::Error::new(std::io::ErrorKind::InvalidInput, msg);
        if options.ping_interval.is_zero() || options.backoff_min.is_zero() {
            return Err(invalid(
                "ping_interval and backoff_min must be above zero".into(),
            ));
        }
        // MagicDNS names are lowercase; one trailing dot names the same host.
        let host = host.into();
        let host = host.strip_suffix('.').unwrap_or(&host).to_ascii_lowercase();
        let is_dns_name = !host.ends_with('.')
            && matches!(
                rustls::pki_types::ServerName::try_from(host.as_str()),
                Ok(rustls::pki_types::ServerName::DnsName(_))
            );
        if !is_dns_name {
            return Err(invalid(format!(
                "{host:?} isn't a MagicDNS name; connect by name (machine.tailnet.ts.net) so TLS can check the server"
            )));
        }
        #[cfg(any(test, feature = "test-support"))]
        let roots = options.roots.clone().unwrap_or_else(tls::webpki_roots);
        #[cfg(not(any(test, feature = "test-support")))]
        let roots = tls::webpki_roots();
        let tls = tls::config(roots)?;
        let device_key = options.device_key.clone();
        let (commands, commands_rx) = flume::unbounded();
        let (updates_tx, updates) = flume::unbounded();
        let hello = Arc::new(Mutex::new(None));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let shared_hello = hello.clone();
        let name = host.clone();
        let task_commands = commands.downgrade();
        let stopped = Arc::new(Mutex::new(false));
        let task_stopped = stopped.clone();
        std::thread::Builder::new()
            .name("skyelab-client".into())
            .spawn(move || {
                runtime.block_on(conn::run(
                    host,
                    options,
                    tls,
                    commands_rx.clone(),
                    task_commands,
                    updates_tx,
                    shared_hello,
                ));
                // A request queued as the task ended would wait forever: nobody reads it, and the client's sender
                // keeps it alive. Fail those, and every later one (`command` checks the flag under this lock).
                let mut stopped = task_stopped.lock().unwrap_or_else(PoisonError::into_inner);
                *stopped = true;
                commands_rx.drain().for_each(fail_stopped);
                drop(stopped);
                // A DNS lookup in flight can't be cancelled; don't wait for it.
                runtime.shutdown_background();
            })?;
        Ok(Self {
            commands,
            stopped,
            updates,
            hello,
            next_watch: AtomicU64::new(1),
            device_key,
            host: name,
        })
    }

    /// The code the machine dialled shows for this device's approval request. `None` without a key, or before the
    /// first connection loaded it.
    pub fn device_code(&self) -> Option<String> {
        self.device_key
            .as_ref()?
            .peek()
            .as_deref()
            .map(device_key::code)
    }

    /// Connection changes and watched threads' events, in order.
    pub fn updates(&self) -> &flume::Receiver<Update> {
        &self.updates
    }

    /// The server's hello on the current connection, `None` while disconnected.
    pub fn hello(&self) -> Option<Hello> {
        self.hello
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn list_projects(&self) -> Pending<Vec<Project>> {
        self.call(Call::ListProjects, |r| match r {
            Reply::Projects(p) => Some(p),
            _ => None,
        })
    }

    pub fn list_threads(&self, project_id: i64) -> Pending<Vec<Thread>> {
        self.call(Call::ListThreads { project_id }, |r| match r {
            Reply::Threads(t) => Some(t),
            _ => None,
        })
    }

    /// One page of events after `seq`. To follow a thread, use [`Client::watch`], which pages and dedupes for you.
    pub fn thread_events_since(&self, thread_id: i64, seq: i64) -> Pending<Vec<ThreadEvent>> {
        self.call(Call::ThreadEventsSince { thread_id, seq }, |r| match r {
            Reply::Events(e) => Some(e),
            _ => None,
        })
    }

    /// Returns the new thread's id.
    pub fn start_thread(&self, project_id: i64, prompt: impl Into<String>) -> Pending<i64> {
        let prompt = prompt.into();
        self.call(Call::StartThread { project_id, prompt }, |r| match r {
            Reply::ThreadStarted { thread_id } => Some(thread_id),
            _ => None,
        })
    }

    pub fn send_message(&self, thread_id: i64, text: impl Into<String>) -> Pending<()> {
        let text = text.into();
        self.call(Call::SendMessage { thread_id, text }, ok)
    }

    pub fn interrupt(&self, thread_id: i64) -> Pending<()> {
        self.call(Call::Interrupt { thread_id }, ok)
    }

    /// Answers a permission request the thread's agent waits on (protocol 1.3).
    pub fn answer_permission(
        &self,
        thread_id: i64,
        request_id: impl Into<String>,
        decision: PermissionAnswer,
    ) -> Pending<()> {
        let request_id = request_id.into();
        self.call(
            Call::AnswerPermission {
                thread_id,
                request_id,
                decision,
            },
            ok,
        )
    }

    /// Answers an AskUserQuestion the thread's agent waits on (protocol 1.4): one list of labels per question.
    pub fn answer_questions(
        &self,
        thread_id: i64,
        request_id: impl Into<String>,
        answers: Vec<Vec<String>>,
    ) -> Pending<()> {
        let request_id = request_id.into();
        self.call(
            Call::AnswerQuestions {
                thread_id,
                request_id,
                answers,
            },
            ok,
        )
    }

    /// Puts the thread on a shelf on the environment (protocol 1.6): settled, snoozed until `snoozed_until` (Unix
    /// ms), or neither. An environment older than 1.6 is never asked: it fails here with a `NotWired` error that
    /// says to update SkyeLab there.
    pub fn shelve_thread(
        &self,
        thread_id: i64,
        settled: bool,
        snoozed_until: Option<i64>,
    ) -> Pending<()> {
        if let Some(refused) =
            self.older_than(skyelab_protocol::SHELVES_SINCE_MINOR, "settle and snooze")
        {
            return refused;
        }
        let call = Call::ShelveThread {
            thread_id,
            settled,
            snoozed_until,
        };
        self.call(call, ok)
    }

    /// The environment's projects and threads, then every change to them (protocol 1.7), so a list never polls.
    /// One stream per client: the newest subscribe wins, an older one's changes end at once. An environment older
    /// than 1.7 is never asked (checked on the connection the call goes out on): it fails with a `NotWired` error;
    /// list it with `list_projects` and `list_threads` instead.
    pub fn subscribe_shell(&self) -> Pending<ShellFeed> {
        let (reply, rx) = flume::bounded(1);
        self.command(conn::Command::SubscribeShell { reply });
        Pending(rx)
    }

    /// Delegates to the environment (protocol 1.8): `caller` is the local thread whose agent asks. An environment
    /// older than 1.8 is never asked: it fails here with a `NotWired` error that says to update SkyeLab there.
    pub fn delegate(
        &self,
        caller: skyelab_protocol::Delegator,
        op: skyelab_protocol::DelegateOp,
    ) -> Pending<serde_json::Value> {
        if let Some(refused) =
            self.older_than(skyelab_protocol::DELEGATION_SINCE_MINOR, "delegation")
        {
            return refused;
        }
        self.call(Call::Delegate { caller, op }, |r| match r {
            Reply::Delegated(v) => Some(v),
            _ => None,
        })
    }

    /// A `NotWired` answer for a call the connected environment is older than `minor` for, naming what it lacks.
    fn older_than<T>(&self, minor: u32, lacks: &str) -> Option<Pending<T>> {
        if !self.hello().is_some_and(|h| h.protocol.minor < minor) {
            return None;
        }
        let (tx, rx) = flume::bounded(1);
        let _ = tx.send(Err(too_old(&self.host, lacks)));
        Some(Pending(rx))
    }

    /// Forwards `port` on the environment's loopback to a port on this machine's 127.0.0.1 (protocol 1.5): the same
    /// port when it is free here, else any free one ([`Forward::local_port`]). Each connection to it goes to the
    /// environment over a new connection through its full gate. Fails when the environment refuses the port (a
    /// system or denylisted port, or nothing listens there), when it is older than 1.5, or when this client already
    /// forwards that port. The forward lasts until dropped or until this connection drops.
    pub fn forward_port(&self, port: u16) -> Pending<Forward> {
        let (reply, rx) = flume::bounded(1);
        self.command(conn::Command::Forward { port, reply });
        Pending(rx)
    }

    /// Delivers the thread's events after `after_seq` on [`Client::updates`], then every new one, each once, in
    /// order, across reconnects. Watching a thread again (a reopened tab) replays from the new `after_seq`, so a
    /// watcher already following it sees those events again and must drop them by seq.
    ///
    /// The thread stays watched while any of its watches is live: a late [`Client::unwatch`] of an old watch
    /// never ends a newer one.
    pub fn watch(&self, thread_id: i64, after_seq: i64) -> WatchId {
        let id = WatchId {
            thread_id,
            n: self.next_watch.fetch_add(1, Ordering::Relaxed),
        };
        self.command(conn::Command::Watch {
            id,
            after_seq: after_seq.max(0), // seqs start at 1
        });
        id
    }

    /// Releases a connection held by [`Options::approve_each_connection`], if its hello is still `hello`. An
    /// approval for a connection that has gone applies to its successor only when that one sent the same hello,
    /// so the caller's decision (made from the hello alone) holds for it too.
    pub fn approve(&self, hello: &Hello) {
        self.command(conn::Command::Approve(hello.clone()));
    }

    /// Ends one watch. Unwatching the same id twice does nothing.
    pub fn unwatch(&self, id: WatchId) {
        self.command(conn::Command::Unwatch(id));
    }

    fn call<T: Send + 'static>(
        &self,
        call: Call,
        pick: impl FnOnce(Reply) -> Option<T> + Send + 'static,
    ) -> Pending<T> {
        let (tx, rx) = flume::bounded(1);
        let reply: conn::ReplyFn = Box::new(move |result| {
            let result = result.and_then(|r| pick(r).ok_or(Error::UnexpectedReply));
            let _ = tx.send(result); // the caller may have dropped its Pending
        });
        self.command(conn::Command::Call { call, reply });
        Pending(rx)
    }

    fn command(&self, command: conn::Command) {
        // The connection task has stopped (a fatal error): fail the request right away.
        let stopped = self.stopped.lock().unwrap_or_else(PoisonError::into_inner);
        if *stopped {
            fail_stopped(command);
        } else if let Err(flume::SendError(command)) = self.commands.send(command) {
            fail_stopped(command);
        }
    }
}

fn fail_stopped(command: conn::Command) {
    match command {
        conn::Command::Call { reply, .. } => reply(Err(Error::Stopped)),
        conn::Command::Forward { reply, .. } => {
            let _ = reply.send(Err(Error::Stopped));
        }
        conn::Command::SubscribeShell { reply } => {
            let _ = reply.send(Err(Error::Stopped));
        }
        _ => {}
    }
}

/// The `NotWired` error for a call `host` runs too old a SkyeLab for, naming what it lacks.
fn too_old(host: &str, lacks: &str) -> Error {
    let message = format!("{host} runs an older SkyeLab without {lacks}: update SkyeLab on {host}");
    Error::Rpc(RpcError::new(ErrorCode::NotWired, message))
}

fn ok(reply: Reply) -> Option<()> {
    matches!(reply, Reply::Ok).then_some(())
}
