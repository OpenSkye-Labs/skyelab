//! This machine read in-process and the same store read over the wire give one shell: the UI can't tell them apart.

mod common;

use std::time::Duration;

use skyelab_daemon::testing::{Env, NODE_NAME, ca};
use skyelab_env::{LocalSource, RemoteSource, Source};

/// A client of `env`'s daemon, connected.
fn client(env: &Env) -> skyelab_client::Client {
    let options = skyelab_client::testing::options(env.port, ca().root());
    let client = skyelab_client::Client::connect(NODE_NAME, options).expect("client");
    match client.updates().recv_timeout(Duration::from_secs(60)) {
        Ok(skyelab_client::Update::Connected(_)) => client,
        other => panic!("expected Connected, got {other:?}"),
    }
}

#[test]
fn same_shell_from_local_and_remote() {
    let env = common::env();
    let local = LocalSource::new(env.backend());
    let remote = RemoteSource::new(client(&env));
    let shell = local.subscribe_shell().expect("local shell").snapshot;
    assert_eq!(shell.projects.len(), 2);
    assert_eq!(shell.threads.len(), 3);
    assert!(
        shell
            .threads
            .iter()
            .any(|t| t.pinned && t.preview.is_some())
    );
    assert!(shell.threads.iter().any(|t| t.settled_at.is_some()));
    let remote = remote.subscribe_shell().expect("remote shell").snapshot;
    assert_eq!(
        (remote.projects, remote.threads),
        (shell.projects, shell.threads)
    );
}

#[test]
fn remote_names_its_environment_by_node_id() {
    let env = common::env();
    let remote = RemoteSource::new(client(&env));
    let node = remote
        .client()
        .hello()
        .and_then(|h| h.node_id)
        .expect("node id");
    assert_eq!(remote.environment().expect("environment").as_str(), node);
}
