//! A thread whose events add up to more than the client's message limit still streams, page by page, with no
//! reconnect; an event too big for any page arrives as a visible stand-in with its own seq.

use std::sync::Arc;
use std::time::Duration;

use super::*;

fn text_event(len: usize) -> String {
    serde_json::json!({"Assistant": {"content": [{"type": "text", "text": "x".repeat(len)}]}})
        .to_string()
}

#[test]
fn large_events_stream_without_a_reconnect_loop() {
    let env = Env::new();
    for _ in 0..40 {
        env.append(env.thread_id, "agent", &text_event(1 << 20));
    }
    // Over half the client's 16 MiB limit on its own: no page can carry it.
    env.append(env.thread_id, "agent", &text_event(9 << 20));
    env.append(env.thread_id, "agent", &text_event(10));
    let daemon = start_daemon(Arc::new(FakeTailnet::default()), env.backend(), 0);
    let client = Client::connect(HOST, options(port(&daemon))).expect("client");
    connected(&client);
    let _watch = client.watch(env.thread_id, 0);

    let mut got = Vec::new();
    for _ in 0..42 {
        match client.updates().recv_timeout(Duration::from_secs(60)) {
            Ok(Update::Event(ev)) => got.push(ev),
            other => panic!("expected an event, got {other:?}"),
        }
    }
    assert_eq!(
        got.iter().map(|e| e.seq).collect::<Vec<_>>(),
        (1..=42).collect::<Vec<_>>()
    );
    let big = &got[40];
    assert!(
        big.payload_json.len() < 4096 && big.payload_json.contains("too large"),
        "the oversize event is a short stand-in: {}",
        &big.payload_json[..big.payload_json.len().min(200)]
    );
    let shown: serde_json::Value =
        serde_json::from_str(&big.payload_json).expect("stand-in is JSON");
    assert!(
        shown.pointer("/Assistant/content/0/text").is_some(),
        "the stand-in reads as an agent text event: {shown}"
    );
    assert_eq!(got[41].payload_json, text_event(10));
    quiet(&client, Duration::from_millis(500));
}
