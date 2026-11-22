#![allow(clippy::unwrap_used)]

use skyelab_protocol::{ErrorCode, VERSION, Version, negotiate};

#[test]
fn negotiate_accepts_same_major_any_minor() {
    assert_eq!(negotiate(VERSION).unwrap(), VERSION);
    assert_eq!(
        negotiate(Version {
            major: VERSION.major,
            minor: 99
        })
        .unwrap(),
        VERSION
    );
}

#[test]
fn negotiate_rejects_unknown_major() {
    for major in [0, VERSION.major + 1, u32::MAX] {
        let err = negotiate(Version { major, minor: 0 }).unwrap_err();
        assert_eq!(err.code, ErrorCode::UnsupportedVersion);
    }
}
