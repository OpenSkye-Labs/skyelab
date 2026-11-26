//! Protocol 1.7 shell stream subscription and < 1.7 legacy listing fallback for [`RemoteSource`].

use std::time::Duration;

use skyelab_client::{Error, ErrorCode, ShellChange, ShellFeed, ShellSnapshot};

use super::{Inbound, RemoteSource};
use crate::source::{Connection, Msg};

/// How long to wait before retrying after a subscribe or list request failed while connected.
const LIST_RETRY: Duration = Duration::from_secs(2);

impl RemoteSource {
    /// Subscribes to the 1.7 shell stream: awaits the snapshot, then forwards each delta until the stream ends.
    pub(super) fn subscribe_shell(&mut self) {
        self.shell_gen = self.shell_gen.wrapping_add(1);
        self.shell_live = false;
        let epoch = self.shell_gen;
        let pending = self.client.subscribe_shell();
        let tx = self.inbox_tx.clone();
        let spawned = std::thread::Builder::new()
            .name("skyelab-mobile-shell".into())
            .spawn(move || match pending.recv() {
                Ok(ShellFeed { snapshot, changes }) => {
                    if tx
                        .send(Msg(Inbound::ShellSnapshot(epoch, snapshot)))
                        .is_err()
                    {
                        return;
                    }
                    for change in changes.iter() {
                        if tx.send(Msg(Inbound::ShellChange(epoch, change))).is_err() {
                            return;
                        }
                    }
                    let _ = tx.send(Msg(Inbound::ShellEnded(epoch)));
                }
                Err(e) => {
                    let _ = tx.send(Msg(Inbound::ShellError(epoch, e)));
                }
            });
        if let Err(e) = spawned {
            self.error = Some(format!("Couldn't start live updates: {e}"));
        }
    }

    /// A subscribe or list request failed: try again shortly, once, rather than wait for the next reconnect.
    pub(super) fn list_failed(&mut self, what: &str, e: Error) {
        self.error = Some(format!("Couldn't list {what}: {e}"));
        if self.retry_pending {
            return;
        }
        self.retry_pending = true;
        let tx = self.inbox_tx.clone();
        let spawned = std::thread::Builder::new()
            .name("skyelab-mobile-retry".into())
            .spawn(move || {
                std::thread::sleep(LIST_RETRY);
                let _ = tx.send(Msg(Inbound::Retry));
            });
        if spawned.is_err() {
            self.retry_pending = false; // the next failure tries again
        }
    }

    /// Legacy (< 1.7): lists the projects; their threads follow once the projects arrive.
    pub(super) fn refresh_legacy(&mut self) {
        self.request(self.client.list_projects(), Inbound::Projects);
    }

    pub(super) fn list_threads(&mut self, project_id: i64) {
        let pending = self.client.list_threads(project_id);
        self.request(pending, move |r| Inbound::Threads(project_id, r));
    }

    pub(super) fn on_shell_snapshot(&mut self, epoch: u64, snapshot: ShellSnapshot) {
        if epoch != self.shell_gen || self.connection != Connection::Connected {
            return;
        }
        self.shell = snapshot;
        self.shell_live = true;
        self.error = None;
    }

    pub(super) fn on_shell_change(&mut self, epoch: u64, change: ShellChange) {
        if epoch != self.shell_gen || self.connection != Connection::Connected {
            return;
        }
        self.shell.seq = change.seq;
        self.shell.apply(change.delta);
        self.error = None;
    }

    pub(super) fn on_shell_ended(&mut self, epoch: u64) {
        if epoch != self.shell_gen || self.connection != Connection::Connected {
            return;
        }
        self.shell_live = false;
        self.subscribe_shell();
    }

    pub(super) fn on_shell_error(&mut self, epoch: u64, e: Error) {
        if epoch != self.shell_gen || self.connection != Connection::Connected {
            return;
        }
        if matches!(&e, Error::Rpc(rpc) if rpc.code == ErrorCode::NotWired) {
            // Server doesn't support 1.7 shell stream: fall back cleanly without error spam.
            self.streaming = false;
            self.shell_live = false;
            self.error = None;
            self.refresh_legacy();
            return;
        }
        self.shell_live = false;
        self.list_failed("threads", e);
    }

    pub(super) fn on_retry(&mut self) {
        self.retry_pending = false;
        // Not connected: the next Connected re-subscribes or re-lists anyway.
        if self.connection != Connection::Connected {
            return;
        }
        if self.streaming {
            if !self.shell_live {
                self.subscribe_shell();
            }
        } else {
            self.refresh_legacy();
        }
    }
}
