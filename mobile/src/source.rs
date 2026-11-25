//! Where the screens get projects, threads and events from: a cached view the app reads while it renders.
//! [`crate::remote::RemoteSource`] fills it from an environment; [`FakeSource`] holds canned data for tests and
//! the `demo` build.

use std::collections::HashMap;

use skyelab_agent::timeline::Record;
use skyelab_agent::{Block, Decision, Event};
use skyelab_protocol::{PermissionAnswer, Project, Thread, ThreadEvent};

/// The connection to the environment, as the screens show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Connection {
    Connecting,
    Connected,
    /// The connection dropped or never came up, with why. The client keeps retrying.
    Reconnecting(String),
    /// The machine dropped us at its gate: this device isn't one of the user's own untagged tailnet devices.
    Refused,
    /// The name answered from another node than the one pinned for it (`now`: the id its hello named, `None` if it
    /// sent none). Nothing is listed or sent until the user trusts `now`.
    NodeChanged {
        was: String,
        now: Option<String>,
    },
    /// The pins can't be read or saved (or the client couldn't confirm the node), so the machine can't be checked.
    Unverified(String),
    /// The machine speaks another protocol version. The client has stopped.
    VersionMismatch(String),
    /// The client stopped for another reason.
    Stopped(String),
}

impl Connection {
    /// Refused, or the machine's identity failed: nothing of it may stay on screen.
    pub fn is_untrusted(&self) -> bool {
        matches!(
            self,
            Self::Refused | Self::NodeChanged { .. } | Self::Unverified(_)
        )
    }
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
    /// Every project's threads, once [`ThreadSource::load_threads`] has fetched them.
    fn threads(&self) -> Vec<Thread>;
    /// A watched thread's events, in seq order. Borrowed: the thread screen reads it every frame.
    fn events(&self, thread_id: i64) -> &[ThreadEvent];
    /// Lists every project's threads again (their status and previews change; the protocol pushes neither).
    fn load_threads(&mut self);
    /// Follow this thread's events live, and stop following any other.
    fn watch(&mut self, thread_id: i64);
    fn send_message(&mut self, thread_id: i64, text: &str);
    fn interrupt(&mut self, thread_id: i64);
    /// Answers a permission request the thread's agent is waiting on.
    fn answer_permission(
        &mut self,
        _thread_id: i64,
        _request_id: &str,
        _decision: PermissionAnswer,
    ) {
    }
    /// Answers an AskUserQuestion the thread's agent waits on: one list of picked labels per question.
    fn answer_questions(&mut self, _thread_id: i64, _request_id: &str, _answers: Vec<Vec<String>>) {
    }
    /// Whether the machine speaks protocol 1.`minor` or later (questions need 1.4).
    fn speaks(&self, _minor: u32) -> bool {
        true
    }
    /// Pins the name to `node_id` and connects, if that is still the node the `NodeChanged` warning shows.
    fn trust(&mut self, _node_id: &str) {}
    /// Where background results arrive. Keep receiving from it and pass each to `apply`. `None`: the source
    /// only changes when called.
    fn inbox(&self) -> Option<flume::Receiver<Msg>> {
        None
    }
    fn apply(&mut self, _msg: Msg) {}
}

/// Canned in-memory data, so the screens can be checked without an environment. Rows are built by the desktop's own
/// serializer ([`Record::to_row`]), never by hand.
pub struct FakeSource {
    projects: Vec<Project>,
    threads: Vec<Thread>,
    events: HashMap<i64, Vec<ThreadEvent>>,
}

impl FakeSource {
    pub fn new() -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as i64);
        let project = |id, name: &str| Project {
            id,
            name: name.into(),
            path: format!("/Users/me/code/{name}"),
        };
        let thread = |id, project_id, title: &str, status: &str, ago_h: i64| Thread {
            id,
            project_id,
            title: title.into(),
            account_id: if id == 3 { "personal" } else { "work" }.into(),
            account_label: (id != 3).then(|| "Work".into()),
            status: status.into(),
            pinned: id == 2,
            preview: Some(
                [
                    "Fixed the redirect: login keeps `next`.",
                    "Dark mode follows the system now.",
                ][id as usize % 2]
                    .into(),
            ),
            settled_at: None,
            snoozed_until: None,
            created_at: now - ago_h * 3_600_000,
            updated_at: now - ago_h * 3_600_000,
        };
        let mut fake = Self {
            projects: vec![project(1, "skyelab"), project(2, "website")],
            threads: vec![
                thread(1, 1, "Fix login redirect", "running", 0),
                thread(2, 1, "Add dark mode", "idle", 2),
                thread(3, 1, "Bump tokio", "done", 30),
                thread(4, 2, "Rewrite the pricing page", "idle", 24 * 9),
            ],
            events: HashMap::new(),
        };
        for r in crate::demo::thread_one() {
            fake.record(1, &r);
        }
        fake.prompt(2, "Add a dark theme that follows the system setting.");
        fake
    }

    fn prompt(&mut self, thread_id: i64, text: &str) {
        self.record(thread_id, &Record::Prompt(text.into()));
    }

    fn agent_text(&mut self, thread_id: i64, text: &str) {
        let text = text.into();
        let content = vec![Block::Text { text }];
        self.record(thread_id, &Record::Agent(Event::Assistant { content }));
    }

    fn record(&mut self, thread_id: i64, record: &Record) {
        let (kind, payload_json) = record.to_row();
        let events = self.events.entry(thread_id).or_default();
        events.push(ThreadEvent {
            thread_id,
            seq: events.len() as i64 + 1,
            kind: kind.into(),
            payload_json,
            created_at: 0,
        });
    }

    fn set_status(&mut self, thread_id: i64, status: &str) {
        if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
            t.status = status.into();
        }
    }

    fn resolve(&mut self, thread_id: i64, id: &str, decision: Decision) {
        let id = id.into();
        self.record(
            thread_id,
            &Record::Agent(Event::PermissionResolved {
                id,
                decision,
                rule: None,
            }),
        );
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

    fn threads(&self) -> Vec<Thread> {
        self.threads.clone()
    }

    fn events(&self, thread_id: i64) -> &[ThreadEvent] {
        self.events.get(&thread_id).map_or(&[], Vec::as_slice)
    }

    fn load_threads(&mut self) {}

    fn watch(&mut self, _thread_id: i64) {}

    fn send_message(&mut self, thread_id: i64, text: &str) {
        self.prompt(thread_id, text);
        self.set_status(thread_id, "running");
    }

    fn interrupt(&mut self, thread_id: i64) {
        self.agent_text(thread_id, "Stopped from the phone.");
        self.set_status(thread_id, "interrupted");
    }

    fn answer_permission(&mut self, thread_id: i64, request_id: &str, decision: PermissionAnswer) {
        let decision = match decision {
            PermissionAnswer::AllowOnce => Decision::AllowOnce,
            PermissionAnswer::AllowThread => Decision::AllowThread,
            PermissionAnswer::Deny => Decision::Deny,
        };
        self.resolve(thread_id, request_id, decision);
    }

    fn answer_questions(&mut self, thread_id: i64, request_id: &str, _answers: Vec<Vec<String>>) {
        self.resolve(thread_id, request_id, Decision::AllowOnce);
    }
}
