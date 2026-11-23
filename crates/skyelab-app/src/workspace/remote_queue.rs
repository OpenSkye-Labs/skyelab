//! Remote starts held until the first project load finishes (moved unchanged from remote.rs).

use gpui::Task;
use std::time::Duration;

/// How long a remote start waits for the first project load before it is resolved anyway.
pub(super) const FIRST_LOAD_WAIT: Duration = Duration::from_secs(10);
/// Remote starts held while the first project load runs; more are refused.
pub(super) const MAX_WAITING_STARTS: usize = 16;

/// A start held until the first project load ends.
pub(super) struct HeldStart {
    pub(super) project_id: i64,
    pub(super) prompt: String,
    pub(super) reply: crate::remote::Reply<i64>,
    /// The sharing session it arrived in.
    pub(super) session: Option<u64>,
    pub(super) gate: skyelab_daemon::CallGate,
}

impl HeldStart {
    /// Its client stopped waiting, or its device was cut off.
    pub(super) fn abandoned(&self) -> bool {
        self.reply.is_disconnected() || self.gate.cancelled()
    }
}

/// Remote starts that arrived before the first project load finished.
pub(super) struct StartQueue {
    /// Dropped when the first load ends (or the wait times out), which wakes `wait`.
    pub(super) loading: Option<flume::Sender<()>>,
    pub(super) wait: flume::Receiver<()>,
    pub(super) calls: Vec<HeldStart>,
    pub(super) _drain: Option<Task<()>>,
    pub(super) _timeout: Option<Task<()>>,
}

impl StartQueue {
    pub(super) fn new() -> Self {
        let (loading, wait) = flume::bounded(1);
        Self {
            loading: Some(loading),
            wait,
            calls: Vec::new(),
            _drain: None,
            _timeout: None,
        }
    }

    pub(super) fn loaded(&mut self) {
        self.loading = None;
    }
}
