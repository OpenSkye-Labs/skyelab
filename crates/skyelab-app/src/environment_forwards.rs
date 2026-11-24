//! Ports forwarded from the machine shown (protocol 1.5). They last while the connection does: a disconnect, a
//! refusal, a switch to another machine or the window closing drops them, which closes their listeners.

use gpui::Context;
use skyelab_client::Forward;
use skyelab_protocol::FORWARDS_SINCE_MINOR;

use super::{ConnState, EnvEvent, RemoteEnv};

/// `127.0.0.1:<port>`: the client owns that socket. `localhost` could reach whatever holds ::1 on the port.
pub fn forwarded_address(forward: &Forward) -> String {
    format!("127.0.0.1:{}", forward.local_port)
}

pub fn forwarded_url(forward: &Forward) -> String {
    format!("http://{}/", forwarded_address(forward))
}

impl RemoteEnv {
    fn connected(&self) -> bool {
        self.state == ConnState::Connected
    }

    /// Connected to a machine whose SkyeLab predates port forwarding.
    pub fn forwards_unsupported(&self) -> bool {
        self.connected() && !self.speaks(FORWARDS_SINCE_MINOR)
    }

    /// Whether `port` can be forwarded now: connected to a machine that knows forwarding, and not forwarded or
    /// being forwarded already.
    pub fn can_forward(&self, port: u16) -> bool {
        self.connected()
            && self.speaks(FORWARDS_SINCE_MINOR)
            && !self.forwards.iter().any(|f| f.remote_port == port)
            && !self.pending_forwards.contains_key(&port)
    }

    /// Forwards `port` from the machine shown. Says how it went with an [`EnvEvent::Note`]; does nothing when
    /// [`Self::can_forward`] says no.
    pub fn forward_port(&mut self, port: u16, cx: &mut Context<Self>) {
        if !self.can_forward(port) {
            return;
        }
        let pending = self.client.forward_port(port);
        let host = self.host.clone();
        let task = cx.spawn(async move |this, cx| {
            let result = pending.recv_async().await;
            let _ = this.update(cx, |this, cx| {
                this.pending_forwards.remove(&port);
                let note = match result {
                    Ok(forward) => {
                        let note = format!(
                            "Forwarded port {port} from {host}: open {} on this machine",
                            forwarded_url(&forward)
                        );
                        this.forwards.push(forward);
                        note
                    }
                    Err(e) => format!("Couldn't forward port {port} from {host}: {e}"),
                };
                cx.emit(EnvEvent::Note(note.into()));
                cx.notify();
            });
        });
        self.pending_forwards.insert(port, task);
    }

    pub fn stop_forward(&mut self, remote_port: u16, cx: &mut Context<Self>) {
        self.forwards.retain(|f| f.remote_port != remote_port);
        cx.notify();
    }

    /// Ends every forward, and drops (cancels) every one being set up. Callers that leave the machine call it
    /// themselves: the entity can outlive the switch or the window, and its listeners must not.
    pub(crate) fn drop_forwards(&mut self) {
        self.forwards.clear();
        self.pending_forwards.clear();
    }
}
