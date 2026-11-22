//! Sharing this machine on the user's tailnet. The daemon does all the gating; this only turns it on and off.

mod agents;
mod panel;

pub(crate) use agents::internal;
pub use agents::{RemoteCall, Reply, UiAgents};
pub use panel::RemotePanel;

use gpui::{Action, AppContext as _, Context, Hsla, SharedString, Task, actions};
use gpui_component::Theme;
use skyelab_core::AppSettings;
use skyelab_daemon::{Backend, Config, Daemon, Peer, SharedTailnet};
use skyelab_store::Store;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

actions!(skyelab, [OpenRemote, ToggleSharing]);

#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct DisconnectPeer {
    pub id: u64,
}

pub type DaemonSlot = Arc<Mutex<Option<Daemon>>>;

fn lock_slot(slot: &DaemonSlot) -> std::sync::MutexGuard<'_, Option<Daemon>> {
    lock_unpoisoned(slot)
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
    /// `name` is the MagicDNS name clients dial; the TLS certificate is for it and valid until `cert_until`.
    Sharing {
        name: String,
        port: u16,
        cert_until: String,
    },
    Failed(String),
}

impl Status {
    pub fn label(&self) -> SharedString {
        match self {
            Status::Off => "Not sharing".into(),
            Status::Starting => "Starting sharing…".into(),
            Status::Sharing {
                name,
                port,
                cert_until,
            } => format!("Sharing on {name}:{port} · certificate valid until {cert_until}").into(),
            Status::Failed(e) => format!("Not sharing: {e}").into(),
        }
    }

    pub fn color(&self, theme: &Theme) -> Hsla {
        match self {
            Status::Sharing { .. } => theme.success,
            Status::Failed(_) => theme.danger,
            _ => theme.muted_foreground,
        }
    }
}

pub struct Host {
    pub settings_path: Option<PathBuf>,
    pub tailnet: SharedTailnet,
    pub config: Config,
    pub backend: Arc<dyn Backend>,
    pub slot: DaemonSlot,
}

pub struct Remote {
    host: Host,
    /// The persisted toggle. On with `Failed` means the start failed; off with `Failed` means saving failed.
    pub enabled: bool,
    pub status: Status,
    pub peers: Vec<Peer>,
    /// Bumped on every stop, so a start that finishes after it drops its daemon instead of sharing.
    generation: Arc<AtomicU64>,
    /// Held while binding and while dropping a daemon, so a quick off→on never finds port 7287 still taken.
    bind: Arc<Mutex<()>>,
    _start: Option<Task<()>>,
    _poll: Option<Task<()>>,
}

const PEER_POLL: Duration = Duration::from_secs(1);

impl Remote {
    pub fn new(host: Host, cx: &mut Context<Self>) -> Self {
        let loaded = host.settings_path.as_deref().map(AppSettings::load);
        let mut this = Self {
            host,
            enabled: false,
            status: Status::Off,
            peers: Vec::new(),
            generation: Arc::default(),
            bind: Arc::default(),
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

    /// Fails safe: if the choice can't be saved, sharing ends up off either way, and the error shows.
    pub fn set_sharing(&mut self, on: bool, cx: &mut Context<Self>) {
        let saved = match &self.host.settings_path {
            // A file that doesn't parse is left alone, and the toggle doesn't move.
            Some(path) => AppSettings::update(path, |s| s.share_on_tailnet = on).map(drop),
            None => Err(anyhow::anyhow!(
                "no config directory on this OS, so the setting can't be saved"
            )),
        };
        self.enabled = on && saved.is_ok();
        if self.enabled {
            self.start(cx);
        } else {
            self.stop(cx);
        }
        if let Err(e) = saved {
            self.status = Status::Failed(format!("{e:#}"));
            cx.notify();
        }
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        self.stop(cx);
        self.status = Status::Starting;
        let generation = self.generation.load(Ordering::SeqCst);
        let (tailnet, config, backend, current, bind) = (
            self.host.tailnet.clone(),
            self.host.config.clone(),
            self.host.backend.clone(),
            self.generation.clone(),
            self.bind.clone(),
        );
        // Shells out to tailscale (status, then cert, which can take tens of seconds the first time).
        let started = cx.background_spawn(async move {
            let _bind = lock_unpoisoned(&bind);
            let daemon = Daemon::start(config, tailnet, backend)?;
            if current.load(Ordering::SeqCst) != generation {
                drop(daemon); // stopped meanwhile; freed before the next start can bind
                anyhow::bail!("stopped while starting");
            }
            let port = daemon.local_addrs().first().map_or(0, |a| a.port());
            anyhow::Ok((daemon, port))
        });
        // A start still running is detached, not cancelled, so its stale daemon is dropped under `bind`.
        if let Some(old) = self._start.take() {
            old.detach();
        }
        self._start = Some(cx.spawn(async move |this, cx| {
            let started = started.await;
            let _ = this.update(cx, |this, cx| {
                if this.generation.load(Ordering::SeqCst) != generation {
                    if let Ok((daemon, ..)) = started {
                        drop_off_thread(daemon, this.bind.clone());
                    }
                    return;
                }
                this.status = match started {
                    Ok((daemon, port)) => {
                        let cert = daemon.certificate();
                        *lock_slot(&this.host.slot) = Some(daemon);
                        this.poll_peers(cx);
                        Status::Sharing {
                            cert_until: cert.expires_utc(),
                            name: cert.name,
                            port,
                        }
                    }
                    Err(e) => Status::Failed(format!("{e:#}")),
                };
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        self.halt();
        self._poll = None;
        self.peers.clear();
        self.status = Status::Off;
        cx.notify();
    }

    fn halt(&mut self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        if let Some(daemon) = lock_slot(&self.host.slot).take() {
            drop_off_thread(daemon, self.bind.clone());
        }
    }

    /// Which sharing session is live, if any: a call queued under one never runs under another.
    pub fn sharing_session(&self) -> Option<u64> {
        matches!(self.status, Status::Sharing { .. })
            .then(|| self.generation.load(Ordering::SeqCst))
    }

    /// Also picks up a renewed certificate's expiry.
    pub fn refresh_peers(&mut self, cx: &mut Context<Self>) {
        let (peers, cert) = match &*lock_slot(&self.host.slot) {
            Some(daemon) => (daemon.peers(), Some(daemon.certificate())),
            None => (Vec::new(), None),
        };
        if peers != self.peers {
            self.peers = peers;
            cx.notify();
        }
        if let (Status::Sharing { cert_until, .. }, Some(cert)) = (&mut self.status, cert) {
            let until = cert.expires_utc();
            if *cert_until != until {
                *cert_until = until;
                cx.notify();
            }
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

/// The window closed: nothing can show or stop sharing any more, so it stops. (The store's push hook keeps
/// the slot alive, so without this the daemon would outlive the window.)
impl Drop for Remote {
    fn drop(&mut self) {
        self.halt();
    }
}

fn lock_unpoisoned<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Dropping a daemon joins its thread (up to a second), so never on the UI thread.
fn drop_off_thread(daemon: Daemon, bind: Arc<Mutex<()>>) {
    let _ = std::thread::Builder::new()
        .name("skyelab-daemon-stop".into())
        .spawn(move || {
            let _bind = lock_unpoisoned(&bind);
            drop(daemon);
        });
}

#[cfg(test)]
mod tests;
