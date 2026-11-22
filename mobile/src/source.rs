//! Where the screens get projects, threads and events from: a cached view the app reads while it renders.
//! [`crate::remote::RemoteSource`] fills it from an environment; [`FakeSource`] holds canned data for tests and
//! the `demo` build.

use skyelab_protocol::{Project, Thread, ThreadEvent};

/// The connection to the environment, as the screens show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Connection {
    Connecting,
    Connected,
    /// The connection dropped or never came up, with why. The client keeps retrying.
    Reconnecting(String),
    /// The machine dropped us at its gate: this device isn't one of the user's own untagged tailnet devices.
    Refused,
    /// The machine speaks another protocol version. The client has stopped.
    VersionMismatch(String),
    /// The client stopped for another reason.
    Stopped(String),
}

/// Something that arrived for a source in the background. Hand it back with [`ThreadSource::apply`].
pub struct Msg(pub(crate) crate::remote::Inbound);

impl From<skyelab_client::Update> for Msg {
    fn from(update: skyelab_client::Update) -> Self {
        Self(crate::remote::Inbound::Update(update))
    }
}

pub trait ThreadSource {
    fn connection(&self) -> Connection;
    /// The last failed request, in words.
    fn error(&self) -> Option<String>;
    fn projects(&self) -> Vec<Project>;
    /// The threads of a project, once [`ThreadSource::load_threads`] has fetched them.
    fn threads(&self, project_id: i64) -> Vec<Thread>;
    /// A watched thread's events, in seq order.
    fn events(&self, thread_id: i64) -> Vec<ThreadEvent>;
    fn load_threads(&mut self, project_id: i64);
    /// Follow this thread's events live, and stop following any other.
    fn watch(&mut self, thread_id: i64);
    fn send_message(&mut self, thread_id: i64, text: &str);
    fn interrupt(&mut self, thread_id: i64);
    /// Where background results arrive. Keep receiving from it and pass each to `apply`. `None`: the source
    /// only changes when called.
    fn inbox(&self) -> Option<flume::Receiver<Msg>> {
        None
    }
    fn apply(&mut self, _msg: Msg) {}
}

/// Canned in-memory data, so the screens can be checked without an environment. Payloads use the desktop store's
/// kinds: `prompt` (a JSON string) and `agent` (a Claude stream event).
pub struct FakeSource {
    projects: Vec<Project>,
    threads: Vec<Thread>,
    events: Vec<ThreadEvent>,
}

impl FakeSource {
    pub fn new() -> Self {
        let project = |id, name: &str| Project {
            id,
            name: name.into(),
            path: format!("/Users/me/code/{name}"),
        };
        let thread = |id, project_id, title: &str, account: &str, status: &str| Thread {
            id,
            project_id,
            title: title.into(),
            account_id: account.into(),
            status: status.into(),
            created_at: 0,
            updated_at: 0,
        };
        let mut fake = Self {
            projects: vec![project(1, "skyelab"), project(2, "website")],
            threads: vec![
                thread(1, 1, "Fix login redirect", "work", "running"),
                thread(2, 1, "Add dark mode", "work", "idle"),
                thread(3, 1, "Bump tokio", "personal", "done"),
                thread(4, 2, "Rewrite the pricing page", "work", "idle"),
            ],
            events: Vec::new(),
        };
        fake.prompt(
            1,
            "The login page redirects to / instead of the page I came from.",
        );
        fake.agent_text(
            1,
            "Found it: the redirect drops the `next` query parameter. Fixing auth/login.rs.",
        );
        fake.push(
            1,
            "agent",
            serde_json::json!({"type": "assistant", "content": [
                {"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "cargo test -p auth"}}
            ]}),
        );
        fake.prompt(2, "Add a dark theme that follows the system setting.");
        fake
    }

    fn prompt(&mut self, thread_id: i64, text: &str) {
        self.push(thread_id, "prompt", serde_json::json!(text));
    }

    fn agent_text(&mut self, thread_id: i64, text: &str) {
        self.push(
            thread_id,
            "agent",
            serde_json::json!({"type": "assistant", "content": [{"type": "text", "text": text}]}),
        );
    }

    fn push(&mut self, thread_id: i64, kind: &str, payload: serde_json::Value) {
        let seq = self
            .events
            .iter()
            .filter(|e| e.thread_id == thread_id)
            .count() as i64
            + 1;
        self.events.push(ThreadEvent {
            thread_id,
            seq,
            kind: kind.into(),
            payload_json: payload.to_string(),
            created_at: 0,
        });
    }

    fn set_status(&mut self, thread_id: i64, status: &str) {
        if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
            t.status = status.into();
        }
    }
}

impl Default for FakeSource {
    fn default() -> Self {
        Self::new()
    }
}

impl ThreadSource for FakeSource {
    fn connection(&self) -> Connection {
        Connection::Connected
    }

    fn error(&self) -> Option<String> {
        None
    }

    fn projects(&self) -> Vec<Project> {
        self.projects.clone()
    }

    fn threads(&self, project_id: i64) -> Vec<Thread> {
        self.threads
            .iter()
            .filter(|t| t.project_id == project_id)
            .cloned()
            .collect()
    }

    fn events(&self, thread_id: i64) -> Vec<ThreadEvent> {
        self.events
            .iter()
            .filter(|e| e.thread_id == thread_id)
            .cloned()
            .collect()
    }

    fn load_threads(&mut self, _project_id: i64) {}

    fn watch(&mut self, _thread_id: i64) {}

    fn send_message(&mut self, thread_id: i64, text: &str) {
        self.prompt(thread_id, text);
        self.set_status(thread_id, "running");
    }

    fn interrupt(&mut self, thread_id: i64) {
        self.agent_text(thread_id, "Stopped from the phone.");
        self.set_status(thread_id, "interrupted");
    }
}
