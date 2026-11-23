//! A call keeps running on its blocking thread after its connection ends or is revoked. [`CallGate`] lets the
//! backend see that mid-call, so a call that has not acted yet is refused instead of run.

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Cancelled once the connection that made the call is revoked, disconnected or closed.
#[derive(Clone, Default, Debug)]
pub struct CallGate(Arc<AtomicBool>);

thread_local! {
    static CURRENT: RefCell<Option<CallGate>> = const { RefCell::new(None) };
}

impl CallGate {
    /// The gate of the daemon call running on this thread. Outside one, a gate that is never cancelled.
    pub fn current() -> Self {
        CURRENT.with(|c| c.borrow().clone()).unwrap_or_default()
    }

    pub fn cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    pub(crate) fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Runs `f` as this gate's call: [`CallGate::current`] inside it returns this gate.
    pub(crate) fn run<T>(&self, f: impl FnOnce() -> T) -> T {
        /// Clears the thread's gate however `f` ends, since blocking threads are reused.
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                CURRENT.with(|c| c.borrow_mut().take());
            }
        }
        CURRENT.with(|c| *c.borrow_mut() = Some(self.clone()));
        let _reset = Reset;
        f()
    }
}
