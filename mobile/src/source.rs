//! Where the screens get projects, threads and events from.

use skyelab_protocol::{Project, Thread, ThreadEvent};

// later: replace with `skyelab-client` (docs/mobile.md step 1) once it lands on main. That client is async and
// channel-based, so this trait becomes a view over its cached state plus requests sent down its channel.
// The fake's event kinds and `{"text": ..}` payloads are made up; the real ones are the store's `prompt`/`agent` records
// (skyelab-app `Record`), so `app::event_text` changes with it.
pub trait ThreadSource {
    fn projects(&self) -> Vec<Project>;
    fn threads(&self, project_id: i64) -> Vec<Thread>;
    fn events(&self, thread_id: i64) -> Vec<ThreadEvent>;
    fn send_message(&mut self, thread_id: i64, text: &str);
    fn interrupt(&mut self, thread_id: i64);
}

/// Canned in-memory data, so the screens can be built and checked before the real client exists.
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
        for (thread_id, kind, text) in [
            (
                1,
                "user",
                "The login page redirects to / instead of the page I came from.",
            ),
            (
                1,
                "assistant",
                "Found it: the redirect drops the `next` query parameter. Fixing auth/login.rs.",
            ),
            (1, "tool", "cargo test -p auth"),
            (
                2,
                "user",
                "Add a dark theme that follows the system setting.",
            ),
        ] {
            fake.push(thread_id, kind, text);
        }
        fake
    }

    fn push(&mut self, thread_id: i64, kind: &str, text: &str) {
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
            payload_json: serde_json::json!({ "text": text }).to_string(),
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

    fn send_message(&mut self, thread_id: i64, text: &str) {
        self.push(thread_id, "user", text);
        self.set_status(thread_id, "running");
    }

    fn interrupt(&mut self, thread_id: i64) {
        self.push(thread_id, "interrupted", "Stopped from the phone.");
        self.set_status(thread_id, "interrupted");
    }
}
