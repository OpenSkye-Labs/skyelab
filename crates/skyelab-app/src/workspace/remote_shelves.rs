//! Settle and snooze on another machine's threads (protocol 1.6). The machine that owns the thread is the source of
//! truth: a row here changes only once that machine's store took the change. While it is offline the action is
//! refused with a note, not queued: a queued shelf would land later on a thread that may have moved on since.

use super::super::Workspace;
use super::Shelf;
use crate::environment::{ConnState, RemoteEnv};
use crate::remote::Reply;
use gpui::{Context, Entity, SharedString, Window};
use skyelab_core::snooze;
use skyelab_protocol::{ErrorCode, RpcError};

impl Workspace {
    /// The connection to `host` that lists its threads here: the machine shown, else a linked one.
    fn env_on(&self, host: &str, cx: &gpui::App) -> Option<Entity<RemoteEnv>> {
        self.remote_env()
            .filter(|e| e.read(cx).host.eq_ignore_ascii_case(host))
            .or_else(|| self.linked_envs_of(host, cx).pop())
    }

    /// A row's settle or snooze button on another machine's thread. Settling and snoozing offer an Undo.
    pub(in crate::workspace) fn shelve_on(
        &mut self,
        host: SharedString,
        id: i64,
        to: Shelf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = match to {
            Shelf::Settle => (true, None),
            Shelf::Snooze(until) => (false, Some(until)),
            Shelf::Unsettle | Shelf::Wake | Shelf::SnoozeCustom => (false, None),
        };
        let undo = matches!(to, Shelf::Settle | Shelf::Snooze(_));
        self.shelve_remote(host, id, target, undo, window, cx);
    }

    fn shelve_remote(
        &mut self,
        host: SharedString,
        id: i64,
        (settled, until): (bool, Option<i64>),
        undo: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(env) = self.env_on(&host, cx) else {
            return;
        };
        let e = env.read(cx);
        let Some(t) = e.threads.iter().find(|t| t.id == id) else {
            return;
        };
        let (title, was) = (t.title.clone(), (t.settled_at.is_some(), t.snoozed_until));
        if e.state != ConnState::Connected {
            let note = format!("{host} is offline. Settle and snooze its threads once it is back.");
            return self.say(note, window, cx);
        }
        let pending = e.client().shelve_thread(id, settled, until);
        cx.spawn_in(window, async move |this, cx| {
            let result = pending.recv_async().await;
            let _ = this.update_in(cx, |ws, window, cx| {
                if let Err(e) = result {
                    let why = match e {
                        skyelab_client::Error::Rpc(e) => e.message,
                        e => e.to_string(),
                    };
                    return ws.say(
                        format!("Couldn't change “{title}” on {host}: {why}"),
                        window,
                        cx,
                    );
                }
                env.update(cx, |e, cx| e.set_shelf(id, settled, until, cx));
                if !undo {
                    return;
                }
                let text = match until {
                    Some(until) => format!(
                        "Snoozed “{title}” for {}",
                        snooze::wake_label(until, crate::ui::now_ms())
                    ),
                    None => format!("Settled “{title}”"),
                };
                ws.thread_undo_notice(
                    text,
                    move |ws, window, cx| {
                        ws.shelve_remote(host.clone(), id, was, false, window, cx)
                    },
                    window,
                    cx,
                );
            });
        })
        .detach(); // `this` is weak: a closed window drops the answer
    }

    /// Another machine asked to shelve one of this machine's threads: the same store write and row change as the
    /// sidebar's own, answered once written.
    pub(in crate::workspace) fn remote_shelve(
        &mut self,
        id: i64,
        settled: bool,
        until: Option<i64>,
        reply: Reply<()>,
        cx: &mut Context<Self>,
    ) {
        // Archived threads count: the daemon lists them too.
        if !self.threads.iter().any(|t| t.id == id) {
            let missing = RpcError::new(ErrorCode::NotFound, format!("no thread {id} listed here"));
            let _ = reply.send(Err(missing));
            return;
        }
        self.put_on_shelf(id, settled, until, cx, move |_, done, _| {
            let _ = reply.send(done.map_err(|e| {
                eprintln!("skyelab: shelving thread {id} for another machine: {e:#}");
                crate::remote::internal("store error")
            }));
        });
    }
}

impl RemoteEnv {
    /// The serving machine took a shelve for thread `id`: its row shows it now, and a listing follows so one that was
    /// already in flight (with the old shelf) doesn't win.
    pub fn set_shelf(
        &mut self,
        id: i64,
        settled: bool,
        until: Option<i64>,
        cx: &mut Context<Self>,
    ) {
        if let Some(t) = self.threads.iter_mut().find(|t| t.id == id) {
            t.settled_at = settled.then(crate::ui::now_ms);
            t.snoozed_until = until.filter(|_| !settled);
            cx.notify();
        }
        self.refresh_soon(cx);
    }
}
