//! Gate slots: connections waiting on whois, bounded overall (`max_pending`) and per source address
//! (`max_pending_per_peer`), so one tailnet member flooding the port can't take every slot from the user's devices.
//! A peer whois just refused as someone else's is dropped at accept for [`REFUSED_FOR`], without another whois.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// How long a refused peer is dropped without a whois. A device moved to this user is admitted after it.
pub(crate) const REFUSED_FOR: Duration = Duration::from_secs(30);

pub(crate) struct Slots {
    global: Arc<Semaphore>,
    per_peer: usize,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    busy: HashMap<IpAddr, usize>,
    refused: HashMap<IpAddr, Instant>,
}

/// One connection's slot; freed on drop.
pub(crate) struct Slot {
    slots: Arc<Slots>,
    ip: IpAddr,
    _global: OwnedSemaphorePermit,
}

impl Slots {
    pub(crate) fn new(global: usize, per_peer: usize) -> Arc<Self> {
        Arc::new(Self {
            global: Arc::new(Semaphore::new(global)),
            per_peer,
            state: Mutex::default(),
        })
    }

    fn state(&self) -> MutexGuard<'_, State> {
        // Plain counters: still consistent after a panic elsewhere.
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A slot for a connection from `ip`, or why it is dropped.
    pub(crate) fn take(self: &Arc<Self>, ip: IpAddr, now: Instant) -> Result<Slot, &'static str> {
        let mut state = self.state();
        if state
            .refused
            .get(&ip)
            .is_some_and(|at| now.duration_since(*at) < REFUSED_FOR)
        {
            return Err("refused moments ago");
        }
        if state.busy.get(&ip).is_some_and(|n| *n >= self.per_peer) {
            return Err("too many connections from this address waiting for whois");
        }
        let global = self
            .global
            .clone()
            .try_acquire_owned()
            .map_err(|_| "too many connections waiting for whois")?;
        *state.busy.entry(ip).or_default() += 1;
        Ok(Slot {
            slots: self.clone(),
            ip,
            _global: global,
        })
    }
}

impl Slot {
    /// whois said this peer is not one of the user's devices.
    pub(crate) fn refused(&self, now: Instant) {
        let mut state = self.slots.state();
        // Bounded by the tailnet addresses refused within the window; expired ones go here.
        state
            .refused
            .retain(|_, at| now.duration_since(*at) < REFUSED_FOR);
        state.refused.insert(self.ip, now);
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut state = self.slots.state();
        if let Some(n) = state.busy.get_mut(&self.ip) {
            *n -= 1;
            if *n == 0 {
                state.busy.remove(&self.ip);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    const A: IpAddr = IpAddr::V4(Ipv4Addr::new(100, 64, 0, 1));
    const B: IpAddr = IpAddr::V4(Ipv4Addr::new(100, 64, 0, 2));

    #[test]
    fn one_address_cannot_take_every_slot() {
        let slots = Slots::new(16, 4);
        let now = Instant::now();
        let held: Vec<Slot> = (0..4)
            .map(|_| slots.take(A, now).expect("under cap"))
            .collect();
        assert!(slots.take(A, now).is_err(), "A is at its cap");
        assert!(slots.take(B, now).is_ok(), "B still gets a slot");
        drop(held);
        assert!(slots.take(A, now).is_ok(), "freed on drop");
    }

    #[test]
    fn the_global_cap_still_holds() {
        let slots = Slots::new(2, 4);
        let now = Instant::now();
        let _a = slots.take(A, now).expect("one");
        let _b = slots.take(B, now).expect("two");
        assert!(
            slots
                .take(IpAddr::V4(Ipv4Addr::new(100, 64, 0, 3)), now)
                .is_err()
        );
    }

    #[test]
    fn a_refusal_expires() {
        let slots = Slots::new(16, 4);
        let now = Instant::now();
        slots.take(A, now).expect("slot").refused(now);
        assert!(slots.take(A, now + Duration::from_secs(1)).is_err());
        assert!(slots.take(B, now).is_ok(), "only A is refused");
        assert!(slots.take(A, now + REFUSED_FOR).is_ok(), "expired");
    }
}
