//! Palette entries for ports forwarded from the machine shown (T3's "Forwarded ports"): forward a typed port
//! number, then open each forward in a browser tab, copy its address, or stop it.

use super::Workspace;
use crate::environment::{ConnState, forwarded_address, forwarded_url};
use crate::palette::{self, Entry, Kind, OpenCommandPalette};
use gpui::{Action as _, App, ClipboardItem, Context, Task};
use std::rc::Rc;

pub(crate) fn typed_port(query: &str) -> Option<u16> {
    let query = query.trim();
    if query.is_empty() || query.len() > 5 || !query.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    query.parse().ok().filter(|&p| p != 0)
}

fn entry(label: String, run: palette::Run) -> Entry {
    Entry::new(label, Kind::Action("skyelab::ForwardPort"), run)
}

impl Workspace {
    /// While another machine is shown: a hint to type a port (or why it can't forward), then each forward's open,
    /// copy and stop.
    pub(super) fn forward_entries(&self, cx: &mut Context<Self>) -> Vec<Entry> {
        let Some(env) = self.remote_env() else {
            return Vec::new();
        };
        let ws = cx.entity().downgrade();
        let read = env.read(cx);
        let mut out = Vec::new();
        let reopen: palette::Run =
            Rc::new(|window, cx| window.dispatch_action(OpenCommandPalette.boxed_clone(), cx));
        if read.forwards_unsupported() {
            out.push(entry(
                format!(
                    "Forward a Port: {} runs an older SkyeLab without port forwarding",
                    read.host
                ),
                Rc::new(|_, _| {}),
            ));
        } else if read.state == ConnState::Connected {
            out.push(entry(
                format!("Forward a Port from {}: type its number", read.host),
                reopen,
            ));
        }
        for forward in &read.forwards {
            let (url, address) = (forwarded_url(forward), forwarded_address(forward));
            let remote = forward.remote_port;
            let (ws, open) = (ws.clone(), url.clone());
            out.push(entry(
                format!(
                    "Open Forwarded Port: {address} (port {remote} on {})",
                    read.host
                ),
                Rc::new(move |window, cx| {
                    _ = ws.update(cx, |ws, cx| ws.open_browser_at(Some(&open), window, cx));
                }),
            ));
            out.push(entry(
                format!("Copy Forwarded Address: {url}"),
                Rc::new(move |_, cx| cx.write_to_clipboard(ClipboardItem::new_string(url.clone()))),
            ));
            let env = env.downgrade();
            out.push(entry(
                format!("Stop Forwarding: {address} (port {remote})"),
                Rc::new(move |_, cx| {
                    _ = env.update(cx, |env, cx| env.stop_forward(remote, cx));
                }),
            ));
        }
        out
    }

    /// While another machine is shown: a typed port number it can forward becomes "Forward Port N from <machine>".
    pub(super) fn forward_search(&self, cx: &App) -> Option<palette::Search> {
        let env = self.remote_env()?;
        let host = env.read(cx).host.clone();
        let env = env.downgrade();
        Some(Rc::new(move |query: String, cx: &mut App| {
            let found = typed_port(&query)
                .filter(|&port| {
                    env.upgrade()
                        .is_some_and(|env| env.read(cx).can_forward(port))
                })
                .map(|port| {
                    let env = env.clone();
                    entry(
                        format!("Forward Port {port} from {host}"),
                        Rc::new(move |_, cx| {
                            _ = env.update(cx, |env, cx| env.forward_port(port, cx));
                        }),
                    )
                });
            Task::ready(found.into_iter().collect())
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::typed_port;

    #[test]
    fn typed_ports() {
        assert_eq!(typed_port(" 5173 "), Some(5173));
        assert_eq!(typed_port("65535"), Some(65535));
        for no in ["", "0", "65536", "123456", "51a", "-1", "+80", "port"] {
            assert_eq!(typed_port(no), None, "{no:?}");
        }
    }
}
