//! Messages sent while the agent works (T3's follow-up behavior): Send queues them for after the turn, Steer
//! sends one into the running turn now. A slash command always waits, since steering it would replace the task.

use skyelab_agent::Attachment;
use std::collections::VecDeque;

/// What a send does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SendMode {
    /// No turn runs: send it.
    Now,
    /// Hold it until the running turn ends.
    Queue,
    /// Into the running turn.
    Steer,
}

/// `steer` is the user's ask (⌘⇧⏎ or the Steer button); `running` is whether a turn runs now.
pub(crate) fn mode(running: bool, steer: bool, text: &str) -> SendMode {
    match (running, steer) {
        (false, _) => SendMode::Now,
        (true, true) if !text.trim_start().starts_with('/') => SendMode::Steer,
        (true, _) => SendMode::Queue,
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Queued {
    pub id: u64,
    pub text: String,
    pub attachments: Vec<Attachment>,
}

/// The waiting messages, oldest first.
#[derive(Debug, Default)]
pub(crate) struct Queue {
    items: VecDeque<Queued>,
    next_id: u64,
}

impl Queue {
    pub(crate) fn push(&mut self, text: String, attachments: Vec<Attachment>) -> u64 {
        self.next_id += 1;
        self.items.push_back(Queued {
            id: self.next_id,
            text,
            attachments,
        });
        self.next_id
    }

    /// Takes message `id` out (to steer it, or because it was removed).
    pub(crate) fn take(&mut self, id: u64) -> Option<Queued> {
        let ix = self.items.iter().position(|q| q.id == id)?;
        self.items.remove(ix)
    }

    /// The next message once a turn ended.
    pub(crate) fn pop(&mut self) -> Option<Queued> {
        self.items.pop_front()
    }

    /// Puts back a message whose send was refused, first in line again.
    pub(crate) fn put_back(&mut self, q: Queued) {
        self.items.push_front(q);
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &Queued> {
        self.items.iter()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}
