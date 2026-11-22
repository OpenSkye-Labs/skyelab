pub mod common;

use std::time::{Duration, Instant};

use common::{Env, NODE_NAME, options};
use skyelab_mobile::{
    remote::RemoteSource,
    source::{Connection, ThreadSource},
};

fn settle(source: &mut RemoteSource, done: impl Fn(&RemoteSource) -> bool) {
    let inbox = source.inbox().expect("inbox");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done(source) {
        let msg = inbox
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("timed out");
        source.apply(msg);
    }
}

#[test]
fn rewatching_a_thread_does_not_duplicate_events() {
    let env = Env::start();
    let mut source = RemoteSource::connect(NODE_NAME, options(env.port())).expect("client");
    let inbox = source.inbox().expect("inbox");
    settle(&mut source, |s| {
        s.connection() == Connection::Connected && !s.projects().is_empty()
    });
    source.watch(env.thread_id);
    settle(&mut source, |s| s.events(env.thread_id).len() == 2);
    let live = env.push(
        "agent",
        r#"{"Assistant":{"content":[{"type":"text","text":"live"}]}}"#,
    );
    // The live event reaches the inbox but isn't applied yet, as when the app's pump hasn't run.
    let deadline = Instant::now() + Duration::from_secs(5);
    while inbox.is_empty() {
        assert!(Instant::now() < deadline, "live event never arrived");
        std::thread::sleep(Duration::from_millis(10));
    }
    source.watch(env.thread_id + 1000);
    source.watch(env.thread_id);
    settle(&mut source, |s| s.events(env.thread_id).len() >= 3);
    std::thread::sleep(Duration::from_millis(300));
    while let Ok(msg) = inbox.try_recv() {
        source.apply(msg);
    }
    let seqs: Vec<i64> = source.events(env.thread_id).iter().map(|e| e.seq).collect();
    assert_eq!(
        seqs,
        vec![1, 2, live.seq],
        "duplicated events after re-watch"
    );
}
