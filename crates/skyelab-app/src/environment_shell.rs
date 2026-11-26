//! The shell stream of a machine on protocol 1.7: a snapshot, then each row change, so its list never polls.

use gpui::Context;
use skyelab_client::Error;

use super::{ConnState, RemoteEnv};

impl RemoteEnv {
    /// Whether this machine streams its list (1.7). An older one is listed again after pushed events instead.
    pub(super) fn streams(&self) -> bool {
        self.speaks(skyelab_protocol::SHELL_SINCE_MINOR)
    }

    /// The machine's projects and threads, then each change as it is written there. A stream that ends while
    /// connected missed a change: subscribe again for a new snapshot (T3's shell resync).
    pub(super) fn subscribe_shell(&mut self, cx: &mut Context<Self>) {
        let pending = self.client.subscribe_shell();
        self._shell = Some(cx.spawn(async move |this, cx| {
            let feed = pending.recv_async().await;
            let feed = match this.update(cx, |this, cx| this.show_snapshot(feed, cx)) {
                Ok(Some(feed)) => feed,
                _ => return,
            };
            while let Ok(change) = feed.changes.recv_async().await {
                let applied = this.update(cx, |this, cx| {
                    let mut shell = skyelab_protocol::ShellSnapshot {
                        seq: change.seq,
                        projects: std::mem::take(&mut this.projects),
                        threads: std::mem::take(&mut this.threads),
                    };
                    shell.apply(change.delta);
                    (this.projects, this.threads) = (shell.projects, shell.threads);
                    this.keep_project();
                    cx.notify();
                });
                if applied.is_err() {
                    return;
                }
            }
            let _ = this.update(cx, |this, cx| {
                if this.state == ConnState::Connected {
                    this.subscribe_shell(cx);
                }
            });
        }));
    }

    /// Shows a subscription's snapshot and hands back its changes; an error stays on screen with the rows shown.
    fn show_snapshot(
        &mut self,
        feed: Result<skyelab_client::ShellFeed, Error>,
        cx: &mut Context<Self>,
    ) -> Option<skyelab_client::ShellFeed> {
        cx.notify();
        match feed {
            Ok(mut feed) => {
                self.error = None;
                self.projects = std::mem::take(&mut feed.snapshot.projects);
                self.threads = std::mem::take(&mut feed.snapshot.threads);
                self.keep_project();
                Some(feed)
            }
            Err(e) => {
                self.error = Some(e.to_string().into());
                None
            }
        }
    }

    /// New thread's project: the one picked, while it is still listed, else the first.
    pub(super) fn keep_project(&mut self) {
        if !self
            .project
            .is_some_and(|id| self.projects.iter().any(|p| p.id == id))
        {
            self.project = self.projects.first().map(|p| p.id);
        }
    }
}
