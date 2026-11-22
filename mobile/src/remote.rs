//! [`ThreadSource`] over `skyelab-client`: a connection to one environment, cached for the screens.
//!
//! Nothing here blocks the caller. Updates (connection changes, watched events) and request results all land in
//! one inbox; the app keeps receiving from it and hands each message to [`ThreadSource::apply`].

use std::collections::HashMap;

use skyelab_client::{Client, Error, Options, Pending, Project, Thread, ThreadEvent, Update};

use crate::source::{Connection, Msg, ThreadSource};

pub(crate) enum Inbound {
    Update(Update),
    Projects(Result<Vec<Project>, Error>),
    Threads(i64, Result<Vec<Thread>, Error>),
    /// A send or stop finished; `what` names it for the error line.
    Done(&'static str, Result<(), Error>),
}

/// The client says this when the server closed the socket before the WebSocket handshake, which is what the
/// daemon's gate does to a device whois doesn't accept. tests/remote_source.rs pins it against a real daemon.
const GATE_CLOSED: &str = "WebSocket handshake failed";

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
    /// The watched thread's events.
    events: Vec<ThreadEvent>,
}

impl RemoteSource {
    /// Starts dialing `host` in the background. Fails only if a thread can't be started.
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
                self.refresh();
            }
            Update::Disconnected(reason) if reason.starts_with(GATE_CLOSED) => {
                self.connection = Connection::Refused;
            }
            Update::Disconnected(reason) => self.connection = Connection::Reconnecting(reason),
            // The client delivers each watched event once, in order; a thread we stopped watching may still
            // have a few in flight.
            Update::Event(event) if Some(event.thread_id) == self.watched => {
                self.events.push(event)
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
            Inbound::Projects(Ok(projects)) => self.projects = projects,
            Inbound::Threads(project_id, Ok(threads)) => {
                self.threads.insert(project_id, threads);
            }
            Inbound::Done(_, Ok(())) => {
                // The thread's status changed; the protocol doesn't push that.
                if let Some(project_id) = self.open_project {
                    self.load_threads(project_id);
                }
            }
            Inbound::Projects(Err(e)) => self.error = Some(format!("Couldn't list projects: {e}")),
            Inbound::Threads(_, Err(e)) => self.error = Some(format!("Couldn't list threads: {e}")),
            Inbound::Done(what, Err(e)) => self.error = Some(format!("Couldn't {what}: {e}")),
        }
    }
}
