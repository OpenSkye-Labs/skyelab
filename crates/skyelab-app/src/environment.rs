//! A remote environment: another of the user's machines, reached through `skyelab-client`.
//!
//! The client's update channel is unbounded, so `_drain` reads it for as long as this entity lives. Every client
//! call returns a `Pending` that is only ever awaited (`recv_async`), never waited on, so the UI thread never blocks.

use gpui::{Context, EventEmitter, Hsla, SharedString, Task};
use gpui_component::Theme;
use skyelab_client::{Client, Error, Pending, Project, Thread, ThreadEvent, Update};
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnState {
    Connecting,
    Connected,
    /// Lost (or not up yet); the client retries with its backoff. Holds why.
    Reconnecting(String),
    /// The remote's gate (whois) dropped us before the handshake. The client keeps retrying.
    Refused,
    /// Fatal: the client stopped.
    VersionMismatch(String),
    Stopped(String),
}

/// The client dialed fine but the socket closed before the WebSocket handshake: the daemon's gate refused us.
/// Matches skyelab-client's reason text; `env_states_test` drives a real refusal so a change there fails a test.
fn refused(reason: &str) -> bool {
    reason.starts_with("WebSocket handshake failed")
}

impl ConnState {
    fn from_update(update: &Update) -> Option<Self> {
        Some(match update {
            Update::Connected(_) => Self::Connected,
            Update::Disconnected(reason) if refused(reason) => Self::Refused,
            Update::Disconnected(reason) => Self::Reconnecting(reason.clone()),
            Update::Fatal(Error::UnsupportedVersion(why)) => Self::VersionMismatch(why.clone()),
            Update::Fatal(e) => Self::Stopped(e.to_string()),
            Update::Event(_) => return None,
        })
    }

    pub fn label(&self) -> SharedString {
        match self {
            Self::Connecting => "Connecting…".into(),
            Self::Connected => "Connected".into(),
            Self::Reconnecting(why) => format!("Reconnecting: {why}").into(),
            Self::Refused => "That machine didn't accept this device".into(),
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
            Self::Connecting | Self::Reconnecting(_) => theme.warning,
            _ => theme.danger,
        }
    }
}

pub enum EnvEvent {
    Thread(ThreadEvent),
}

pub struct RemoteEnv {
    /// What the user picked: a MagicDNS name or a tailnet IP.
    pub host: String,
    client: Rc<Client>,
    pub state: ConnState,
    pub projects: Vec<Project>,
    pub project: Option<i64>,
    /// The selected project's threads, as the remote lists them.
    pub threads: Vec<Thread>,
    pub error: Option<SharedString>,
    _drain: Task<()>,
    _projects: Option<Task<()>>,
    _threads: Option<Task<()>>,
}

impl EventEmitter<EnvEvent> for RemoteEnv {}

impl RemoteEnv {
    /// Wraps a client made with `Client::connect(host, ..)`, which never blocks on the network.
    pub fn new(host: String, client: Client, cx: &mut Context<Self>) -> Self {
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
            state: ConnState::Connecting,
            projects: Vec::new(),
            project: None,
            threads: Vec::new(),
            error: None,
            _drain,
            _projects: None,
            _threads: None,
        }
    }

    pub fn client(&self) -> Rc<Client> {
        self.client.clone()
    }

    fn on_update(&mut self, update: Update, cx: &mut Context<Self>) {
        if let Update::Event(event) = update {
            return cx.emit(EnvEvent::Thread(event));
        }
        if let Some(state) = ConnState::from_update(&update) {
            let up = state == ConnState::Connected;
            self.state = state;
            if up {
                self.load_projects(cx);
                self.load_threads(cx);
            }
            cx.notify();
        }
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

    pub fn load_threads(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.project else {
            return;
        };
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

/// A machine name or tailnet address someone typed: letters, digits, `.`, `-`, and `:` for IPv6. The client
/// still refuses anything that doesn't resolve into the tailnet; this only catches typos early.
pub fn valid_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && !host.starts_with(['-', '.'])
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_from_updates() {
        let state = |u| ConnState::from_update(&u);
        assert_eq!(
            state(Update::Disconnected(
                "WebSocket handshake failed: IO error: Connection reset by peer".into()
            )),
            Some(ConnState::Refused)
        );
        assert_eq!(
            state(Update::Disconnected("can't connect: refused".into())),
            Some(ConnState::Reconnecting("can't connect: refused".into()))
        );
        assert!(matches!(
            state(Update::Fatal(Error::UnsupportedVersion("2.0".into()))),
            Some(ConnState::VersionMismatch(_))
        ));
    }

    #[test]
    fn typed_hosts() {
        for ok in [
            "laptop.tail1234.ts.net",
            "100.64.0.1",
            "fd7a:115c:a1e0::1",
            "laptop",
        ] {
            assert!(valid_host(ok), "{ok}");
        }
        for bad in [
            "",
            "-oProxy",
            ".x",
            "a b",
            "http://x",
            "x/y",
            &"a".repeat(254),
        ] {
            assert!(!valid_host(bad), "{bad}");
        }
    }
}
