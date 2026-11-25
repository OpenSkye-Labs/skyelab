//! `shelve_thread` (1.6) against the real daemon: the serving machine's store changes and its listing shows it; a
//! machine older than 1.6 is never asked, and says to update SkyeLab there.

use std::net::TcpListener;

use skyelab_daemon::testing::Ran;
use skyelab_protocol::{Hello, Reply, Request, ServerMessage, Version};
use tungstenite::Message;

use super::*;
use crate::{Error, ErrorCode};

#[test]
fn settle_snooze_and_back_reach_the_serving_store() {
    let env = Env::start();
    let client = Client::connect(HOST, options(env.port)).expect("client");
    connected(&client);
    let id = env.thread_id;
    let shelf = || {
        let t = client.list_threads(env.project_id).recv().expect("threads");
        (t[0].settled_at.is_some(), t[0].snoozed_until)
    };

    client
        .shelve_thread(id, true, None)
        .recv()
        .expect("settled");
    assert_eq!(shelf(), (true, None));
    client
        .shelve_thread(id, false, Some(99))
        .recv()
        .expect("snoozed");
    assert_eq!(shelf(), (false, Some(99)), "snoozing clears the settle");
    client.shelve_thread(id, false, None).recv().expect("back");
    assert_eq!(shelf(), (false, None));
    assert_eq!(
        env.ran(),
        [
            Ran::Shelve(id, true, None),
            Ran::Shelve(id, false, Some(99)),
            Ran::Shelve(id, false, None)
        ]
    );

    // Both at once is refused before the app is asked.
    match client.shelve_thread(id, true, Some(5)).recv() {
        Err(Error::Rpc(e)) => assert_eq!(e.code, ErrorCode::BadRequest),
        other => panic!("expected a bad request, got {other:?}"),
    }
    assert_eq!(env.ran().len(), 3);
}

/// A 1.5 machine is never sent `shelve_thread`: the client fails it, naming the machine to update.
#[test]
fn an_older_environment_is_told_to_update() {
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
                protocol: Version { major: 1, minor: 5 },
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
    match client.shelve_thread(1, true, None).recv() {
        Err(Error::Rpc(e)) => {
            assert_eq!(e.code, ErrorCode::NotWired);
            assert!(
                e.message.contains(&format!("update SkyeLab on {HOST}")),
                "{e:?}"
            );
        }
        other => panic!("expected too old, got {other:?}"),
    }
    assert!(
        seen.recv_timeout(Duration::from_millis(300)).is_err(),
        "nothing was sent"
    );
}
