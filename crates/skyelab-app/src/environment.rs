//! A remote environment: another of the user's machines, reached through `skyelab-client`.
//!
//! The client's update channel is unbounded, so `_drain` reads it for as long as this entity lives. Every client
//! call returns a `Pending` that is only ever awaited (`recv_async`), never waited on, so the UI thread never blocks.

use gpui::{AppContext as _, Context, EventEmitter, Hsla, SharedString, Task};
use gpui_component::Theme;
use skyelab_client::{
    Client, DisconnectKind, Error, Hello, Pending, Project, Thread, ThreadEvent, Update,
};
use skyelab_core::AppSettings;
use std::path::PathBuf;
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnState {
    /// Dialling, or connected while this machine's identity is checked.
    Connecting,
    Connected,
    /// Lost (or not up yet); the client retries with its backoff. Holds why.
    Reconnecting(String),
    /// The remote's gate (whois) dropped us before the handshake. The client keeps retrying.
    Refused,
    /// The remote hasn't approved this device yet; the user is asked there. Holds the client's words, which
    /// name this device's code.
    NotApproved(String),
    /// The user turned this device down on the remote.
    Denied,
    /// This machine's own Tailscale doesn't vouch for the dialled address as one of the user's own untagged devices
    /// (or whois failed). Nothing was sent to it. The client keeps retrying.
    NotYourDevice(String),
    /// The remote's hello named another node than this machine's Tailscale reports for its address. Retrying.
    IdentityMismatch(String),
    /// The name answered from another Tailscale node than the one trusted for it (a reused device name in a shared
    /// tailnet, or a replaced machine). Nothing is listed or sent until the user trusts `now`.
    NodeChanged {
        was: String,
        now: String,
    },
    /// The settings file that holds the trusted nodes can't be read, so this machine can't be checked. Refused.
    Unverified(String),
    /// The remote speaks another protocol. Fatal: the client stops and won't retry.
    VersionMismatch(String),
    /// Fatal for any other reason.
    Stopped(String),
}

impl ConnState {
    /// The state a client update leads to. `Connected` is decided by [`RemoteEnv`] after the node check.
    fn from_update(update: &Update) -> Option<Self> {
        Some(match update {
            Update::Disconnected(d) if d.kind == DisconnectKind::Refused => Self::Refused,
            Update::Disconnected(d) if d.kind == DisconnectKind::NotApproved => {
                Self::NotApproved(d.message.clone())
            }
            Update::Disconnected(d) if d.kind == DisconnectKind::Denied => Self::Denied,
            Update::Disconnected(d) if d.kind == DisconnectKind::NotYourDevice => {
                Self::NotYourDevice(d.message.clone())
            }
            Update::Disconnected(d) if d.kind == DisconnectKind::IdentityMismatch => {
                Self::IdentityMismatch(d.message.clone())
            }
            Update::Disconnected(d) => Self::Reconnecting(d.message.clone()),
            Update::Fatal(Error::UnsupportedVersion(why)) => Self::VersionMismatch(why.clone()),
            Update::Fatal(e) => Self::Stopped(e.to_string()),
            Update::Connected(_) | Update::Event(_) => return None,
        })
    }

    pub fn label(&self) -> SharedString {
        match self {
            Self::Connecting => "Connecting…".into(),
            Self::Connected => "Connected".into(),
            Self::Reconnecting(why) => format!("Reconnecting: {why}").into(),
            Self::Refused => "That machine didn't accept this device".into(),
            Self::NotApproved(why) => format!("Waiting for approval: {why}").into(),
            Self::Denied => "That machine denied this device. It can ask again after sharing restarts there."
                .into(),
            Self::NotYourDevice(why) => format!(
                "Not connected: your Tailscale can't confirm that machine is one of your own devices ({why})"
            )
            .into(),
            Self::IdentityMismatch(why) => format!(
                "Not connected: that machine's identity doesn't match what your Tailscale reports ({why})"
            )
            .into(),
            Self::NodeChanged { was, now } => format!(
                "This name now answers from a different machine (node {now}, trusted: {was}). \
                 Trust it only if you replaced or renamed that machine."
            )
            .into(),
            Self::Unverified(why) => format!(
                "Can't check which machine this is ({why}). Fix or remove settings.toml, then switch again."
            )
            .into(),
            Self::VersionMismatch(why) => format!(
                "Version mismatch: {why}. Update SkyeLab so both machines run the same version."
            )
            .into(),
            Self::Stopped(why) => format!("Disconnected: {why}").into(),
        }
    }

