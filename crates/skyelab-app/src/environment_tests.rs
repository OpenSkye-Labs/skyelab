//! Connection states from the client's updates.
use super::*;
use skyelab_client::Disconnect;

fn gone(kind: DisconnectKind, message: &str) -> Update {
    Update::Disconnected(Disconnect {
        kind,
        message: message.into(),
    })
}

#[test]
fn states_from_updates() {
    let state = |u| ConnState::from_update(&u);
    // Only the client's typed refusal is a refusal, whatever the text says.
    assert_eq!(
        state(gone(DisconnectKind::Refused, "anything")),
        Some(ConnState::Refused)
    );
    let bad_cert = "TLS with x.ts.net failed: invalid peer certificate: NotValidForName";
    assert_eq!(
        state(gone(DisconnectKind::Certificate, bad_cert)),
        Some(ConnState::Reconnecting(bad_cert.into()))
    );
    let reset = "TLS with x.ts.net failed: Connection reset by peer (os error 54)";
    assert_eq!(
        state(gone(DisconnectKind::Other, reset)),
        Some(ConnState::Reconnecting(reset.into()))
    );
    assert_eq!(
        state(gone(
            DisconnectKind::NotApproved,
            "x hasn't approved (code AB12-CD34)"
        )),
        Some(ConnState::NotApproved(
            "x hasn't approved (code AB12-CD34)".into()
        ))
    );
    assert_eq!(
        state(gone(DisconnectKind::Denied, "x turned this device down")),
        Some(ConnState::Denied)
    );
    assert_eq!(
        state(gone(DisconnectKind::NotYourDevice, "tagged")),
        Some(ConnState::NotYourDevice("tagged".into()))
    );
    assert_eq!(
        state(gone(DisconnectKind::IdentityMismatch, "n1 vs n2")),
        Some(ConnState::IdentityMismatch("n1 vs n2".into()))
    );
    assert!(matches!(
        state(Update::Fatal(Error::UnsupportedVersion("2.0".into()))),
        Some(ConnState::VersionMismatch(_))
    ));
}

#[test]
fn forwarded_pages_are_told_apart() {
    use super::forwards::is_forwarded_page;
    assert!(is_forwarded_page("http://127.0.0.1:3000/", [3000]));
    assert!(is_forwarded_page(
        "http://127.0.0.1:3000/a?b#c",
        [5173, 3000]
    ));
    assert!(
        is_forwarded_page("127.0.0.1:3000/x", [3000]),
        "a bare address, as typed"
    );
    assert!(!is_forwarded_page("http://127.0.0.1:3001/", [3000]));
    assert!(!is_forwarded_page("http://localhost:3000/", [3000]));
    assert!(!is_forwarded_page("http://127.0.0.1:3000/", []));
    assert!(!is_forwarded_page("http://127.0.0.1/", [3000]));
}
