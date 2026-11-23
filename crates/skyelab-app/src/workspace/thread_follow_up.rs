//! A follow-up to a running agent: stored and sent in order, into the running turn when steered.

use super::{Record, ThreadView};
use crate::workspace::lock;
use gpui::Context;
use skyelab_core::Account;

impl ThreadView {
    /// Sends `text` to the running agent, into its turn with `steer`. `false` when it was refused here (the folder
    /// was rebound); the caller then keeps the prompt.
    pub(in crate::workspace) fn follow_up(
        &mut self,
        text: String,
        attachments: Vec<skyelab_agent::Attachment>,
        steer: bool,
        account: &Account,
        cx: &mut Context<Self>,
    ) -> bool {
        let (Some(session), Some(thread)) = (self.session.clone(), &self.thread) else {
            return false;
        };
        // The running agent keeps the account it started with; a rebound folder needs a new thread.
        if thread.account_id != account.id {
            let msg = format!(
                "This folder is now bound to {}. Start a new thread to run under it.",
                account.label
            );
            self.error = Some(msg.into());
            cx.notify();
            return false;
        }
        let (store, id, parent) = (
            self.backend.store.clone(),
            thread.id,
            thread.parent_thread_id,
        );
        let turns = self.turns.clone();
        let (cwd, key) = (
            thread.cwd.clone(),
            crate::workspace::thread_checkpoints::key(thread),
        );
        let on_sent = self.on_sent.take();
        let unsaved = self.unsaved.clone();
        let shown = super::composer::shown_prompt(&text, &attachments);
        self.keep_images(&shown, &attachments);
        // Claude folds any message sent into a running turn into that turn (one result answers both), so one sent
        // from another device or agent mid-turn is a steer too.
        let joins = self.turn_running() && account.provider == skyelab_core::Provider::Claude;
        let record = if steer || joins {
            Record::Steer(shown)
        } else {
            Record::Prompt(shown)
        };
        self.push(&record, cx);
        // Given back to the composer if the agent refuses it (lesson: a send must never lose what was typed).
        let draft = (text.clone(), attachments.clone());
        // Chained, so quick follow-ups reach the store and claude in the order they were sent.
        let prev = self.send_chain.take();
        let shown_in = self.window;
        let write = cx.background_executor().clone();
        self.send_chain = Some(cx.spawn(async move |this, cx| {
            if let Some(prev) = prev {
                prev.await;
            }
            let sent = write
                .spawn(async move {
                    // An idle child that gets a new turn takes a slot again: claimed with its status write.
                    let (was, idle) = {
                        let store = lock(&store)?;
                        let was = store.thread(id)?.map(|t| t.status).unwrap_or_default();
                        // Stop or an exit got here first: writing "running" would hold a slot with no process.
                        if !skyelab_core::delegation::is_live(&was) {
                            anyhow::bail!("the agent has ended; send again to resume it");
                        }
                        if let Some(parent) = parent
                            && !skyelab_core::delegation::is_busy(&was)
                        {
                            crate::workspace::child_slots::claim(&store, parent, Some(id))?;
                        }
                        store.set_thread_status(id, "running")?;
                        // Counted before it is sent, so a quick result can't find zero and mark the row idle early,
                        // and under the store lock, so a held idle can't land between "running" and the count.
                        let idle = turns.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0;
                        (was, idle)
                    };
                    // Before the agent gets it: what "before this turn" rewinds to. A prompt queued behind a running
                    // turn takes none, or it would overwrite that turn's own starting point.
                    if idle {
                        crate::workspace::thread_checkpoints::before_turn(&cwd, &key);
                    }
                    // Queued first, so a full queue stores no prompt claude never got.
                    let sent = lock(&session).and_then(|mut s| {
                        if steer {
                            s.steer(&text, &attachments)
                        } else {
                            s.send_message(&text, &attachments)
                        }
                    });
                    if let Err(e) = sent {
                        turns.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                        // Only if nothing (a result, Stop) has written since.
                        let _ = lock(&store).and_then(|s| match s.thread(id)? {
                            Some(t) if t.status == "running" => s.set_thread_status(id, &was),
                            _ => Ok(()),
                        });
                        return Err(e);
                    }
                    // Behind any held replies, and held itself if the store refuses it.
                    Ok((
                        unsaved.push_and_save(&store, id, Some(record), None).0,
                        idle,
                    ))
                })
                .await;
            if let Some(reply) = on_sent {
                // Only a limit or an ended agent is the caller's business; store details stay in the tab.
                let _ = reply.send(sent.as_ref().map(|_| ()).map_err(|e| {
                    let why = format!("{e:#}");
                    if why.contains("running children") || why.contains("has ended") {
                        why
                    } else {
                        "the message couldn't be sent; the thread's tab says why".to_owned()
                    }
                }));
            }
            let _ = this.update(cx, |this, cx| match sent {
                Ok((left, idle)) => {
                    // The turn ended before this got there, so it started a new one (a steer too): time that one.
                    if idle {
                        this.turn_started_ms = Some(crate::ui::now_ms());
                        this.tick_while_working(cx);
                    }
                    this.saved(left, cx)
                }
                Err(e) => {
                    this.composer.restore = Some(draft);
                    this.show_error(e, cx)
                }
            });
            // Back into the prompt now if it is empty, else once the user empties it (`restore_draft`).
            let _ = shown_in.update(cx, |_, window, cx| {
                this.update(cx, |this, cx| this.restore_draft(window, cx))
            });
        }));
        true
    }
}
