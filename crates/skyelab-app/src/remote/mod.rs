//! Sharing this machine on the user's tailnet. The daemon does all the gating; this only turns it on and off.

mod agents;
mod panel;

pub(crate) use agents::internal;
pub use agents::{RemoteCall, Reply, UiAgents};
pub use panel::RemotePanel;

use gpui::{Action, AppContext as _, Context, Hsla, SharedString, Task, actions};
use gpui_component::Theme;
use skyelab_core::{AppSettings, ApprovedDevice};
use skyelab_daemon::{AccessRequest, Approval, Backend, Config, Daemon, Peer, SharedTailnet};
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

/// One device, as the approval actions name it.
#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct AllowDevice {
    pub node_id: String,
    pub key_hash: String,
}

#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct DenyDevice {
    pub node_id: String,
    pub key_hash: String,
}

#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct RevokeDevice {
    pub node_id: String,
    pub key_hash: String,
}

fn approvals(devices: &[ApprovedDevice]) -> Vec<Approval> {
    devices
        .iter()
        .map(|d| Approval {
            node_id: d.node_id.clone(),
            key_hash: d.key_hash.clone(),
        })
        .collect()
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
    /// Devices waiting for Allow or Deny, as the daemon lists them.
    pub requests: Vec<AccessRequest>,
    /// The devices allowed in settings.toml.
    pub approved: Vec<ApprovedDevice>,
    /// Why the last Allow or Revoke couldn't be saved.
    pub approval_error: Option<String>,
    /// Revokes whose save failed: kept out of every later save, so a later Allow can't bring them back.
    unsaved_revokes: Vec<Approval>,
    /// Bumped on every stop, so a start that finishes after it drops its daemon instead of sharing.
    generation: Arc<AtomicU64>,
    /// Held while binding and while dropping a daemon, so a quick off→on never finds port 7287 still taken.
    bind: Arc<Mutex<()>>,
    /// Stopped daemons not dropped yet. Filled on the UI thread at once; emptied (dropped) by whoever takes `bind`
    /// next, so a start that wins `bind` before the stop's drop thread still frees the old port before binding.
    retired: Retired,
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
            requests: Vec::new(),
            approved: Vec::new(),
            approval_error: None,
            unsaved_revokes: Vec::new(),
            generation: Arc::default(),
            bind: Arc::default(),
            retired: Retired::default(),
            _start: None,
            _poll: None,
        };
        if let Some(Ok(s)) = &loaded {
            this.approved = s.approved_devices.clone();
        }
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
        let (tailnet, mut config, backend, current, bind, retired) = (
            self.host.tailnet.clone(),
            self.host.config.clone(),
            self.host.backend.clone(),
            self.generation.clone(),
            self.bind.clone(),
            self.retired.clone(),
        );
        // From settings.toml as it is now. A file that can't be read approves nobody, never the last good list.
        let saved = self.host.settings_path.as_deref().map(AppSettings::load);
        let unsaved = &self.unsaved_revokes;
        self.approved = match saved {
            Some(Ok(s)) => s.approved_devices,
            _ => Vec::new(),
        };
        self.approved.retain(|d| {
            !unsaved
                .iter()
                .any(|r| r.node_id == d.node_id && r.key_hash == d.key_hash)
        });
        config.approved = approvals(&self.approved);
        // Shells out to tailscale (status, then cert, which can take tens of seconds the first time).
        let started = cx.background_spawn(async move {
            let _bind = lock_unpoisoned(&bind);
            let old = std::mem::take(&mut *lock_unpoisoned(&retired)); // guard ends here: the drop may take a second
            drop(old);
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
                        drop_off_thread(daemon, this.bind.clone(), &this.retired);
                    }
                    return;
                }
                this.status = match started {
                    Ok((daemon, port)) => {
                        let cert = daemon.certificate();
                        // The list may have changed while it started (a Revoke during "Starting").
                        daemon.set_approved(approvals(&this.approved));
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
        self.requests.clear();
        self.status = Status::Off;
        cx.notify();
    }

    fn halt(&mut self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        if let Some(daemon) = lock_slot(&self.host.slot).take() {
            drop_off_thread(daemon, self.bind.clone(), &self.retired);
        }
    }

    /// Which sharing session is live, if any: a call queued under one never runs under another.
    pub fn sharing_session(&self) -> Option<u64> {
        matches!(self.status, Status::Sharing { .. })
            .then(|| self.generation.load(Ordering::SeqCst))
    }

    /// Also picks up waiting devices and a renewed certificate's expiry.
    pub fn refresh_peers(&mut self, cx: &mut Context<Self>) {
        let (peers, requests, cert) = match &*lock_slot(&self.host.slot) {
            Some(daemon) => (
                daemon.peers(),
                daemon.requests(),
                Some(daemon.certificate()),
            ),
            None => (Vec::new(), Vec::new(), None),
        };
        if peers != self.peers || requests != self.requests {
            self.peers = peers;
            self.requests = requests;
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

    pub fn request_label(request: &AccessRequest) -> String {
        format!("{} ({}) wants to connect", request.device, request.user)
    }

    /// Saves the waiting device `node_id`/`key_hash` as approved, then lets it in. Nothing is approved that
    /// wasn't saved: an Allow that can't be written shows why and changes nothing.
    pub fn allow(&mut self, node_id: &str, key_hash: &str, cx: &mut Context<Self>) {
        let Some(request) = self
            .requests
            .iter()
            .find(|r| r.node_id == node_id && r.key_hash == key_hash)
            .cloned()
        else {
            return; // answered or expired meanwhile
        };
        let device = ApprovedDevice {
            node_id: request.node_id,
            key_hash: request.key_hash,
            name: request.device,
            user: request.user,
        };
        let saved = self.save_approved(|list| {
            if !list
                .iter()
                .any(|d| d.node_id == device.node_id && d.key_hash == device.key_hash)
            {
                list.push(device);
            }
        });
        if saved {
            self.apply_approved(cx);
        }
        cx.notify();
    }

    /// Cuts the device off first, then forgets it: even if saving fails it stays out until sharing restarts.
    pub fn revoke(&mut self, node_id: &str, key_hash: &str, cx: &mut Context<Self>) {
        let gone = Approval {
            node_id: node_id.to_owned(),
            key_hash: key_hash.to_owned(),
        };
        self.approved
            .retain(|d| !(d.node_id == node_id && d.key_hash == key_hash));
        self.apply_approved(cx);
        if !self.unsaved_revokes.contains(&gone) {
            self.unsaved_revokes.push(gone);
        }
        self.save_approved(|_| {});
        cx.notify();
    }

    /// Turns a waiting device down until sharing stops. Nothing is saved.
    pub fn deny(&mut self, node_id: &str, key_hash: &str, cx: &mut Context<Self>) {
        if let Some(daemon) = &*lock_slot(&self.host.slot) {
            daemon.deny(node_id, key_hash);
        }
        self.refresh_peers(cx);
    }

    /// Changes the saved list; on success `approved` is what the file now says. False (with the error shown) if
    /// it couldn't be saved.
    fn save_approved(&mut self, change: impl FnOnce(&mut Vec<ApprovedDevice>)) -> bool {
        let revoked = self.unsaved_revokes.clone();
        let saved = match &self.host.settings_path {
            Some(path) => AppSettings::update(path, |s| {
                change(&mut s.approved_devices);
                s.approved_devices.retain(|d| {
                    !revoked
                        .iter()
                        .any(|r| r.node_id == d.node_id && r.key_hash == d.key_hash)
                });
            })
            .map(|(s, ())| s.approved_devices),
            None => Err(anyhow::anyhow!(
                "no config directory on this OS, so the device list can't be saved"
            )),
        };
        match saved {
            Ok(list) => {
                self.approved = list;
                self.approval_error = None;
                self.unsaved_revokes.clear();
                true
            }
            Err(e) => {
                self.approval_error = Some(format!(
                    "Couldn't save the device list ({e:#}). A revoked device stays out until sharing restarts."
                ));
                false
            }
        }
    }

    fn apply_approved(&mut self, cx: &mut Context<Self>) {
        if let Some(daemon) = &*lock_slot(&self.host.slot) {
            daemon.set_approved(approvals(&self.approved));
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
type Retired = Arc<Mutex<Vec<Daemon>>>;

fn drop_off_thread(daemon: Daemon, bind: Arc<Mutex<()>>, retired: &Retired) {
    lock_unpoisoned(retired).push(daemon);
    let retired = retired.clone();
    let _ = std::thread::Builder::new()
        .name("skyelab-daemon-stop".into())
        .spawn(move || {
            let _bind = lock_unpoisoned(&bind);
            let old = std::mem::take(&mut *lock_unpoisoned(&retired));
            drop(old);
        });
}

#[cfg(test)]
mod tests;
