//! One running copy per data dir: a second claim reaches the first (which is asked to show a window), the lock goes
//! with the process that held it, and debug builds keep their own dirs.

use skyelab_core::instance::{APP_DIR, Claim, claim};
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn debug_builds_use_their_own_dirs() {
    assert_eq!(APP_DIR, "skyelab-dev", "tests are debug builds");
}

#[test]
fn a_second_claim_asks_the_first_to_show_and_the_lock_goes_with_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let Claim::First(first) = claim(dir.path()).expect("first claim") else {
        panic!("an empty dir was held");
    };
    let (shown, got) = mpsc::channel();
    let first = std::thread::spawn(move || {
        first.wait_for_show().expect("asked");
        shown.send(()).expect("send");
        first // keeps holding until the test drops it
    });
    assert!(matches!(
        claim(dir.path()).expect("second claim"),
        Claim::Running
    ));
    got.recv_timeout(Duration::from_secs(5))
        .expect("the first copy was asked to show a window");
    drop(first.join().expect("join"));
    assert!(
        matches!(claim(dir.path()).expect("claim after"), Claim::First(_)),
        "the lock went with its holder: never stale"
    );
}

#[test]
fn a_wrong_token_is_ignored() {
    use std::io::Write as _;
    let dir = tempfile::tempdir().expect("tempdir");
    let Claim::First(first) = claim(dir.path()).expect("claim") else {
        panic!("held");
    };
    let info = std::fs::read_to_string(dir.path().join("skyelab.instance")).expect("info");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(dir.path().join("skyelab.instance"))
            .expect("meta")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    let (port, token) = info.trim().split_once(' ').expect("port and token");
    let (done, got) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = first.wait_for_show();
        let _ = done.send(());
    });
    let mut bad = std::net::TcpStream::connect(("127.0.0.1", port.parse::<u16>().expect("port")))
        .expect("connect");
    bad.write_all(b"0000 show").expect("write");
    drop(bad);
    assert!(
        got.recv_timeout(Duration::from_millis(500)).is_err(),
        "a wrong token shows nothing"
    );
    let mut good = std::net::TcpStream::connect(("127.0.0.1", port.parse::<u16>().expect("port")))
        .expect("connect");
    good.write_all(format!("{token} show").as_bytes())
        .expect("write");
    drop(good);
    got.recv_timeout(Duration::from_secs(5)).expect("shown");
}

/// A held dir whose address is a crashed run's (nothing listens there) never lets a second copy start.
#[test]
fn a_held_dir_with_a_stale_address_never_starts_a_second_copy() {
    let dir = tempfile::tempdir().expect("tempdir");
    let held = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.path().join("skyelab.lock"))
        .expect("open");
    held.try_lock().expect("hold the lock like a running copy");
    let dead = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = dead.local_addr().expect("addr").port();
    drop(dead);
    std::fs::write(
        dir.path().join("skyelab.instance"),
        format!("{port} deadbeef\n"),
    )
    .expect("write");
    assert!(matches!(
        claim(dir.path()).expect("claim"),
        Claim::Unreachable(_)
    ));
}

/// Another user can't open the lock (and hold it to keep SkyeLab from starting).
#[cfg(unix)]
#[test]
fn the_lock_and_its_dir_are_private() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = tempfile::tempdir().expect("tempdir");
    let data = dir.path().join("data");
    let _held = claim(&data).expect("claim");
    let mode =
        |p: &std::path::Path| std::fs::metadata(p).expect("meta").permissions().mode() & 0o777;
    assert_eq!(mode(&data.join("skyelab.lock")), 0o600);
    assert_eq!(mode(&data), 0o700);
}

/// A peer that trickles bytes is dropped after a short deadline, so a real second launch is still shown.
#[test]
fn a_slow_peer_does_not_hold_back_a_second_launch() {
    use std::io::Write as _;
    let dir = tempfile::tempdir().expect("tempdir");
    let Claim::First(first) = claim(dir.path()).expect("claim") else {
        panic!("held");
    };
    let info = std::fs::read_to_string(dir.path().join("skyelab.instance")).expect("info");
    let port: u16 = info
        .split_whitespace()
        .next()
        .expect("port")
        .parse()
        .expect("a port");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = first.wait_for_show();
        let _ = tx.send(());
    });
    let mut slow = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
    std::thread::spawn(move || {
        for _ in 0..200 {
            if slow.write_all(b"x").is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    });
    std::thread::sleep(Duration::from_millis(200));
    assert!(matches!(claim(dir.path()).expect("claim"), Claim::Running));
    rx.recv_timeout(Duration::from_secs(5))
        .expect("the slow peer held back the show");
}
