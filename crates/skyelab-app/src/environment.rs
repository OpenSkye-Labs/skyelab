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

pub enum EnvEvent {
    Thread(ThreadEvent),
    /// The name moved to another machine: its tabs show threads of a machine no longer trusted, so they close.
    NodeChanged,
}

pub struct RemoteEnv {
    /// What the user picked: a MagicDNS name.
    pub host: String,
    client: Rc<Client>,
    /// Where each name's trusted node is kept. `None` (no config directory, some tests): no node pinning.
    settings_path: Option<PathBuf>,
    /// The node the current connection presented, `None` while disconnected or from a 1.0 server.
    node: Option<String>,
    /// The node checked and trusted on this connection's predecessors, so a reconnect to it needs no disk read.
    trusted: Option<String>,
    pub state: ConnState,
    pub projects: Vec<Project>,
    pub project: Option<i64>,
    /// The selected project's threads, as the remote lists them.
    pub threads: Vec<Thread>,
    pub error: Option<SharedString>,
    _drain: Task<()>,
    _projects: Option<Task<()>>,
    _threads: Option<Task<()>>,
    _node: Option<Task<()>>,
    _trust: Option<Task<()>>,
    /// Events that came while the connection's node was being checked, delivered once it is trusted.
    held: Vec<ThreadEvent>,
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
        Self {
            host,
            client,
            settings_path,
            node: None,
            trusted: None,
            state: ConnState::Connecting,
            projects: Vec::new(),
            project: None,
            threads: Vec::new(),
            error: None,
            _drain,
            _projects: None,
            _threads: None,
            _node: None,
            _trust: None,
            held: Vec::new(),
        }
    }

    pub fn client(&self) -> Rc<Client> {
        self.client.clone()
    }

    fn on_update(&mut self, update: Update, cx: &mut Context<Self>) {
        match update {
            // Events reach the tabs only from a trusted connection. Those that arrive while it is being
            // checked are held: the client delivers each event once, so dropping them would lose them.
            Update::Event(event) if self.state == ConnState::Connected => {
                cx.emit(EnvEvent::Thread(event))
            }
            Update::Event(event) if self.state == ConnState::Connecting => self.held.push(event),
            Update::Event(_) => {}
            Update::Connected(hello) => self.on_connected(hello, cx),
            update => {
                if let Some(state) = ConnState::from_update(&update) {
                    self.node = None;
                    self.held.clear();
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
        self.node = hello.node_id;
        self.held.clear();
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
                        this.node_changed(ConnState::NodeChanged { was, now }, cx)
                    }
                    Err(e) => this.node_changed(ConnState::Unverified(format!("{e:#}")), cx),
                }
            });
        }));
    }

    /// Refuses this connection: nothing listed, nothing sent, and its tabs close.
    fn node_changed(&mut self, state: ConnState, cx: &mut Context<Self>) {
        self.state = state;
        self.held.clear();
        self.projects.clear();
        self.threads.clear();
        self.project = None;
        self._projects = None;
        self._threads = None;
        cx.emit(EnvEvent::NodeChanged);
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
        for event in std::mem::take(&mut self.held) {
            cx.emit(EnvEvent::Thread(event));
        }
        self.load_projects(cx);
        self.load_threads(cx);
        cx.notify();
    }

    fn load_projects(&mut self, cx: &mut Context<Self>) {
        let pending = self.client.list_projects();
        self._projects = Some(self.await_reply(pending, cx, |this, projects, _| {
            this.projects = projects;
        }));
    }

    pub fn select_project(&mut self, id: i64, cx: &mut Context<Self>) {
        self.project = Some(id);
        self.threads.clear(); // never show another project's threads while this one's load
        self.load_threads(cx);
        cx.notify();
    }

    /// Only from a trusted connection; `up` loads them once it is.
    pub fn load_threads(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.project else {
            return;
        };
        if self.state != ConnState::Connected {
            return;
        }
        let pending = self.client.list_threads(id);
        self._threads = Some(self.await_reply(pending, cx, move |this, mut threads, _| {
            // A slow reply for a project no longer selected changes nothing.
            if this.project == Some(id) {
                threads.sort_by_key(|t| std::cmp::Reverse(t.updated_at));
                this.threads = threads;
            }
        }));
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

#[cfg(test)]
mod tests {
    use super::*;
    use skyelab_client::Disconnect;

    fn gone(kind: DisconnectKind, message: &str) -> Update {
        Update::Disconnected(Disconnect {
            kind,
            message: message.into(),
        })
    }

    #[test]
    fn states_from_updates() {
        let state = |u| ConnState::from_update(&u);
        // Only the client's typed refusal is a refusal, whatever the text says.
        assert_eq!(
            state(gone(DisconnectKind::Refused, "anything")),
            Some(ConnState::Refused)
        );
        let bad_cert = "TLS with x.ts.net failed: invalid peer certificate: NotValidForName";
        assert_eq!(
            state(gone(DisconnectKind::Certificate, bad_cert)),
            Some(ConnState::Reconnecting(bad_cert.into()))
        );
        let reset = "TLS with x.ts.net failed: Connection reset by peer (os error 54)";
        assert_eq!(
            state(gone(DisconnectKind::Other, reset)),
            Some(ConnState::Reconnecting(reset.into()))
        );
        assert_eq!(
            state(gone(
                DisconnectKind::NotApproved,
                "x hasn't approved (code AB12-CD34)"
            )),
            Some(ConnState::NotApproved(
                "x hasn't approved (code AB12-CD34)".into()
            ))
        );
        assert_eq!(
            state(gone(DisconnectKind::Denied, "x turned this device down")),
            Some(ConnState::Denied)
        );
        assert!(matches!(
            state(Update::Fatal(Error::UnsupportedVersion("2.0".into()))),
            Some(ConnState::VersionMismatch(_))
        ));
    }
}
