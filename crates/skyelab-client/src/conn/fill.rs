//! Filling a watched thread's events from the store: after a gap, a reconnect, or a failed fill.

use skyelab_protocol::{Call, Reply};
use tokio::time::Instant;

use super::{Ctx, End, Live, Waiter};
use crate::Error;

impl Live {
    /// Asks for the events after the thread's last delivered seq, unless a fill is already running.
    pub(super) async fn fill(&mut self, cx: &mut Ctx, thread_id: i64) -> Result<(), End> {
        let Some(w) = cx.watched.get_mut(&thread_id) else {
            return Ok(());
        };
        if w.filling {
            return Ok(());
        }
        w.filling = true;
        let call = Call::ThreadEventsSince {
            thread_id,
            seq: w.last,
        };
        self.request(cx, call, Waiter::Fill(thread_id)).await
    }

    /// One page arrived. Pages while pages make progress; if a gap is left after that (the fill failed, or raced a
    /// push, or the server's store has a hole), retries after `backoff_min`.
    pub(super) async fn filled(
        &mut self,
        cx: &mut Ctx,
        thread_id: i64,
        result: Result<Reply, Error>,
    ) -> Result<(), End> {
        let Some(w) = cx.watched.get_mut(&thread_id) else {
            return Ok(());
        };
        w.filling = false;
        let page = match result {
            Ok(Reply::Events(page)) => page,
            other => {
                eprintln!("skyelab-client: can't fetch events of thread {thread_id}: {other:?}");
                self.mark_stale(cx, thread_id);
                return Ok(());
            }
        };
        let before = w.last;
        let mut gap = !w.ahead.is_empty();
        for ev in page {
            if ev.thread_id == thread_id {
                gap = w.accept(ev, &cx.updates);
            }
        }
        if w.last > before {
            self.retry_delay = cx.options.backoff_min;
            self.fill(cx, thread_id).await
        } else {
            if gap {
                self.mark_stale(cx, thread_id);
            }
            Ok(())
        }
    }

    pub(super) fn mark_stale(&mut self, cx: &mut Ctx, thread_id: i64) {
        if let Some(w) = cx.watched.get_mut(&thread_id) {
            w.stale = true;
            let delay = self.retry_delay;
            self.retry_at.get_or_insert_with(|| Instant::now() + delay);
        }
    }

    pub(super) async fn retry_stale(&mut self, cx: &mut Ctx) -> Result<(), End> {
        self.retry_delay = self
            .retry_delay
            .saturating_mul(2)
            .min(cx.options.backoff_max);
        let stale: Vec<i64> = cx
            .watched
            .iter_mut()
            .filter(|(_, w)| w.stale)
            .map(|(id, w)| {
                w.stale = false;
                *id
            })
            .collect();
        for thread_id in stale {
            self.fill(cx, thread_id).await?;
        }
        Ok(())
    }
}
