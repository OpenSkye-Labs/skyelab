//! Listing a machine older than 1.7, which has no shell stream: projects, then each project's threads, again a
//! second after pushed events.

use gpui::Context;

use super::{ConnState, REFRESH_AFTER, RemoteEnv};

impl RemoteEnv {
    /// The projects, then every project's threads.
    pub(super) fn load_projects(&mut self, cx: &mut Context<Self>) {
        let pending = self.client.list_projects();
        self._projects = Some(self.await_reply(pending, cx, |this, projects, cx| {
            this.projects = projects;
            this.keep_project();
            this.got_live(cx);
            this.load_threads(cx);
        }));
    }

    /// The project New thread starts in.
    pub fn select_project(&mut self, id: i64, cx: &mut Context<Self>) {
        self.project = Some(id);
        cx.notify();
    }

    /// Lists every project's threads again within a second, once however many events arrive meanwhile.
    pub(crate) fn refresh_soon(&mut self, cx: &mut Context<Self>) {
        if self._refresh.is_some() || self.streams() {
            return;
        }
        let timer = cx.background_executor().timer(REFRESH_AFTER);
        self._refresh = Some(cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                this._refresh = None;
                this.load_threads(cx);
            });
        }));
    }

    /// Every project's threads, newest first, from a machine older than 1.7 (a streaming one needs no listing).
    /// Only from a trusted connection; `up` loads them once it is. A listing that fails keeps the rows shown and
    /// says why.
    pub fn load_threads(&mut self, cx: &mut Context<Self>) {
        if self.state != ConnState::Connected || self.streams() {
            return;
        }
        let pending: Vec<_> = self
            .projects
            .iter()
            .map(|p| self.client.list_threads(p.id))
            .collect();
        self._threads = Some(cx.spawn(async move |this, cx| {
            let mut all = Vec::new();
            let mut failed = None;
            for p in pending {
                match p.recv_async().await {
                    Ok(threads) => all.extend(threads),
                    Err(e) => failed = Some(e.to_string()),
                }
            }
            let _ = this.update(cx, |this, cx| {
                match failed {
                    Some(e) => this.error = Some(e.into()),
                    None => {
                        all.sort_by_key(|t| std::cmp::Reverse(t.updated_at));
                        this.error = None;
                        this.threads = all;
                        this.got_live(cx);
                    }
                }
                cx.notify();
            });
        }));
    }
}
