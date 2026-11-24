//! Review findings on project rules: ways a saved rule could allow more than it says.

use serde_json::json;
use skyelab_core::project_rules::{Kind, Rule, matches, propose, suggested_prefix, validate};

fn rule(tool: &str, kind: Kind, pattern: &str) -> Rule {
    Rule {
        tool: tool.into(),
        kind,
        pattern: pattern.into(),
    }
}

#[cfg(unix)]
#[test]
fn a_dangling_symlink_out_of_the_folder_does_not_match() {
    let root = tempfile::tempdir().expect("root");
    let outside = tempfile::tempdir().expect("outside");
    std::fs::create_dir(root.path().join("docs")).expect("docs");
    let link = root.path().join("docs/notes.md");
    std::os::unix::fs::symlink(outside.path().join("not-yet.plist"), &link).expect("link");
    let input = json!({ "file_path": link.to_string_lossy(), "content": "x" });
    let r = rule("Write", Kind::Path, "docs/**");
    assert!(
        !matches(&r, "Write", &input, root.path()),
        "WRITE ESCAPES through a dangling link"
    );
    assert_eq!(
        propose("Write", &input, root.path()),
        None,
        "not offered either"
    );
}

#[test]
fn a_prefix_never_opens_a_shell_or_an_interpreter() {
    for p in [
        "bash -c",
        "sh",
        "/bin/zsh -lc",
        "env",
        "xargs rm",
        "sudo make",
        "python -c",
        "node -e",
        "git -c",
    ] {
        assert!(
            validate(&rule("Bash", Kind::Prefix, p)).is_err(),
            "PREFIX ACCEPTED {p}"
        );
    }
    assert_eq!(
        suggested_prefix("bash -c 'make build'"),
        None,
        "PREFIX SUGGESTED bash -c"
    );
    assert!(
        validate(&rule("Bash", Kind::Exact, "bash -c 'make build'")).is_ok(),
        "exactly is fine"
    );
}

#[test]
fn a_wide_glob_never_covers_repo_internals() {
    let root = tempfile::tempdir().expect("root");
    std::fs::create_dir_all(root.path().join(".git/hooks")).expect("git dir");
    let r = rule("Write", Kind::Path, "**");
    for rel in [
        ".git/config",
        ".git/hooks/pre-commit",
        ".GIT/config",
        "sub/.git/config",
        ".claude/settings.json",
        ".mcp.json",
    ] {
        let p = root.path().join(rel);
        let input = json!({ "file_path": p.to_string_lossy(), "content": "x" });
        assert!(
            !matches(&r, "Write", &input, root.path()),
            "AUTO-ALLOWED {rel}"
        );
    }
    let ok = json!({ "file_path": root.path().join("src/a.rs").to_string_lossy(), "content": "x" });
    assert!(matches(&r, "Write", &ok, root.path()));
}

#[test]
fn a_rule_does_not_lift_the_sandbox_or_take_unknown_fields() {
    let root = tempfile::tempdir().expect("root");
    let r = rule("Bash", Kind::Exact, "npm test");
    let unsandboxed = json!({ "command": "npm test", "dangerouslyDisableSandbox": true });
    assert!(
        !matches(&r, "Bash", &unsandboxed, root.path()),
        "UNSANDBOXED AUTO-ALLOW"
    );
    let odd = json!({ "command": "npm test", "somethingNew": 1 });
    assert!(
        !matches(&r, "Bash", &odd, root.path()),
        "UNKNOWN FIELD AUTO-ALLOW"
    );
    let plain = json!({ "command": "npm test", "description": "d", "timeout": 1000, "run_in_background": false });
    assert!(matches(&r, "Bash", &plain, root.path()));
}

#[test]
fn prefix_words_split_like_the_shell() {
    let root = tempfile::tempdir().expect("root");
    let r = rule("Bash", Kind::Prefix, "npm test");
    for cmd in ["npm\u{a0}test", "npm test\n", "npm\u{2028}test x"] {
        assert!(
            !matches(&r, "Bash", &json!({ "command": cmd }), root.path()),
            "MATCHED {cmd:?}"
        );
    }
}

#[test]
fn a_file_name_with_a_glob_character_is_not_proposed_as_a_glob() {
    let root = tempfile::tempdir().expect("root");
    let f = root.path().join("docs/draft*.md");
    assert_eq!(
        propose(
            "Write",
            &json!({ "file_path": f.to_string_lossy() }),
            root.path()
        ),
        None
    );
}

#[test]
fn many_stars_do_not_hang() {
    let root = tempfile::tempdir().expect("root");
    let r = rule("Write", Kind::Path, "*a*a*a*a*a*a*a*a*a*a*a*a*b");
    let name = format!("{}.rs", "a".repeat(60));
    let input = json!({ "file_path": root.path().join(name).to_string_lossy() });
    let started = std::time::Instant::now();
    assert!(!matches(&r, "Write", &input, root.path()));
    assert!(
        started.elapsed().as_secs() < 1,
        "GLOB TOOK {:?}",
        started.elapsed()
    );
}