    pub fn color(&self, theme: &Theme) -> Hsla {
        match self {
            Self::Connected => theme.success,
            Self::Connecting | Self::Reconnecting(_) | Self::NotApproved(_) => theme.warning,
            _ => theme.danger,
        }
    }
}

/// Checks each dialled environment with this machine's own Tailscale (whois), so the node pinned per name is the
/// one the local tailnet sees at that address, never only what the server says about itself.
fn peer_verifier(tailnet: skyelab_daemon::SharedTailnet) -> skyelab_client::PeerVerifier {
    skyelab_client::PeerVerifier::new(move |peer| skyelab_tailnet::verify_own_peer(&*tailnet, peer))
}

/// The desktop's client options: every connection checked with `tailnet`. Without a verifier the pin is only the
/// server's word, so the app never builds them any other way.
pub fn client_options(tailnet: skyelab_daemon::SharedTailnet) -> skyelab_client::Options {
    skyelab_client::Options::default().with_verify_peer(peer_verifier(tailnet))
}

pub enum EnvEvent {
    Thread(ThreadEvent),
    /// A message for a toast.
    Note(SharedString),
    /// The connection was refused or its identity failed: its tabs show threads of a machine not trusted now.
    Untrusted,
}

/// How long after a pushed event a machine older than 1.7 (no shell stream) is listed again.
const REFRESH_AFTER: std::time::Duration = std::time::Duration::from_secs(1);

pub struct RemoteEnv {
    /// What the user picked: a MagicDNS name.
    pub host: String,
    client: Rc<Client>,
    /// Where each name's trusted node is kept. `None` (no config directory, some tests): no node pinning.
    settings_path: Option<PathBuf>,
    /// The node the current connection presented, `None` while disconnected or from a 1.0 server.
    node: Option<String>,
    /// The current connection's hello. The client holds the connection until `up` approves it.
    hello: Option<Hello>,
    /// The node checked and trusted on this connection's predecessors, so a reconnect to it needs no disk read.
    trusted: Option<String>,
    pub state: ConnState,
    pub projects: Vec<Project>,
    /// Where New thread starts: the project picked last, else the first one.
    pub project: Option<i64>,
    /// Every project's threads, newest first, as the remote lists them.
    pub threads: Vec<Thread>,
    pub error: Option<SharedString>,
    _drain: Task<()>,
    _projects: Option<Task<()>>,
    _threads: Option<Task<()>>,
    /// A thread listing due after pushed events (status and previews change as turns run). 1.6 machines only.
    _refresh: Option<Task<()>>,
    /// The shell stream (1.7): the snapshot, then every row change as it happens.
    _shell: Option<Task<()>>,
    _node: Option<Task<()>>,
    _trust: Option<Task<()>>,
    /// Whether the rows shown came from this run's connections (else they are the cached ones, or none).
    live: bool,
    /// Reading the cached rows at launch (`environment_cache.rs`), and the save due after live changes.
    _cache: Option<Task<()>>,
    _persist: Option<Task<()>>,
    /// Ports of this machine forwarded to this one.
    pub forwards: Vec<skyelab_client::Forward>,
    pending_forwards: std::collections::HashMap<u16, Task<()>>,
}

impl EventEmitter<EnvEvent> for RemoteEnv {}

