//! A delegation request names the caller's account by its label there, cleaned, or by its id without one.

use super::who;
use skyelab_daemon::{DelegationRequest, Origin};

fn request(label: Option<&str>) -> DelegationRequest {
    DelegationRequest {
        origin: Origin {
            node_id: "nDesk".into(),
            device: "desk.tail1.ts.net".into(),
            key_hash: "k".into(),
        },
        account_id: "work".into(),
        account_label: label.map(str::to_owned),
        email: "me@example.com".into(),
        org_id: "org-1".into(),
        provider: "claude".into(),
    }
}

#[test]
fn the_request_names_the_account_by_its_label() {
    assert_eq!(
        who(&request(Some("Work (Acme)"))),
        "Agents on desk under Work (Acme) (me@example.com · org-1)"
    );
    // An older peer sends no label; a label that cleans to nothing says no more than none.
    for label in [None, Some("\u{202E}\u{200B}\n")] {
        assert_eq!(
            who(&request(label)),
            "Agents on desk under work (me@example.com · org-1)"
        );
    }
    // Control and bidi characters go, and the length is bounded like a title.
    let shown = who(&request(Some(&format!(
        "A\u{202E}b\u{0007}{}",
        "x".repeat(500)
    ))));
    assert!(shown.starts_with("Agents on desk under Abx"), "{shown}");
    let kept = format!("Abx{} (", "x".repeat(197));
    assert!(shown.contains(&kept), "{shown}");
}
