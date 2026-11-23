//! Acceptance: diagrams render on a fixed pool of worker threads. A reply with many diagrams queues them instead of
//! starting a thread each, and a render that panics doesn't take its worker with it.

use super::Pool;
use std::collections::HashSet;
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn pool_runs_on_fixed_threads_and_survives_a_panic() {
    let pool = Pool::new(2);
    let (tx, rx) = mpsc::channel();
    for _ in 0..20 {
        let tx = tx.clone();
        pool.run(move || {
            std::thread::sleep(Duration::from_millis(2));
            tx.send(std::thread::current().id()).expect("send");
        });
    }
    let threads: HashSet<_> = (0..20)
        .map(|_| {
            rx.recv_timeout(Duration::from_secs(10))
                .expect("every job ran")
        })
        .collect();
    assert!(threads.len() <= 2, "{} threads for 20 jobs", threads.len());

    for _ in 0..2 {
        pool.run(|| panic!("a render panicked"));
    }
    for _ in 0..4 {
        let tx = tx.clone();
        pool.run(move || tx.send(std::thread::current().id()).expect("send"));
    }
    for _ in 0..4 {
        rx.recv_timeout(Duration::from_secs(10))
            .expect("the workers outlive a panicking job");
    }
}
