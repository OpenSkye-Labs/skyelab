//! Debug mode's switches, redaction, log and live counters (docs/debugging.md).

use skyelab_core::debug::{self, Entry, Kind, Live, Switches};
use std::ffi::OsString;

fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let pairs: Vec<(String, String)> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    move |k| pairs.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone())
}

#[test]
fn switches_parse_truthy_values_and_trace_path() {
    assert_eq!(Switches::parse(env(&[])), Switches::default());
    for on in ["1", "true", "ON", " yes "] {
        let s = Switches::parse(env(&[("SKYELAB_DEBUG", on)]));
        assert!(s.debug, "{on:?} turns debug on");
    }
    for off in ["0", "", "false", "nope"] {
        assert!(
            !Switches::parse(env(&[("SKYELAB_DEBUG", off)])).debug,
            "{off:?}"
        );
    }
    let s = Switches::parse(env(&[
        ("SKYELAB_TRACE", "/tmp/t.json"),
        ("SKYELAB_DEBUG_RENDERS", "1"),
    ]));
    assert_eq!(
        s.trace.as_deref(),
        Some(std::path::Path::new("/tmp/t.json"))
    );
    assert!(s.renders && !s.debug);
    assert_eq!(Switches::parse(env(&[("SKYELAB_TRACE", " ")])).trace, None);
}

fn args(list: &[&str]) -> Vec<OsString> {
    list.iter().map(OsString::from).collect()
}

#[test]
fn redaction_hides_secret_flags_tokens_and_url_credentials() {
    let out = debug::redact_args(&args(&[
        "api",
        "--token",
        "abc123",
        "--password=hunter2",
        "-H",
        "Authorization: Bearer xyz",
        "-c",
        "http.extraHeader=Authorization: Basic Zm9v",
        "ghp_0123456789abcdefghijklmnopqrstuvwxyz",
        "https://user:pa55@github.com/o/r.git?access_token=t0k",
        "commit",
        "-m",
        "the agent's commit message",
        "--body",
        "PR text",
        "status",
    ]));
    let joined = out.join(" ");
    for secret in [
        "abc123",
        "hunter2",
        "xyz",
        "Zm9v",
        "ghp_0123",
        "pa55",
        "t0k",
        "commit message",
        "PR text",
    ] {
        assert!(!joined.contains(secret), "{secret} leaked: {joined}");
    }
    assert_eq!(out[0], "api");
    assert_eq!(out[1], "--token");
    assert_eq!(out[3], "--password=<redacted>");
    assert_eq!(out[7], "http.extraHeader=<redacted>");
    assert_eq!(out[9], "https://<redacted>@github.com/o/r.git?<redacted>");
    assert_eq!(out.last().map(String::as_str), Some("status"));
}

#[test]
fn redaction_shortens_long_values_and_keeps_plain_args() {
    let long = "x".repeat(500);
    let out = debug::redact_args(&args(&["status", "--porcelain=v2", "-z", &long]));
    assert_eq!(out[..3], ["status", "--porcelain=v2", "-z"]);
    assert_eq!(out[3], "<500 chars>");
    assert_eq!(
        debug::redact_text("failed: sk-ant-abc123 at https://a:b@h/x"),
        "failed: <redacted> at https://<redacted>@h/x"
    );
}

#[test]
fn the_log_records_only_while_enabled_and_keeps_order() {
    // One test owns the global switch, so parallel tests can't race it.
    debug::set_enabled(false);
    debug::record(|| Entry::new(Kind::Rpc, "client list_projects"));
    assert!(
        !debug::recent(50)
            .iter()
            .any(|e| e.what == "client list_projects"),
        "off records nothing"
    );
    let dir = tempfile::tempdir().expect("temp dir");
    let file = dir.path().join("debug.log");
    debug::set_log_file(&file).expect("log file opens");
    debug::set_enabled(true);
    debug::record(|| {
        Entry::new(Kind::Rpc, "client list_threads")
            .ms(std::time::Duration::from_millis(12))
            .detail("312 B")
    });
    debug::record(|| Entry::new(Kind::Mcp, "start_thread").failed("refused"));
    let recent = debug::recent(1000);
    let a = recent
        .iter()
        .position(|e| e.what == "client list_threads")
        .expect("rpc entry");
    let b = recent
        .iter()
        .position(|e| e.what == "start_thread")
        .expect("mcp entry");
    assert!(a < b, "oldest first");
    assert_eq!(recent[a].ms, Some(12));
    assert!(recent[a].ok && !recent[b].ok);
    let text = std::fs::read_to_string(&file).expect("log written");
    let line: serde_json::Value = serde_json::from_str(
        text.lines()
            .find(|l| l.contains("list_threads"))
            .expect("a json line"),
    )
    .expect("json");
    assert_eq!(line["kind"], "rpc");
    assert_eq!(line["ms"], 12);
    assert_eq!(line["detail"], "312 B");
    assert!(line["at_ms"].as_u64().is_some_and(|t| t > 0));

    // A spawned child: program and redacted args, never its env.
    #[cfg(unix)]
    child_is_logged_redacted(&file);
    #[cfg(unix)]
    a_timed_out_child_gets_an_end_entry();
    debug::set_enabled(false);
}

