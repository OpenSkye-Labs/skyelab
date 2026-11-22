//! One watched thread's delivery state.

use std::collections::BTreeMap;

use skyelab_protocol::ThreadEvent;

use crate::Update;

/// Limits on the out-of-order pushes kept per thread. Past either, the buffer is emptied; the fill fetches them anyway.
const MAX_AHEAD: usize = 1000;
const MAX_AHEAD_BYTES: usize = 8 << 20;

pub(super) struct Watched {
    /// The last seq delivered.
    pub(super) last: i64,
    pub(super) filling: bool,
    /// A fill ended with the gap still open (an error, a timeout, a race with a push); retry it later.
    pub(super) stale: bool,
    /// Pushed events past a gap, waiting for the fill to reach them.
    pub(super) ahead: BTreeMap<i64, ThreadEvent>,
}

impl Watched {
    pub(super) fn new(last: i64) -> Self {
        Self {
            last,
            filling: false,
            stale: false,
            ahead: BTreeMap::new(),
        }
    }

    /// Delivers `ev` if it is the next seq (then any buffered ones that follow), drops it if already delivered,
    /// buffers it if there is a gap. Every event goes through here, pushed or fetched, so the order can't matter.
    /// Returns true if a gap is left that a fill must close.
    pub(super) fn accept(&mut self, ev: ThreadEvent, updates: &flume::Sender<Update>) -> bool {
        if ev.seq <= self.last {
            return !self.ahead.is_empty();
        }
        // seq > last, so last < i64::MAX and last + 1 can't overflow.
        if ev.seq > self.last + 1 {
            let bytes: usize = self.ahead.values().map(|e| e.payload_json.len()).sum();
            if self.ahead.len() < MAX_AHEAD && bytes + ev.payload_json.len() <= MAX_AHEAD_BYTES {
                self.ahead.insert(ev.seq, ev);
            } else {
                self.ahead.clear();
            }
            return true;
        }
        self.last = ev.seq;
        let _ = updates.send(Update::Event(ev));
        self.ahead = self.ahead.split_off(&self.last.saturating_add(1));
        while let Some(next) = self.ahead.remove(&self.last.saturating_add(1)) {
            self.last = next.seq;
            let _ = updates.send(Update::Event(next));
        }
        !self.ahead.is_empty()
    }
}
