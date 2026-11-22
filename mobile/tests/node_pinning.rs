//! Acceptance: the phone pins the node a name answered from (from the hello, all it has), refuses another node, a
//! missing id or a pin file it can't read, and Trust re-pins exactly the node shown.

pub mod common;

use std::path::Path;
use std::time::{Duration, Instant};

use common::{Env, NODE_NAME, options, start};
use skyelab_client::Update;
use skyelab_mobile::{
    env_store,
    remote::RemoteSource,
    source::{Connection, Msg, ThreadSource},
};
use skyelab_protocol::{Hello, VERSION};

fn settle(source: &mut RemoteSource, what: &str, done: impl Fn(&RemoteSource) -> bool) {
    let inbox = source.inbox().expect("a remote source has an inbox");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done(source) {
        let left = deadline.saturating_duration_since(Instant::now());
        match inbox.recv_timeout(left) {
            Ok(msg) => source.apply(msg),
            Err(_) => panic!(
                "timed out waiting for {what}; connection {:?}, error {:?}",
                source.connection(),
                source.error()
            ),
        }
    }
}

/// Applies whatever arrives for a while, so a wrong turn (a listing after a refusal) has time to show.
fn linger(source: &mut RemoteSource) {
    let inbox = source.inbox().expect("inbox");
    let until = Instant::now() + Duration::from_millis(400);
    while let Ok(msg) = inbox.recv_deadline(until) {
        source.apply(msg);
    }
}

fn pin(dir: &Path) -> Option<String> {
    env_store::load_nodes(dir)
        .expect("readable pins")
        .get(NODE_NAME)
        .cloned()
}

fn connect(env: &Env, dir: &Path) -> RemoteSource {
    RemoteSource::connect(NODE_NAME, options(env.port), dir).expect("client thread")
}

fn changed(source: &RemoteSource) -> bool {
    matches!(source.connection(), Connection::NodeChanged { .. })
}

#[test]
fn the_first_node_is_pinned() {
    let env = start();
    let dir = tempfile::tempdir().expect("dir");
    let mut source = connect(&env, dir.path());
    settle(&mut source, "projects", |s| !s.projects().is_empty());
    assert_eq!(source.connection(), Connection::Connected);
    assert_eq!(pin(dir.path()).as_deref(), Some("nSelf"));
}

#[test]
fn another_node_is_refused_and_asked_nothing() {
    let env = start();
    let dir = tempfile::tempdir().expect("dir");
    env_store::trust_node(dir.path(), NODE_NAME, "nOld").expect("pin");
    let mut source = connect(&env, dir.path());
    // An open thread, as on a reconnect: its refill must wait for the check too.
    source.watch(env.thread_id);
    settle(&mut source, "the warning", changed);
    linger(&mut source);
    assert_eq!(
        source.connection(),
        Connection::NodeChanged {
            was: "nOld".into(),
            now: Some("nSelf".into())
        }
    );
    assert!(source.projects().is_empty());
    assert_eq!(env.asked(), Vec::<&str>::new(), "nothing asked of it");
    assert_eq!(pin(dir.path()).as_deref(), Some("nOld"), "not overwritten");
}

#[test]
fn a_pinned_name_whose_hello_has_no_id_is_refused() {
    // Nothing listens there, so only the hello below arrives; a listing would fail with "not connected".
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .expect("addr")
        .port();
    let dir = tempfile::tempdir().expect("dir");
    env_store::trust_node(dir.path(), NODE_NAME, "nSelf").expect("pin");
    let mut source =
        RemoteSource::connect(NODE_NAME, options(port), dir.path()).expect("client thread");
    source.apply(Msg::from(Update::Connected(Hello {
        protocol: VERSION,
        capabilities: vec![],
        node_id: None,
    })));
    assert_eq!(
        source.connection(),
        Connection::NodeChanged {
            was: "nSelf".into(),
            now: None
        }
    );
    // Nothing to trust: no id was named.
    source.trust("nSelf");
    assert!(changed(&source), "{:?}", source.connection());
    linger(&mut source);
    assert!(source.projects().is_empty());
    assert_eq!(
        source.error().as_deref(),
        Some("That machine changed again. Check the new warning.")
    );
    assert_eq!(pin(dir.path()).as_deref(), Some("nSelf"));
}

#[test]
fn a_never_pinned_name_whose_hello_has_no_id_connects_unpinned() {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .expect("addr")
        .port();
    let dir = tempfile::tempdir().expect("dir");
    let mut source =
        RemoteSource::connect(NODE_NAME, options(port), dir.path()).expect("client thread");
    source.apply(Msg::from(Update::Connected(Hello {
        protocol: VERSION,
        capabilities: vec![],
        node_id: None,
    })));
    assert_eq!(source.connection(), Connection::Connected);
    assert_eq!(pin(dir.path()), None);
}

#[test]
fn a_pin_file_that_cant_be_parsed_refuses() {
    let env = start();
    let dir = tempfile::tempdir().expect("dir");
    std::fs::write(dir.path().join(env_store::NODES_FILE), "{not json").expect("write");
    let mut source = connect(&env, dir.path());
    settle(&mut source, "the refusal", |s| {
        matches!(s.connection(), Connection::Unverified(_))
    });
    linger(&mut source);
    assert!(source.projects().is_empty());
    assert_eq!(env.asked(), Vec::<&str>::new());
}

/// Unreadable but replaceable, so only the read can refuse it (a save would succeed).
#[cfg(unix)]
#[test]
fn a_pin_file_that_cant_be_read_refuses() {
    use std::os::unix::fs::PermissionsExt as _;
    let env = start();
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join(env_store::NODES_FILE);
    std::fs::write(&path, "{}").expect("write");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).expect("chmod");
    if std::fs::read(&path).is_ok() {
        eprintln!("skipped: running as root, the file is still readable");
        return;
    }
    let mut source = connect(&env, dir.path());
    settle(&mut source, "the refusal", |s| {
        matches!(s.connection(), Connection::Unverified(_))
    });
    linger(&mut source);
    assert!(source.projects().is_empty());
    assert_eq!(env.asked(), Vec::<&str>::new());
}

#[test]
fn trust_re_pins_only_the_node_shown() {
    let env = start();
    let dir = tempfile::tempdir().expect("dir");
    env_store::trust_node(dir.path(), NODE_NAME, "nOld").expect("pin");
    let mut source = connect(&env, dir.path());
    settle(&mut source, "the warning", changed);

    source.trust("nSomethingElse");
    linger(&mut source);
    assert!(changed(&source), "{:?}", source.connection());
    assert_eq!(pin(dir.path()).as_deref(), Some("nOld"));

    source.trust("nSelf");
    settle(&mut source, "projects", |s| !s.projects().is_empty());
    assert_eq!(source.connection(), Connection::Connected);
    assert_eq!(pin(dir.path()).as_deref(), Some("nSelf"));
}
