#![cfg(unix)]
use std::{collections::HashMap, sync::mpsc, time::Duration};

use skyelab_terminal::{SpawnOptions, Terminal};

#[test]
fn link_at_on_the_bottom_row_returns() {
    let (t, _rx) = Terminal::spawn(SpawnOptions {
        cwd: std::env::temp_dir(),
        command: Some(("/bin/sh".into(), vec!["-c".into(), "sleep 30".into()])),
        env: HashMap::new(),
        cols: 40,
        lines: 10,
    })
    .expect("spawn");
    let (tx, done) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = t.link_at(9, 0);
        let _ = tx.send(());
    });
    assert!(
        done.recv_timeout(Duration::from_secs(5)).is_ok(),
        "link_at hung on the bottom row"
    );
}
