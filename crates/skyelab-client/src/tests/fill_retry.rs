//! A fill the daemon refuses (TooManyRequests) must be retried, or the thread's events are lost.

use std::sync::Arc;
use std::time::Duration;

use skyelab_daemon::Daemon;
use skyelab_protocol::{Project, RpcError, Thread, ThreadEvent};

use super::*;

/// The real store, but every events query takes 500 ms, so a second one finds the in-flight slot taken.
struct Slow(Arc<dyn Backend>);

impl Backend for Slow {
    fn list_projects(&self) -> Result<Vec<Project>, RpcError> {
        self.0.list_projects()
    }
    fn list_threads(&self, project_id: i64) -> Result<Vec<Thread>, RpcError> {
        self.0.list_threads(project_id)
    }
    fn thread_events_since(&self, thread_id: i64, seq: i64) -> Result<Vec<ThreadEvent>, RpcError> {
        std::thread::sleep(Duration::from_millis(500));
        self.0.thread_events_since(thread_id, seq)
    }
    fn start_thread(&self, project_id: i64, prompt: &str) -> Result<i64, RpcError> {
        self.0.start_thread(project_id, prompt)
    }
    fn send_message(&self, thread_id: i64, text: &str) -> Result<(), RpcError> {
        self.0.send_message(thread_id, text)
    }
    fn interrupt(&self, thread_id: i64) -> Result<(), RpcError> {
        self.0.interrupt(thread_id)
    }
}

#[test]
fn refused_fill_is_retried() {
    let env = Env::new();
    env.append(3);
    let config = Config {
        max_in_flight: 1,
        ..daemon_config(0)
    };
    let daemon = Daemon::start(
        config,
        Arc::new(FakeTailnet::default()),
        Arc::new(Slow(env.backend())),
    )
    .expect("daemon");
    let client = Client::connect(HOST, options(port(&daemon))).expect("client");
    connected(&client);
    // Another watched thread's fill holds the only in-flight slot...
    let _watch = client.watch(env.thread_id + 100, 0);
    // ...so this one's fill is refused with TooManyRequests and must be retried.
    let _watch = client.watch(env.thread_id, 0);
    assert_eq!(seqs(&client, 3), [1, 2, 3]);
}
