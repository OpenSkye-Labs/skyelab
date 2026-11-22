//! [`ThreadSource`] over `skyelab-client`: a connection to one environment, cached for the screens.
//!
//! Nothing here blocks the caller. Updates (connection changes, watched events) and request results all land in
//! one inbox; the app keeps receiving from it and hands each message to [`ThreadSource::apply`].
//!
//! Node pinning: the first node id a name's hello names is saved in the data dir (`env_store::NODES_FILE`), and
//! every connection, reconnects included, is held by the client until its hello matches the pin. This trusts the
//! server's word about itself: the phone has no local Tailscale to ask (docs/mobile.md, step 4).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use skyelab_client::{
    Client, DisconnectKind, Error, Hello, Options, Pending, Project, Thread, ThreadEvent, Update,
    WatchId,
};

use crate::env_store;
use crate::source::{Connection, Msg, ThreadSource};

pub(crate) enum Inbound {
    Update(Update),
    Projects(Result<Vec<Project>, Error>),
    Threads(i64, Result<Vec<Thread>, Error>),
    /// A send or stop finished; `what` names it for the error line.
    Done(&'static str, Result<(), Error>),
    /// Time to retry the lists after one failed.
    Retry,
}

/// How long to wait before re-listing after a list request failed while connected.
const LIST_RETRY: Duration = Duration::from_secs(2);

pub struct RemoteSource {
    client: Client,
    host: String,
    /// Where the pins live (the app's data dir).
    data_dir: PathBuf,
    /// The current connection's hello, `None` while disconnected. The client holds it until approved.
    hello: Option<Hello>,
    inbox_tx: flume::Sender<Msg>,
    inbox: flume::Receiver<Msg>,
    connection: Connection,
    error: Option<String>,
    projects: Vec<Project>,
    threads: HashMap<i64, Vec<Thread>>,
    /// The project whose threads are shown; refreshed on reconnect and after a send or stop.
    open_project: Option<i64>,
    watched: Option<i64>,
    /// The client's handle for `watched`, so only that watch is ended on switching threads.
    watch_id: Option<WatchId>,
    events: Vec<ThreadEvent>,
    retry_pending: bool,
}

impl RemoteSource {
    /// Starts dialing `host` in the background, checking each connection against the node pinned for it in
    /// `data_dir`. Fails on invalid `options` or when its threads can't start.
    pub fn connect(host: &str, mut options: Options, data_dir: &Path) -> std::io::Result<Self> {
        options.approve_each_connection = true;
        let client = Client::connect(host, options)?;
        let (inbox_tx, inbox) = flume::unbounded();
        // The client's update channel is unbounded: drain it for as long as the source lives, whatever the
        // screens are doing. The thread ends when the client stops or the source (with its inbox) is gone.
        let updates = client.updates().clone();
        let tx = inbox_tx.clone();
        std::thread::Builder::new()
            .name("skyelab-mobile-updates".into())
            .spawn(move || {
                for update in updates.iter() {
                    if tx.send(Msg::from(update)).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Self {
            client,
            host: host.trim().to_ascii_lowercase(),
            data_dir: data_dir.to_path_buf(),
            hello: None,
            inbox_tx,
            inbox,
            connection: Connection::Connecting,
            error: None,
            projects: Vec::new(),
            threads: HashMap::new(),
            open_project: None,
            watched: None,
            watch_id: None,
            events: Vec::new(),
            retry_pending: false,
        })
    }

    /// Waits for `pending` off the caller's thread and posts the result to the inbox.
    fn request<T: Send + 'static>(
        &mut self,
        pending: Pending<T>,
        wrap: impl FnOnce(Result<T, Error>) -> Inbound + Send + 'static,
    ) {
        let tx = self.inbox_tx.clone();
        // shortcut: one thread per request while it waits (the client's request timeout bounds it); requests are
        // taps, so a handful at most. Upgrade to one waiter thread if requests ever come in bulk.
        let spawned = std::thread::Builder::new()
            .name("skyelab-mobile-request".into())
            .spawn(move || {
                let _ = tx.send(Msg(wrap(pending.recv())));
            });
        if let Err(e) = spawned {
            self.error = Some(format!("Couldn't start a request: {e}"));
        }
    }

    /// A list request failed: try again shortly, once, rather than wait for the next reconnect.
    fn list_failed(&mut self, what: &str, e: Error) {
        self.error = Some(format!("Couldn't list {what}: {e}"));
        if self.retry_pending {
            return;
        }
        self.retry_pending = true;
        let tx = self.inbox_tx.clone();
        let spawned = std::thread::Builder::new()
            .name("skyelab-mobile-retry".into())
            .spawn(move || {
                std::thread::sleep(LIST_RETRY);
                let _ = tx.send(Msg(Inbound::Retry));
            });
        if spawned.is_err() {
            self.retry_pending = false; // the next failure tries again
        }
    }

    fn refresh(&mut self) {
        self.request(self.client.list_projects(), Inbound::Projects);
        if let Some(project_id) = self.open_project {
            self.load_threads(project_id);
        }
    }

    /// Whether this hello's node may be trusted for the name: the pinned one, or the first one seen (then pinned).
    /// A pinned name whose hello names no node, or pins that can't be read or saved, is refused.
    fn check(&self, hello: &Hello) -> Connection {
        let nodes = match env_store::load_nodes(&self.data_dir) {
            Ok(nodes) => nodes,
            Err(e) => return Connection::Unverified(format!("{e:#}")),
        };
        match (nodes.get(&self.host), &hello.node_id) {
            (Some(was), Some(now)) if was == now => Connection::Connected,
            (Some(was), now) => Connection::NodeChanged {
                was: was.clone(),
                now: now.clone(),
            },
            (None, Some(now)) => match env_store::trust_node(&self.data_dir, &self.host, now) {
                Ok(()) => Connection::Connected,
                Err(e) => Connection::Unverified(format!("couldn't pin it: {e:#}")),
            },
            // Never pinned and no id to pin (an old server): nothing to check against, as on desktop.
            (None, None) => Connection::Connected,
        }
    }

    /// Trusted: release the held connection and list.
    fn up(&mut self) {
        self.connection = Connection::Connected;
        self.error = None;
        if let Some(hello) = &self.hello {
            self.client.approve(hello);
        }
        self.refresh();
    }

    /// Refused or not trusted: nothing of this machine stays, and its watch ends (the thread screen re-watches
    /// from the start once trusted again, so no event is lost).
    fn refuse(&mut self, connection: Connection) {
        self.connection = connection;
        self.projects.clear();
        self.threads.clear();
        self.events.clear();
        self.watched = None;
        if let Some(old) = self.watch_id.take() {
            self.client.unwatch(old);
        }
    }

    fn on_update(&mut self, update: Update) {
        match update {
            Update::Connected(hello) => {
                let checked = self.check(&hello);
                self.hello = Some(hello);
                self.error = None;
                match checked {
                    Connection::Connected => self.up(),
                    refused => self.refuse(refused),
                }
            }
            // The daemon's gate hung up before answering: not one of the user's own untagged devices.
            Update::Disconnected(d) if d.kind == DisconnectKind::Refused => {
                self.hello = None;
                self.refuse(Connection::Refused);
            }
            // Only with a verifier, which the phone doesn't have; refused all the same.
            Update::Disconnected(d)
                if matches!(
                    d.kind,
                    DisconnectKind::NotYourDevice | DisconnectKind::IdentityMismatch
                ) =>
            {
                self.hello = None;
                self.refuse(Connection::Unverified(d.message));
            }
            Update::Disconnected(d) => {
                self.hello = None;
                self.connection = Connection::Reconnecting(d.message);
            }
            // A thread we stopped watching may still have a few in flight, and re-watching replays from seq 1 while
            // events from the earlier watch may still be queued: keep each seq once, in order.
            Update::Event(event)
                if Some(event.thread_id) == self.watched
                    && self.connection == Connection::Connected =>
            {
                if let Err(at) = self.events.binary_search_by_key(&event.seq, |e| e.seq) {
                    self.events.insert(at, event);
                }
            }
            Update::Event(_) => {}
            Update::Fatal(Error::UnsupportedVersion(why)) => {
                self.connection = Connection::VersionMismatch(why);
            }
            Update::Fatal(e) => self.connection = Connection::Stopped(e.to_string()),
        }
    }
}

impl ThreadSource for RemoteSource {
    fn connection(&self) -> Connection {
        self.connection.clone()
    }

