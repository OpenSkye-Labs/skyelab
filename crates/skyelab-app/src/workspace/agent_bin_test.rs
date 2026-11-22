use std::path::Path;

#[test]
fn claude_bin_override_is_debug_only() {
    let fake = Some(std::ffi::OsString::from("/tmp/fake-claude"));
    assert_eq!(
        crate::agent_bin(fake.clone(), true, "claude"),
        Path::new("/tmp/fake-claude")
    );
    assert_eq!(crate::agent_bin(fake, false, "claude"), Path::new("claude"));
    assert_eq!(crate::agent_bin(None, true, "codex"), Path::new("codex"));
}