impl RemoteEnv {
    /// Wraps a client made with `Client::connect(host, ..)`, which never blocks on the network.
    pub fn new(
        host: String,
        client: Client,
        settings_path: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Self {
        let client = Rc::new(client);
        let updates = client.updates().clone();
        let _drain = cx.spawn(async move |this, cx| {
            while let Ok(update) = updates.recv_async().await {
                if this
                    .update(cx, |this, cx| this.on_update(update, cx))
                    .is_err()
                {
                    return; // dropped: the client goes with it
                }
            }
        });
        let mut env = Self {
            host,
            client,
            settings_path,
            node: None,
            hello: None,
            trusted: None,
            state: ConnState::Connecting,
            projects: Vec::new(),
            project: None,
            threads: Vec::new(),
            error: None,
            _drain,
            _projects: None,
            _threads: None,
            _refresh: None,
            _shell: None,
            _node: None,
            _trust: None,
            live: false,
            _cache: None,
            _persist: None,
            forwards: Vec::new(),
            pending_forwards: Default::default(),
        };
        env.load_cache(cx);
        env
    }

    pub fn client(&self) -> Rc<Client> {
        self.client.clone()
    }

    /// The node the current connection presented and this machine trusts, `None` while disconnected.
    pub fn node(&self) -> Option<&str> {
        self.node
            .as_deref()
            .filter(|_| self.state == ConnState::Connected)
    }

    fn on_update(&mut self, update: Update, cx: &mut Context<Self>) {
        match update {
            // The client delivers events only once `up` approved the connection; this guards the
            // ones still queued from a connection that has since been refused.
            Update::Event(event) if self.state == ConnState::Connected => {
                self.refresh_soon(cx);
                cx.emit(EnvEvent::Thread(event))
            }
            Update::Event(_) => {}
            Update::Connected(hello) => self.on_connected(hello, cx),
            update => {
                // Whoever answered wasn't proven to be this machine (a certificate for another name included).
                let untrusted = matches!(
                    &update,
                    Update::Disconnected(d) if matches!(
                        d.kind,
                        DisconnectKind::Refused
                            | DisconnectKind::Denied
                            | DisconnectKind::Certificate
                            | DisconnectKind::NotYourDevice
                            | DisconnectKind::IdentityMismatch
                    )
                );
                if let Some(state) = ConnState::from_update(&update) {
                    self.node = None;
                    self.hello = None;
                    self.drop_forwards();
                    if untrusted {
                        return self.refuse(state, cx);
                    }
                    self.state = state;
                    cx.notify();
                }
            }
        }
    }

    /// The node the live connection presented. `None` while disconnected or when the server sent no id.
    pub fn presented_node(&self) -> Option<&str> {
        self.node.as_deref()
    }

    /// A connection passed hello. It counts as connected once its node is the one trusted for this name: the
    /// first node a name answers from is trusted (and saved), a different one later is refused until trusted.
    /// A server that sends no node id connects only to a name never pinned, so leaving the id out can't get
    /// past a pin. Settings that can't be read refuse the connection: nothing to check against.
    fn on_connected(&mut self, hello: Hello, cx: &mut Context<Self>) {
        self.node = hello.node_id.clone();
        self.hello = Some(hello);
        let Some(path) = self.settings_path.clone() else {
            return self.up(cx); // no config directory: nowhere to pin
        };
        if self.node.is_some() && self.trusted == self.node {
            return self.up(cx);
        }
        self.state = ConnState::Connecting;
        cx.notify();
        let host = self.host.clone();
        let node = self.node.clone();
        let checking = node.clone();
        let check = cx.background_spawn(async move {
            let (_, was) = AppSettings::update(&path, |s| match s.environment_node(&host) {
                Some(was) if Some(was) != checking.as_deref() => Some(was.to_owned()),
                Some(_) => None,
                None => {
                    if let Some(id) = &checking {
                        s.trust_environment_node(&host, id);
                    }
                    None
                }
            })?;
            anyhow::Ok(was)
        });
        self._node = Some(cx.spawn(async move |this, cx| {
            let checked = check.await;
            let _ = this.update(cx, |this, cx| {
                // The connection this check was for has gone (or another node answered since).
                if this.node != node || this.state != ConnState::Connecting {
                    return;
                }
                match checked {
                    Ok(None) => this.up(cx),
                    Ok(Some(was)) => {
                        let now = node.unwrap_or_else(|| "none (it sent no node id)".into());
                        this.refuse(ConnState::NodeChanged { was, now }, cx)
                    }
                    Err(e) => this.refuse(ConnState::Unverified(format!("{e:#}")), cx),
                }
            });
        }));
    }

    /// Clears what the window still shows (and keeps on disk) from this machine and closes its tabs. A refused
    /// connection that did pass hello stays held by the client, so nothing is sent on it.
    fn refuse(&mut self, state: ConnState, cx: &mut Context<Self>) {
        self.state = state;
        self.drop_cache(cx);
        self.drop_forwards();
        self.projects.clear();
        self.threads.clear();
        self.project = None;
        self._projects = None;
        self._threads = None;
        self._refresh = None;
        self._shell = None;
        cx.emit(EnvEvent::Untrusted);
        cx.notify();
    }

    /// Trusts `node_id` for this name, if it is still the node shown in the warning and still connected. Saved
    /// first; a failed save trusts nothing.
    pub fn trust(&mut self, node_id: &str, cx: &mut Context<Self>) {
        let shown = matches!(&self.state, ConnState::NodeChanged { now, .. } if now == node_id);
        if !shown || self.node.as_deref() != Some(node_id) {
            self.error = Some("That machine changed again. Check the new warning.".into());
            return cx.notify();
        }
        let Some(path) = self.settings_path.clone() else {
            return;
        };
        let (host, node) = (self.host.clone(), node_id.to_owned());
        let save = cx.background_spawn(async move {
            AppSettings::update(&path, |s| s.trust_environment_node(&host, &node)).map(drop)
        });
        let node = node_id.to_owned();
        // Its own task: a reconnect starting a new check must not cancel the save.
        self._trust = Some(cx.spawn(async move |this, cx| {
            let saved = save.await;
            let _ = this.update(cx, |this, cx| match saved {
                Err(e) => {
                    this.error = Some(format!("couldn't trust that machine: {e:#}").into());
                    cx.notify();
                }
                // Still the refused node the user trusted, on a live connection.
                Ok(())
                    if this.node.as_ref() == Some(&node)
                        && matches!(this.state, ConnState::NodeChanged { .. }) =>
                {
                    this.error = None;
                    this.up(cx);
                }
                Ok(()) => this.trusted = Some(node),
            });
        }));
    }

    fn up(&mut self, cx: &mut Context<Self>) {
        self.trusted = self.node.clone();
        self.state = ConnState::Connected;
        if let Some(hello) = &self.hello {
            self.client.approve(hello);
        }
        // Every approved connection subscribes anew: its snapshot replays whatever changed while it was down.
        match self.streams() {
            true => self.subscribe_shell(cx),
            false => self.load_projects(cx),
        }
        cx.notify();
    }

    /// Whether the connected machine speaks protocol 1.`minor` or later (a call it doesn't know would fail).
    pub fn speaks(&self, minor: u32) -> bool {
        self.hello.as_ref().is_some_and(|h| {
            h.protocol.major == skyelab_protocol::VERSION.major && h.protocol.minor >= minor
        })
    }

    /// Awaits `pending` off the UI thread's critical path, then applies it (or shows its error).
    fn await_reply<T: 'static>(
        &self,
        pending: Pending<T>,
        cx: &mut Context<Self>,
        apply: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
    ) -> Task<()> {
        cx.spawn(async move |this, cx| {
            let result = pending.recv_async().await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(value) => {
                        this.error = None;
                        apply(this, value, cx);
                    }
                    Err(e) => this.error = Some(e.to_string().into()),
                }
                cx.notify();
            });
        })
    }
}

#[path = "environment_list.rs"]
mod list;

#[path = "environment_cache.rs"]
mod cache;
pub use cache::{ShellCacheDir, forget_cache};

#[path = "environment_shell.rs"]
mod shell;

#[path = "environment_forwards.rs"]
mod forwards;
pub use forwards::{forwarded_address, forwarded_url};

#[cfg(test)]
#[path = "environment_tests.rs"]
mod tests;
