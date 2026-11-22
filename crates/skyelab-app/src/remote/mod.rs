//! Sharing this machine on the user's tailnet: the daemon's lifecycle, its status, and who is connected.
//!
//! The daemon (skyelab-daemon) does the gating: tailnet IPs only, whois must be one of the user's own untagged
//! devices, no browsers. This module only turns it on and off, and wires its agent calls to the thread view.

mod agents;
mod panel;

pub(crate) use agents::internal;
pub use agents::{RemoteCall, Reply, UiAgents};
pub use panel::RemotePanel;

use gpui::{Action, AppContext as _, Context, SharedString, Task, actions};
use skyelab_core::AppSettings;
use skyelab_daemon::{Backend, Config, Daemon, Peer, SharedTailnet};
use skyelab_store::Store;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

actions!(skyelab, [OpenRemote, ToggleSharing]);

#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct DisconnectPeer {
    pub id: u64,
}

/// The running daemon, shared with the store's push hook. `None` while not sharing.
pub type DaemonSlot = Arc<Mutex<Option<Daemon>>>;

fn lock_slot(slot: &DaemonSlot) -> std::sync::MutexGuard<'_, Option<Daemon>> {
    // Nothing panics while holding it; a poisoned slot still holds a usable daemon.
    slot.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Every stored event, from local and remote threads alike, goes to connected clients. They could read it with
/// `thread_events_since` anyway; pushing just saves them the poll.
pub fn push_events(store: &mut Store, slot: DaemonSlot) {
    store.on_append(move |e| {
        if let Some(daemon) = &*lock_slot(&slot) {
            daemon.push_event(skyelab_daemon::event(e.clone()));
        }
    });
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Off,
    Starting,
    Sharing { name: String, port: u16 },
    Failed(String),
}

impl Status {
    /// The status bar's text; `None` when there is nothing to say.
    pub fn label(&self) -> Option<SharedString> {
        match self {
            Status::Off => None,
            Status::Starting => Some("Starting sharing…".into()),
            Status::Sharing { name, port } => Some(format!("Sharing on {name}:{port}").into()),
            Status::Failed(e) => Some(format!("Not sharing: {e}").into()),
        }
    }
}

/// What starting the daemon needs. Tests pass a fake tailnet and port 0.
pub struct Host {
    pub settings_path: Option<PathBuf>,
    pub tailnet: SharedTailnet,
    pub config: Config,
    pub backend: Arc<dyn Backend>,
    pub slot: DaemonSlot,
}

pub struct Remote {
    host: Host,
    /// The persisted toggle. On while `status` is `Failed` means "wanted, but it couldn't start".
    pub enabled: bool,
    pub status: Status,
    pub peers: Vec<Peer>,
    /// Bumped on every toggle, so a start that finishes after the user turned sharing off is dropped.
    generation: u64,
    _start: Option<Task<()>>,
    _poll: Option<Task<()>>,
}

/// How often the peer list is re-read while sharing.
const PEER_POLL: Duration = Duration::from_secs(1);

impl Remote {
    /// Reads the toggle and starts sharing if it was left on.
    pub fn new(host: Host, cx: &mut Context<Self>) -> Self {
        let loaded = host.settings_path.as_deref().map(AppSettings::load);
        let mut this = Self {
            host,
            enabled: false,
            status: Status::Off,
            peers: Vec::new(),
            generation: 0,
            _start: None,
            _poll: None,
        };
        match loaded {
            Some(Ok(s)) if s.share_on_tailnet => {
                this.enabled = true;
                this.start(cx);
            }
            // A broken file never turns sharing on.
            Some(Err(e)) => this.status = Status::Failed(format!("{e:#}")),
            _ => {}
        }
        this
    }

    pub fn toggle(&mut self, cx: &mut Context<Self>) {
        self.set_sharing(!self.enabled, cx);
    }

    /// Saves the toggle first: if that fails, nothing changes and the error shows.
    pub fn set_sharing(&mut self, on: bool, cx: &mut Context<Self>) {
        let saved = match &self.host.settings_path {
            // A file that doesn't parse is left alone, and the toggle doesn't move.
            Some(path) => AppSettings::load(path).and_then(|mut s| {
                s.share_on_tailnet = on;
                s.save(path)
            }),
            None => Err(anyhow::anyhow!(
                "no config directory on this OS, so the setting can't be saved"
            )),
        };
        if let Err(e) = saved {
            self.status = Status::Failed(format!("{e:#}"));
            cx.notify();
            return;
        }
        self.enabled = on;
        if on {
            self.start(cx);
        } else {
            self.stop(cx);
        }
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        self.stop(cx);
        self.status = Status::Starting;
        let generation = self.generation;
        let (tailnet, config, backend) = (
            self.host.tailnet.clone(),
            self.host.config.clone(),
            self.host.backend.clone(),
        );
        // self_node and bind shell out to tailscale and can take seconds.
        let started = cx.background_spawn(async move {
            let me = tailnet.self_node()?;
            let name = match me.dns_name.trim_end_matches('.') {
                "" => me.hostname,
                dns => dns.to_owned(),
            };
            let daemon = Daemon::start(config, tailnet, backend)?;
            let port = daemon.local_addrs().first().map_or(0, |a| a.port());
            anyhow::Ok((daemon, name, port))
        });
        self._start = Some(cx.spawn(async move |this, cx| {
            let started = started.await;
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    if let Ok((daemon, ..)) = started {
                        drop_off_thread(daemon);
                    }
                    return;
                }
                this.status = match started {
                    Ok((daemon, name, port)) => {
                        *lock_slot(&this.host.slot) = Some(daemon);
                        this.poll_peers(cx);
                        Status::Sharing { name, port }
                    }
                    Err(e) => Status::Failed(format!("{e:#}")),
                };
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        self._start = None;
        self._poll = None;
        self.peers.clear();
        self.status = Status::Off;
        if let Some(daemon) = lock_slot(&self.host.slot).take() {
            drop_off_thread(daemon);
        }
        cx.notify();
    }

    /// Re-reads who's connected; notifies only on a change.
    pub fn refresh_peers(&mut self, cx: &mut Context<Self>) {
        let peers = lock_slot(&self.host.slot)
            .as_ref()
            .map(Daemon::peers)
            .unwrap_or_default();
        if peers != self.peers {
            self.peers = peers;
            cx.notify();
        }
    }

    pub fn disconnect(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(daemon) = &*lock_slot(&self.host.slot) {
            daemon.disconnect(id);
        }
        self.refresh_peers(cx);
    }

    fn poll_peers(&mut self, cx: &mut Context<Self>) {
        self._poll = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(PEER_POLL).await;
                if this.update(cx, |this, cx| this.refresh_peers(cx)).is_err() {
                    return;
                }
            }
        }));
    }
}

/// Dropping a daemon joins its thread (up to a second), so never on the UI thread.
fn drop_off_thread(daemon: Daemon) {
    let _ = std::thread::Builder::new()
        .name("skyelab-daemon-stop".into())
        .spawn(move || drop(daemon));
}

#[cfg(test)]
mod tests;