    fn error(&self) -> Option<String> {
        self.error.clone()
    }

    fn projects(&self) -> Vec<Project> {
        self.projects.clone()
    }

    fn threads(&self, project_id: i64) -> Vec<Thread> {
        self.threads.get(&project_id).cloned().unwrap_or_default()
    }

    fn events(&self, thread_id: i64) -> &[ThreadEvent] {
        if self.watched == Some(thread_id) {
            &self.events
        } else {
            &[]
        }
    }

    fn load_threads(&mut self, project_id: i64) {
        self.open_project = Some(project_id);
        let pending = self.client.list_threads(project_id);
        self.request(pending, move |r| Inbound::Threads(project_id, r));
    }

    fn watch(&mut self, thread_id: i64) {
        if self.watched == Some(thread_id) {
            return;
        }
        self.watched = Some(thread_id);
        if let Some(old) = self.watch_id.take() {
            self.client.unwatch(old);
        }
        self.events.clear();
        self.watch_id = Some(self.client.watch(thread_id, 0));
    }

    fn send_message(&mut self, thread_id: i64, text: &str) {
        self.error = None;
        let pending = self.client.send_message(thread_id, text);
        self.request(pending, |r| Inbound::Done("send", r));
    }

    fn interrupt(&mut self, thread_id: i64) {
        self.error = None;
        let pending = self.client.interrupt(thread_id);
        self.request(pending, |r| Inbound::Done("stop", r));
    }

