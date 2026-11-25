use super::*;

fn marked(name: &str, value: &str) -> String {
    format!("{}\n{value}\n{}\n", start_marker(name), end_marker(name))
}

#[test]
fn parse_reads_values_between_markers_and_ignores_rc_chatter() {
    let out = format!(
        "Welcome!\n\n{}\nnoise\n{}{}",
        marked("PATH", "/a/bin:/b bin"),
        marked("LANG", ""),
        marked("JAVA_HOME", "/jdk")
    );
    let got = parse(&out);
    assert_eq!(
        got,
        [
            (OsString::from("PATH"), OsString::from("/a/bin:/b bin")),
            ("JAVA_HOME".into(), "/jdk".into()),
        ],
        "an empty LANG is dropped"
    );
    assert!(parse("no markers").is_empty());
}

/// Only allowlisted names are read: an account's dirs or a token printed with markers never come in.
#[test]
fn parse_keeps_only_allowlisted_names() {
    let out = [
        "CLAUDE_CONFIG_DIR",
        "CODEX_HOME",
        "GH_CONFIG_DIR",
        "ANTHROPIC_API_KEY",
        "PNPM_HOME",
    ]
    .map(|n| marked(n, "/x"))
    .concat();
    assert!(parse(&out).is_empty());
    for n in NAMES {
        assert!(crate::env::terminal_allowed(OsStr::new(n), false), "{n}");
        assert!(!n.contains("CONFIG_DIR") && *n != "CODEX_HOME", "{n}");
    }
}

#[test]
fn merge_puts_the_shell_path_first_and_only_fills_missing_vars() {
    let process = vec![
        (OsString::from("PATH"), OsString::from("/usr/bin:/bin")),
        ("LANG".into(), "C".into()),
    ];
    let captured = [
        (
            OsString::from("PATH"),
            OsString::from("/home/u/.local/bin:/usr/bin"),
        ),
        ("LANG".into(), "en_US.UTF-8".into()),
        ("JAVA_HOME".into(), "/jdk".into()),
    ];
    let got = merge(process.clone(), &captured, &[PathBuf::from("/opt/x")]);
    let get = |k: &str| got.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    assert_eq!(
        get("PATH"),
        Some("/home/u/.local/bin:/usr/bin:/bin:/opt/x".into())
    );
    assert_eq!(
        get("LANG"),
        Some("C".into()),
        "the process's own value wins"
    );
    assert_eq!(get("JAVA_HOME"), Some("/jdk".into()));
    assert_eq!(merge(process.clone(), &[], &[]), process);
}

#[test]
fn fallback_dirs_expand_home_and_skip_missing() {
    let home = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(home.path().join(".local/bin")).expect("mkdir");
    let got = fallback_dirs(Some(home.path()));
    assert!(got.contains(&home.path().join(".local/bin")));
    assert!(!got.contains(&home.path().join(".bun/bin")), "not there");
}

/// Debug builds and tests never start a shell: nothing is captured, no fallback dirs, and waiting returns at once.
#[test]
fn start_in_tests_never_spawns_a_shell() {
    start();
    assert!(!STARTED.load(Ordering::SeqCst));
    let (captured, fallback) = current();
    assert!(captured.is_empty() && fallback.is_empty());
    wait();
}

#[cfg(unix)]
fn fake_shell(dir: &Path, name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt as _;
    let p = dir.join(name);
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).expect("write");
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    p
}

/// A missing shell is skipped, and the next one's PATH comes back; a var it exports outside the allowlist doesn't.
#[cfg(unix)]
#[test]
fn capture_runs_the_login_shell_and_falls_back() {
    let dir = tempfile::tempdir().expect("tempdir");
    let shell = fake_shell(
        dir.path(),
        "sh",
        r#"[ "$1" = -ilc ] || exit 9
export PATH="/captured/bin:$PATH" CLAUDE_CONFIG_DIR=/leak JAVA_HOME=/jdk
echo "rc noise"
exec /bin/sh -c "$2""#,
    );
    let got = capture(
        &[PathBuf::from("/nonexistent/shell"), shell],
        Duration::from_secs(10),
        Duration::from_secs(10),
    );
    let get = |k: &str| got.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    let path = get("PATH").expect("PATH");
    assert!(
        path.to_string_lossy().starts_with("/captured/bin:"),
        "{path:?}"
    );
    assert_eq!(get("JAVA_HOME"), Some("/jdk".into()));
    assert_eq!(get("CLAUDE_CONFIG_DIR"), None);
}

/// A shell that hangs is killed at its limit, retried once with the slow limit, then given up on.
#[cfg(unix)]
#[test]
fn capture_gives_up_on_a_hanging_shell() {
    let dir = tempfile::tempdir().expect("tempdir");
    let count = dir.path().join("count");
    let shell = fake_shell(
        dir.path(),
        "slow",
        &format!("echo x >> '{}'\nexec /bin/sleep 30", count.display()),
    );
    let started = std::time::Instant::now();
    let got = capture(
        &[shell],
        Duration::from_millis(1000),
        Duration::from_millis(1500),
    );
    assert!(got.is_empty());
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );
    let runs = std::fs::read_to_string(&count)
        .expect("ran")
        .lines()
        .count();
    assert_eq!(runs, 2, "the first try and one slow retry");
}
