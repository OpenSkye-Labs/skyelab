//! Requests past their deadline: the caller hears Timeout, a fill is retried later.

use tokio::time::Instant;

use super::{Ctx, Live, Waiter};
use crate::Error;
use skyelab_protocol::RequestId;

impl Live {
    pub(super) fn expire(&mut self, cx: &mut Ctx, now: Instant) {
        let expired: Vec<RequestId> = self
            .pending
            .iter()
            .filter(|(_, (deadline, waiter))| {
                *deadline <= now && !matches!(waiter, Waiter::Forward(..))
            })
            .map(|(id, _)| *id)
            .collect();
        for id in expired {
            if let Some(call) = self.timing.remove(&id) {
                call.done(Some("timed out"));
            }
            match self.pending.remove(&id) {
                Some((_, Waiter::User(reply))) => reply(Err(Error::Timeout)),
                Some((_, Waiter::Fill(thread_id))) => {
                    if let Some(w) = cx.watched.get_mut(&thread_id) {
                        w.filling = false;
                    }
                    self.mark_stale(cx, thread_id);
                }
                Some((_, Waiter::Forward(..))) | None => {}
            }
        }
    }
}
