//! A node id that isn't a StableID is the server's arbitrary text: never pinned, shown or trusted.

pub mod common;

use common::{NODE_NAME, options};
use skyelab_client::Update;
use skyelab_mobile::{
    env_store,
    remote::RemoteSource,
    source::{Connection, Msg, ThreadSource},
};
use skyelab_protocol::{Hello, VERSION};

#[test]
fn an_over_long_node_id_is_not_pinned() {
    // Nothing listens there; only the hello below arrives.
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
        node_id: Some("n".repeat(1 << 20)),
    })));
    let pinned = env_store::load_nodes(dir.path())
        .expect("readable pins")
        .get(NODE_NAME)
        .map(String::len);
    assert!(
        pinned.is_none_or(|n| n <= 64),
        "pinned an over-long node id ({pinned:?} bytes)"
    );
    assert_ne!(
        source.connection(),
        Connection::Connected,
        "connected to an over-long node id"
    );
}

#[test]
fn an_empty_or_odd_node_id_is_refused_even_against_a_pin() {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .expect("addr")
        .port();
    let dir = tempfile::tempdir().expect("dir");
    env_store::trust_node(dir.path(), NODE_NAME, "nOld").expect("pin");
    for id in ["", "n\u{202e}Self", "n Self"] {
        let mut source =
            RemoteSource::connect(NODE_NAME, options(port), dir.path()).expect("client thread");
        source.apply(Msg::from(Update::Connected(Hello {
            protocol: VERSION,
            capabilities: vec![],
            node_id: Some(id.into()),
        })));
        assert!(
            matches!(source.connection(), Connection::Unverified(_)),
            "{id:?}: {:?}",
            source.connection()
        );
    }
}
