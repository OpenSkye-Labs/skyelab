//! Port forwarding from the machine shown (T3's "Forwarded ports" palette items): a typed port is forwarded, its
//! entries open it in a browser tab and stop it, and forwards end with the connection.

use super::env_harness::*;
use super::*;
use gpui::{TestAppContext, VisualTestContext};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};

/// An echo server on 127.0.0.1: the dev server an agent started on the other machine.
fn echo_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let mut conn = conn;
                let mut buf = [0u8; 1024];
                while let Ok(n) = conn.read(&mut buf) {
                    if n == 0 || conn.write_all(&buf[..n]).is_err() {
                        break;
                    }
                }
            });
        }
    });
    port
}

fn entries(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> Vec<crate::palette::Entry> {
    ws.update_in(cx, |ws, window, cx| {
        let focus = gpui::Focusable::focus_handle(ws, cx);
        ws.palette_entries(&focus, window, cx)
    })
}

fn labels(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> Vec<String> {
    entries(ws, cx)
        .into_iter()
        .map(|e| e.label.to_string())
        .filter(|l| {
            [
                "Forward a Port",
                "Open Forwarded",
                "Copy Forwarded",
                "Stop Forwarding",
            ]
            .iter()
            .any(|p| l.starts_with(p))
        })
        .collect()
}

fn run(ws: &Entity<Workspace>, label_starts: &str, cx: &mut VisualTestContext) {
    let entry = entries(ws, cx)
        .into_iter()
        .find(|e| e.label.starts_with(label_starts))
        .unwrap_or_else(|| panic!("no entry {label_starts:?}"));
    cx.update(|window, cx| (entry.run)(window, cx));
}

/// What the palette's search finds for `query`.
fn search(
    ws: &Entity<Workspace>,
    query: &str,
    cx: &mut VisualTestContext,
) -> Vec<crate::palette::Entry> {
    let search = ws
        .read_with(cx, |ws, cx| ws.forward_search(cx))
        .expect("a search while remote");
    let task = cx.update(|_, cx| search(query.to_owned(), cx));
    cx.foreground_executor().block_test(task)
}

fn forwards(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> Vec<(u16, u16)> {
    ws.read_with(cx, |ws, cx| {
        ws.remote_env()
            .map(|e| {
                e.read(cx)
                    .forwards
                    .iter()
                    .map(|f| (f.remote_port, f.local_port))
                    .collect()
            })
            .unwrap_or_default()
    })
}

fn echoes(local: u16) -> bool {
    let Ok(mut tcp) = TcpStream::connect((Ipv4Addr::LOCALHOST, local)) else {
        return false;
    };
    tcp.set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .expect("timeout");
    let mut got = [0u8; 4];
    tcp.write_all(b"ping").is_ok() && tcp.read_exact(&mut got).is_ok() && &got == b"ping"
}

#[gpui::test]
fn forward_open_and_stop_a_port_from_the_palette(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    let dev = echo_server();
    let (ws, _, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    // This Mac: nothing to forward from, and the palette keeps its thread search.
    assert!(labels(&ws, cx).is_empty());
    assert!(ws.read_with(cx, |ws, cx| ws.forward_search(cx)).is_none());

    connect(&ws, &remote, cx);
    let host = skyelab_daemon::testing::NODE_NAME;
    assert_eq!(
        labels(&ws, cx),
        [format!("Forward a Port from {host}: type its number")]
    );
    assert!(search(&ws, "abc", cx).is_empty());
    let found = search(&ws, &dev.to_string(), cx);
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].label.as_ref(),
        format!("Forward Port {dev} from {host}")
    );
    cx.update(|window, cx| (found[0].run)(window, cx));
    wait_until(cx, "the forward", |cx| forwards(&ws, cx).len() == 1);
    let (remote_port, local) = forwards(&ws, cx)[0];
    assert_eq!(remote_port, dev);
    assert!(echoes(local), "bytes reach the dev server there");
    // Already forwarded: the search doesn't offer it again.
    assert!(search(&ws, &dev.to_string(), cx).is_empty());

    let address = format!("127.0.0.1:{local}");
    let listed = labels(&ws, cx);
    for want in [
        format!("Open Forwarded Port: {address} (port {dev} on {host})"),
        format!("Copy Forwarded Address: http://{address}/"),
        format!("Stop Forwarding: {address} (port {dev})"),
    ] {
        assert!(listed.contains(&want), "{want:?} in {listed:?}");
    }

    let before = ws.read_with(cx, |ws, _| ws.browsers.len());
    run(&ws, "Open Forwarded Port", cx);
    cx.run_until_parked();
    let tab = ws.read_with(cx, |ws, _| ws.browsers.clone());
    assert_eq!(tab.len(), before + 1, "a browser tab opened");
    let browser = tab
        .last()
        .and_then(|p| p.upgrade())
        .expect("tab")
        .read_with(cx, |p, _| p.browser.clone());
    let url = browser.read_with(cx, |b, cx| b.url_bar_text(cx));
    assert_eq!(url, format!("http://{address}/"));

    run(&ws, "Stop Forwarding", cx);
    assert!(forwards(&ws, cx).is_empty());
    wait_until(cx, "the local port to close", |_| !echoes(local));
}

#[gpui::test]
fn forwards_end_with_the_connection(cx: &mut TestAppContext) {
    let mut remote = remote_side(FakeTailnet::default());
    let dev = echo_server();
    let (ws, _, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    connect(&ws, &remote, cx);
    let env = ws.read_with(cx, |ws, _| ws.remote_env()).expect("remote");
    env.update(cx, |env, cx| env.forward_port(dev, cx));
    wait_until(cx, "the forward", |cx| forwards(&ws, cx).len() == 1);
    let local = forwards(&ws, cx)[0].1;
    remote.stop();
    wait_until(cx, "the forward to go", |cx| forwards(&ws, cx).is_empty());
    wait_until(cx, "the local port to close", |_| !echoes(local));
    assert!(
        labels(&ws, cx).is_empty(),
        "nothing to forward while disconnected"
    );
    // Back on This Mac after another forward: the switch drops it too.
    remote.serve();
    wait_until(cx, "reconnected", |cx| state(&ws, cx) == "Connected");
    env.update(cx, |env, cx| env.forward_port(dev, cx));
    wait_until(cx, "the forward", |cx| forwards(&ws, cx).len() == 1);
    let local = forwards(&ws, cx)[0].1;
    switch(&ws, None, cx);
    wait_until(cx, "the local port to close", |_| !echoes(local));
}
