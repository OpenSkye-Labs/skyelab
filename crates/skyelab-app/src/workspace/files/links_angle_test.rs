//! Outside-folder markdown links with CommonMark `<…>` destinations are unwrapped like bare ones.

use super::linkify;
use std::path::PathBuf;

fn root() -> PathBuf {
    if cfg!(windows) {
        "C:/repo".into()
    } else {
        "/repo".into()
    }
}

#[test]
fn angle_bracket_outside_links_lose_their_link() {
    let r = root();
    assert_eq!(
        linkify("read [the key](</etc/ssh/key>) now", &r),
        "read the key now"
    );
    assert_eq!(linkify("[up](<../x.rs:2>)", &r), "up");
}
