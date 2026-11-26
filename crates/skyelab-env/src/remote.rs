//! Another machine, over the wire: each call is the client's, waited on.

use skyelab_client::Client;
use skyelab_protocol::{PermissionAnswer, Project, Thread, ThreadEvent};

use crate::{EnvironmentId, Error, ShellFeed, Source};

pub struct RemoteSource(Client);

impl RemoteSource {
    pub fn new(client: Client) -> Self {
        Self(client)
    }

    /// The wire client, for what isn't a [`Source`] call yet (watching, forwards, approval).
    pub fn client(&self) -> &Client {
        &self.0
    }
}

impl Source for RemoteSource {
    fn environment(&self) -> Option<EnvironmentId> {
        self.0.hello()?.node_id.map(EnvironmentId::new)
    }
    fn list_projects(&self) -> Result<Vec<Project>, Error> {
        self.0.list_projects().recv()
    }
    fn list_threads(&self, project_id: i64) -> Result<Vec<Thread>, Error> {
        self.0.list_threads(project_id).recv()
    }
    fn thread_events_since(&self, thread_id: i64, seq: i64) -> Result<Vec<ThreadEvent>, Error> {
        self.0.thread_events_since(thread_id, seq).recv()
    }
    fn start_thread(&self, project_id: i64, prompt: &str) -> Result<i64, Error> {
        self.0.start_thread(project_id, prompt).recv()
    }
    fn send_message(&self, thread_id: i64, text: &str) -> Result<(), Error> {
        self.0.send_message(thread_id, text).recv()
    }
    fn interrupt(&self, thread_id: i64) -> Result<(), Error> {
        self.0.interrupt(thread_id).recv()
    }
    fn answer_permission(
        &self,
        thread_id: i64,
        request_id: &str,
        decision: PermissionAnswer,
    ) -> Result<(), Error> {
        self.0
            .answer_permission(thread_id, request_id, decision)
            .recv()
    }
    fn answer_questions(
        &self,
        thread_id: i64,
        request_id: &str,
        answers: &[Vec<String>],
    ) -> Result<(), Error> {
        self.0
            .answer_questions(thread_id, request_id, answers.to_vec())
            .recv()
    }
    fn shelve_thread(
        &self,
        thread_id: i64,
        settled: bool,
        snoozed_until: Option<i64>,
    ) -> Result<(), Error> {
        self.0
            .shelve_thread(thread_id, settled, snoozed_until)
            .recv()
    }
    fn subscribe_shell(&self) -> Result<ShellFeed, Error> {
        self.0.subscribe_shell().recv()
    }
}
