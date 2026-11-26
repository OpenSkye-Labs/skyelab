//! The connection thread's state between connections: offline commands, watches and where it connects.

use std::sync::PoisonError;

use skyelab_protocol::{Hello, Version};

use super::connect::Target;
use super::watched::Watched;
use super::{Command, Ctx};
use crate::{Error, WatchId};

impl Ctx {
    pub(super) fn set_hello(&self, hello: Option<Hello>) {
        *self.hello.lock().unwrap_or_else(PoisonError::into_inner) = hello;
    }

    /// A command while there is no connection. Returns false once the client is gone.
    pub(super) fn offline(&mut self, command: Result<Command, flume::RecvError>) -> bool {
        match command {
            Err(_) => return false,
            Ok(Command::Call { reply, .. }) => reply(Err(Error::NotConnected)),
            Ok(Command::Watch { id, after_seq }) => self.watch(id, after_seq),
            Ok(Command::Unwatch(id)) => self.unwatch(id),
            Ok(Command::Approve(_)) => {} // for a connection that has gone
            Ok(Command::Forward { reply, .. }) => {
                let _ = reply.send(Err(Error::NotConnected));
            }
        }
        true
    }

    /// Starts the thread over from `after_seq`, keeping its other watches.
    pub(super) fn watch(&mut self, id: WatchId, after_seq: i64) {
        let mut w = Watched::new(after_seq);
        if let Some(old) = self.watched.remove(&id.thread_id) {
            w.holders = old.holders;
        }
        w.holders.insert(id.n);
        self.watched.insert(id.thread_id, w);
    }

    pub(super) fn unwatch(&mut self, id: WatchId) {
        if let Some(w) = self.watched.get_mut(&id.thread_id) {
            w.holders.remove(&id.n);
            if w.holders.is_empty() {
                self.watched.remove(&id.thread_id);
            }
        }
    }

    fn allow_loopback(&self) -> bool {
        #[cfg(any(test, feature = "test-support"))]
        return self.options.allow_loopback;
        #[cfg(not(any(test, feature = "test-support")))]
        false
    }

    fn plaintext(&self) -> bool {
        #[cfg(any(test, feature = "test-support"))]
        return self.options.plaintext;
        #[cfg(not(any(test, feature = "test-support")))]
        false
    }

    fn resolve_to(&self) -> Option<std::net::IpAddr> {
        #[cfg(any(test, feature = "test-support"))]
        return self.options.resolve_to;
        #[cfg(not(any(test, feature = "test-support")))]
        None
    }

    pub(super) fn target(&self) -> Target {
        Target {
            host: self.host.clone(),
            options: self.options.clone(),
            device_key: self.device_key.clone(),
            tls: self.tls.clone(),
            allow_loopback: self.allow_loopback(),
            plaintext: self.plaintext(),
            resolve_to: self.resolve_to(),
            protocol: self.protocol(),
            tunnel: false,
        }
    }

    fn protocol(&self) -> Version {
        #[cfg(any(test, feature = "test-support"))]
        return self.options.protocol;
        #[cfg(not(any(test, feature = "test-support")))]
        skyelab_protocol::VERSION
    }
}
