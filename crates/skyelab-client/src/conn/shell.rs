//! The shell stream (protocol 1.7) on one connection: the snapshot reply opens it, numbered changes follow.

use skyelab_protocol::{Reply, ShellChange};

use super::Live;
use crate::{Error, ShellFeed};

/// The shell stream this connection delivers: the seq it expects next, and where changes go. Dropped with the
/// connection, on a gap, or by a new subscribe, which ends the subscriber's receiver.
pub(super) struct ShellStream {
    next: u64,
    tx: flume::Sender<ShellChange>,
}

impl Live {
    /// A pushed change: delivered in order; a gap ends the stream, so the subscriber resubscribes.
    pub(super) fn shell_change(&mut self, change: ShellChange) {
        if let Some(stream) = &mut self.shell {
            if change.seq > stream.next {
                self.shell = None; // missed some: ending the stream tells the subscriber to resubscribe
            } else if change.seq == stream.next {
                stream.next += 1;
                if stream.tx.send(change).is_err() {
                    self.shell = None;
                }
            }
        }
    }

    /// The answer to `subscribe_shell`: the caller gets the snapshot and the changes after it.
    pub(super) fn shell_reply(
        &mut self,
        reply: flume::Sender<Result<ShellFeed, Error>>,
        result: Result<Reply, Error>,
    ) {
        let (tx, changes) = flume::unbounded();
        let feed = match result {
            Ok(Reply::Shell(snapshot)) => Ok((snapshot.seq + 1, snapshot)),
            Ok(_) => Err(Error::UnexpectedReply),
            Err(e) => Err(e),
        };
        let next = feed.as_ref().map(|(next, _)| *next).ok();
        let feed = feed.map(|(_, snapshot)| ShellFeed { snapshot, changes });
        // A caller that gave up (a newer subscribe replaced it) gets no stream.
        if reply.send(feed).is_ok()
            && let Some(next) = next
        {
            self.shell = Some(ShellStream { next, tx });
        }
    }
}