#[cfg(unix)]
fn child_is_logged_redacted(file: &std::path::Path) {
    let mut cmd = std::process::Command::new("/bin/echo");
    cmd.arg("--token")
        .arg("s3cret")
        .env("GH_TOKEN", "envsecret");
    let child = skyelab_core::process::output(&mut cmd, None).expect("echo runs");
    assert!(child.status.success());
    let text = std::fs::read_to_string(file).expect("log written");
    assert!(text.contains("echo --token <redacted>"), "{text}");
    assert!(
        text.contains("\"exit 0\""),
        "the exit is logged with its code: {text}"
    );
    assert!(!text.contains("s3cret") && !text.contains("envsecret"));
}

#[test]
fn live_counts_follow_creation_and_drop() {
    let count = || {
        debug::live_counts()
            .into_iter()
            .find(|(k, _)| *k == "test-live-kind")
            .map_or(0, |(_, n)| n)
    };
    let a = Live::new("test-live-kind");
    let b = Live::new("test-live-kind");
    assert_eq!(count(), 2);
    drop(a);
    assert_eq!(count(), 1);
    drop(b);
    assert_eq!(count(), 0);
}

#[test]
fn rpc_method_reads_the_method_name_only() {
    assert_eq!(
        debug::rpc_method(r#"{"id":3,"method":"send_message","params":{"text":"secret prompt"}}"#),
        "send_message"
    );
    assert_eq!(debug::rpc_method("not json"), "?");
}

#[test]
fn review_fixes_redact_typed_text_config_keys_fragments_and_titles() {
    let out = debug::redact_args(&args(&[
        "-s",
        "emulator-5554",
        "shell",
        "input",
        "text",
        "'Hunter2%sPa55w0rd!'",
        "--config",
        "model_providers.x.base_url=https://u:p@h/v1",
        "--config=k=secretvalue",
        "-c",
        "http.https://u:ghp_abcdefghij@h/.extraHeader=Authorization: x",
        "--title=My private PR title",
        "https://h/cb#access_token=t0k3n",
    ]))
    .join(" ");
    for secret in [
        "Hunter2",
        "u:p@",
        "secretvalue",
        "ghp_abc",
        "private PR",
        "t0k3n",
    ] {
        assert!(!out.contains(secret), "{secret} leaked: {out}");
    }
    assert!(out.contains("input text <redacted>"), "{out}");
    assert!(out.contains("https://h/cb#<redacted>"), "{out}");
}

#[test]
fn tailscale_app_cli_and_fake_are_tagged_tailnet() {
    for program in [
        "/Applications/Tailscale.app/Contents/MacOS/Tailscale",
        "/x/fake-tailscale.sh",
        "tailscale",
    ] {
        let cmd = std::process::Command::new(program);
        assert_eq!(debug::program_kind(&cmd), Kind::Tailnet, "{program}");
    }
    assert_eq!(
        debug::program_kind(&std::process::Command::new("git")),
        Kind::Process
    );
}

/// Called from the test that owns the global switch, while it is on.
#[cfg(unix)]
fn a_timed_out_child_gets_an_end_entry() {
    let _ = skyelab_core::process::output_within(
        std::process::Command::new("/bin/sleep").arg("31"),
        None,
        std::time::Duration::from_millis(200),
    );
    let ended = debug::recent(1000)
        .into_iter()
        .any(|e| e.what == "sleep 31" && e.ms.is_some() && !e.ok);
    assert!(ended, "timed-out child has no end entry");
}
