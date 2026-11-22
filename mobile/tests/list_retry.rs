//! A list that fails while connected is retried on a timer, not left empty until the next reconnect.

pub mod common;

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use common::{NODE_NAME, options, start};
use skyelab_mobile::{
    remote::RemoteSource,
    source::{Connection, ThreadSource},
};

#[test]
fn a_failed_thread_list_is_retried() {
    let env = start();
    let mut source = RemoteSource::connect(NODE_NAME, options(env.port)).expect("client");
    let inbox = source.inbox().expect("inbox");
    let deadline = Instant::now() + Duration::from_secs(10);
    let settle = |source: &mut RemoteSource, what: &str, done: &dyn Fn(&RemoteSource) -> bool| {
        while !done(source) {
            let left = deadline.saturating_duration_since(Instant::now());
            let msg = inbox
                .recv_timeout(left)
                .unwrap_or_else(|_| panic!("timed out waiting for {what}"));
            source.apply(msg);
        }
    };
    settle(&mut source, "connected", &|s| {
        s.connection() == Connection::Connected
    });

    env.fail_lists.store(1, Ordering::SeqCst);
    source.load_threads(env.project_id);
    settle(&mut source, "the error", &|s| s.error().is_some());
    assert!(source.threads(env.project_id).is_empty());
    settle(&mut source, "the retried list", &|s| {
        !s.threads(env.project_id).is_empty()
    });
    assert_eq!(source.error(), None);
}
