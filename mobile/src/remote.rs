//! [`ThreadSource`] over `skyelab-client`: a connection to one environment, cached for the screens.
//!
//! Nothing here blocks the caller. Updates (connection changes, watched events) and request results all land in
//! one inbox; the app keeps receiving from it and hands each message to [`ThreadSource::apply`].

use std::collections::HashMap;
use std::time::Duration;

use skyelab_client::{Client, Error, Options, Pending, Project, Thread, ThreadEvent, Update};

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

/// The daemon's gate closes the socket without answering the WebSocket upgrade when it doesn't accept a device (or
/// its whois fails). The client then reports a handshake that never finished, or a reset when the close found our
/// request unread. An HTTP answer, such as a 404 from another server, reads differently and is not this.
/// tests/remote_source.rs and tests/refused_only_at_gate.rs pin both sides.
fn closed_at_gate(reason: &str) -> bool {
    reason
        .strip_prefix("WebSocket handshake failed: ")
        .is_some_and(|e| e.contains("Handshake not finished") || e.contains("Connection reset"))
}

/// How long to wait before re-listing after a list request failed while connected.
const LIST_RETRY: Duration = Duration::from_secs(2);

pub struct RemoteSource {
    client: Client,
    inbox_tx: flume::Sender<Msg>,
    inbox: flume::Receiver<Msg>,
    connection: Connection,
    error: Option<String>,
    projects: Vec<Project>,
    threads: HashMap<i64, Vec<Thread>>,
    /// The project whose threads are shown; refreshed on reconnect and after a send or stop.
    open_project: Option<i64>,
    watched: Option<i64>,
    events: Vec<ThreadEvent>,
    retry_pending: bool,
}

impl RemoteSource {
    /// Starts dialing `host` in the background. Fails on invalid `options` or when its threads can't start.
    pub fn connect(host: &str, options: Options) -> std::io::Result<Self> {
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
            inbox_tx,
            inbox,
            connection: Connection::Connecting,
            error: None,
            projects: Vec::new(),
            threads: HashMap::new(),
            open_project: None,
            watched: None,
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

    fn on_update(&mut self, update: Update) {
        match update {
            Update::Connected(_) => {
                self.connection = Connection::Connected;
                self.error = None;
                self.refresh();
            }
            Update::Disconnected(reason) if closed_at_gate(&reason) => {
                self.connection = Connection::Refused;
            }
            Update::Disconnected(reason) => self.connection = Connection::Reconnecting(reason),
            // A thread we stopped watching may still have a few in flight, and re-watching replays from seq 1 while
            // events from the earlier watch may still be queued: keep each seq once, in order.
            Update::Event(event) if Some(event.thread_id) == self.watched => {
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

    fn events(&self, thread_id: i64) -> Vec<ThreadEvent> {
        if self.watched == Some(thread_id) {
            self.events.clone()
        } else {
            Vec::new()
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
        if let Some(old) = self.watched.replace(thread_id) {
            self.client.unwatch(old);
        }
        self.events.clear();
        self.client.watch(thread_id, 0);
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
                if let Some(project_id) = self.open_project {
                    self.load_threads(project_id);
                }
            }
            Inbound::Projects(Err(e)) => self.list_failed("projects", e),
            Inbound::Threads(_, Err(e)) => self.list_failed("threads", e),
            Inbound::Done(what, Err(e)) => self.error = Some(format!("Couldn't {what}: {e}")),
        }
    }
}