    fn trust(&mut self, node_id: &str) {
        let shown = matches!(&self.connection, Connection::NodeChanged { now: Some(now), .. } if now == node_id);
        let live = self.hello.as_ref().and_then(|h| h.node_id.as_deref()) == Some(node_id);
        if !shown || !live {
            self.error = Some("That machine changed again. Check the new warning.".into());
            return;
        }
        match env_store::trust_node(&self.data_dir, &self.host, node_id) {
            Ok(()) => self.up(),
            Err(e) => self.error = Some(format!("Couldn't trust that machine: {e:#}")),
        }
    }

    fn inbox(&self) -> Option<flume::Receiver<Msg>> {
        Some(self.inbox.clone())
    }

    fn apply(&mut self, msg: Msg) {
        match msg.0 {
            Inbound::Update(update) => self.on_update(update),
            Inbound::Projects(Ok(projects)) => {
                self.projects = projects;
                self.error = None;
            }
            Inbound::Threads(project_id, Ok(threads)) => {
                self.threads.insert(project_id, threads);
                self.error = None;
            }
            Inbound::Retry => {
                self.retry_pending = false;
                // Not connected: the next Connected re-lists anyway.
                if self.connection == Connection::Connected {
                    self.refresh();
                }
            }
            Inbound::Done(_, Ok(())) => {
                // The thread's status changed; the protocol doesn't push that.
                if let Some(project_id) = self.open_project
                    && self.connection == Connection::Connected
                {
                    self.load_threads(project_id);
                }
            }
            Inbound::Projects(Err(e)) => self.list_failed("projects", e),
            Inbound::Threads(_, Err(e)) => self.list_failed("threads", e),
            Inbound::Done(what, Err(e)) => self.error = Some(format!("Couldn't {what}: {e}")),
        }
    }
}
