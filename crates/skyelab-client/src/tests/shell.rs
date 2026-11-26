//! `subscribe_shell` (1.7) against the real daemon: a snapshot, then live changes; a dropped connection ends the
//! stream, and subscribing after the reconnect replays from a new snapshot that has what changed meanwhile. A
//! machine older than 1.7 is never asked.

use std::net::TcpListener;
use std::time::Duration;

use skyelab_protocol::{Hello, Reply, Request, ServerMessage, ShellDelta, Version};
use skyelab_store::NewThread;
use tungstenite::Message;

use super::*;
use crate::{Error, ErrorCode};

fn delta(feed: &crate::ShellFeed) -> ShellDelta {
    feed.changes
        .recv_timeout(Duration::from_secs(30))
        .expect("a change within 30 s")
        .delta
}

#[test]
fn snapshot_then_live_changes_then_replay_after_a_reconnect() {
    let mut env = Env::start();
    let client = Client::connect(HOST, options(env.port)).expect("client");
    connected(&client);
    let feed = client.subscribe_shell().recv().expect("snapshot");
    assert_eq!(feed.snapshot.threads[0].title, "fix it");

    let started = {
        let store = env.store.lock().expect("lock");
        store
            .create_thread(&NewThread {
                project_id: env.project_id,
                title: "started there",
                cwd: std::path::Path::new("/src/app"),
                account_id: "work",
                status: "running",
            })
            .expect("thread")
            .id
    };
    match delta(&feed) {
        ShellDelta::ThreadUpdated(t) => assert_eq!((t.id, t.status.as_str()), (started, "running")),
        other => panic!("{other:?}"),
    }
    env.store
        .lock()
        .expect("lock")
        .set_thread_status(started, "exited")
        .expect("finish");
    match delta(&feed) {
        ShellDelta::ThreadUpdated(t) => assert_eq!(t.status, "exited", "finished shows at once"),
        other => panic!("{other:?}"),
    }

    env.stop();
    assert!(
        feed.changes.recv_timeout(Duration::from_secs(30)).is_err(),
        "the stream ends with the connection"
    );
    env.store
        .lock()
        .expect("lock")
        .set_thread_title(started, "renamed while away")
        .expect("rename");
    env.serve();
    loop {
        match next(&client) {
            Update::Connected(_) => break,
            Update::Disconnected(_) => continue,
            other => panic!("expected Connected, got {other:?}"),
        }
    }
    let feed = client.subscribe_shell().recv().expect("snapshot again");
    let row = feed
        .snapshot
        .threads
        .iter()
        .find(|t| t.id == started)
        .expect("listed");
    assert_eq!(
        row.title, "renamed while away",
        "the new snapshot has what was missed"
    );
}

/// A 1.6 machine is never sent `subscribe_shell`: the client fails it, so the caller lists the old way.
#[test]
fn an_older_environment_is_not_asked() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let (seen_tx, seen) = flume::unbounded::<String>();
    std::thread::spawn(move || {
        let Ok((stream, _)) = listener.accept() else {
            return;
        };
        let Ok(mut ws) = tungstenite::accept(stream) else {
            return;
        };
        let Ok(Message::Text(hello)) = ws.read() else {
            return;
        };
        let id = serde_json::from_str::<Request>(&hello).expect("hello").id;
        let reply = ServerMessage::Response {
            id,
            result: Reply::Hello(Hello {
                protocol: Version { major: 1, minor: 6 },
                capabilities: vec![],
                node_id: None,
            }),
        };
        ws.send(Message::text(reply.to_json())).expect("send hello");
        while let Ok(msg) = ws.read() {
            if let Message::Text(t) = msg {
                let _ = seen_tx.send(t.to_string());
            }
        }
    });
    let client = Client::connect(
        HOST,
        Options {
            plaintext: true,
            ..options(port)
        },
    )
    .expect("client");
    connected(&client);
    match client.subscribe_shell().recv() {
        Err(Error::Rpc(e)) => {
            assert_eq!(e.code, ErrorCode::NotWired);
            assert!(
                e.message.contains(&format!("update SkyeLab on {HOST}")),
                "{e:?}"
            );
        }
        other => panic!("expected too old, got {:?}", other.map(|f| f.snapshot)),
    }
    assert!(
        seen.recv_timeout(Duration::from_millis(300)).is_err(),
        "nothing was sent"
    );
}

/// Two subscribes answered out of order: the newer one keeps its stream, the older one's ends at once.
#[test]
fn the_newest_subscribe_wins_when_replies_cross() {
    use skyelab_protocol::{ShellChange, ShellSnapshot};
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        let Ok((stream, _)) = listener.accept() else {
            return;
        };
        let Ok(mut ws) = tungstenite::accept(stream) else {
            return;
        };
        let read_id = |ws: &mut tungstenite::WebSocket<std::net::TcpStream>| loop {
            if let Ok(Message::Text(t)) = ws.read() {
                return serde_json::from_str::<Request>(&t).expect("request").id;
            }
        };
        let hello = read_id(&mut ws);
        let reply = |id, result| ServerMessage::Response { id, result }.to_json();
        let hello_reply = Reply::Hello(Hello {
            protocol: skyelab_protocol::VERSION,
            capabilities: vec![],
            node_id: None,
        });
        ws.send(Message::text(reply(hello, hello_reply)))
            .expect("hello");
        let (first, second) = (read_id(&mut ws), read_id(&mut ws));
        let snap = |seq| {
            Reply::Shell(ShellSnapshot {
                seq,
                ..ShellSnapshot::default()
            })
        };
        ws.send(Message::text(reply(second, snap(10))))
            .expect("second");
        ws.send(Message::text(reply(first, snap(5))))
            .expect("first, late");
        let change = ServerMessage::Shell(ShellChange {
            seq: 11,
            delta: ShellDelta::ThreadRemoved { id: 1 },
        });
        ws.send(Message::text(change.to_json())).expect("change");
        while ws.read().is_ok() {}
    });
    let client = Client::connect(
        HOST,
        Options {
            plaintext: true,
            ..options(port)
        },
    )
    .expect("client");
    connected(&client);
    let (older, newer) = (client.subscribe_shell(), client.subscribe_shell());
    let newer = newer.recv().expect("newer snapshot");
    let older = older.recv().expect("older snapshot");
    assert_eq!(
        newer
            .changes
            .recv_timeout(Duration::from_secs(30))
            .map(|c| c.seq),
        Ok(11),
        "the newer stream still delivers"
    );
    assert!(
        older
            .changes
            .recv_timeout(Duration::from_millis(300))
            .is_err()
    );
}
