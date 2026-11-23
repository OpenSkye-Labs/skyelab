//! Acceptance: the composer's model and effort reach the agent. Fakes only: a recorder stands in for `claude`, the
//! codex fake echoes every request it reads.
#![cfg(unix)]

#[path = "common/composer.rs"]
mod composer;

use composer::{bound, codex_fake, codex_requests, launch, lines_of, recorder};
use serde_json::Value;
use skyelab_agent::start_with_bin;
use skyelab_core::Provider;

#[test]
fn model_and_effort_reach_claude_args() {
    let f = bound(Provider::Claude);
    let bin = recorder(&f);
    let _s = start_with_bin(
        &bin,
        &f.config,
        &launch(&f, Some("opus"), Some("high"), &[]),
    )
    .expect("start");
    let args = lines_of(&f, "args.txt", 1);
    let at = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .map(|i| args[i + 1].as_str())
    };
    assert_eq!(at("--model"), Some("opus"));
    assert_eq!(at("--effort"), Some("high"));
}

#[test]
fn no_pick_passes_no_model_flags() {
    let f = bound(Provider::Claude);
    let bin = recorder(&f);
    let _s = start_with_bin(&bin, &f.config, &launch(&f, None, None, &[])).expect("start");
    let args = lines_of(&f, "args.txt", 1);
    assert!(
        !args.iter().any(|a| a == "--model" || a == "--effort"),
        "{args:?}"
    );
}

#[test]
fn a_flag_like_model_is_refused() {
    let f = bound(Provider::Claude);
    let bin = recorder(&f);
    let err = start_with_bin(&bin, &f.config, &launch(&f, Some("--x"), None, &[]))
        .err()
        .expect("refused");
    assert!(format!("{err:#}").contains("refusing model"));
    assert!(!f.dir.join("args.txt").exists(), "nothing started");
}

#[test]
fn codex_turn_carries_model_and_effort() {
    let f = bound(Provider::Codex);
    let mut s = start_with_bin(
        &codex_fake(),
        &f.config,
        &launch(&f, Some("gpt-5.5"), Some("high"), &[]),
    )
    .expect("start");
    let start = codex_requests(&s, "thread/start", 1);
    assert_eq!(start[0]["model"], "gpt-5.5");
    let turn = codex_requests(&s, "turn/start", 1);
    assert_eq!(turn[0]["model"], "gpt-5.5");
    assert_eq!(turn[0]["effort"], "high");
    // A new pick applies to the next turn.
    s.set_model(Some("gpt-5.5-mini"), Some("low"))
        .expect("pick");
    s.send_user_message("again").expect("send");
    let turn = codex_requests(&s, "turn/start", 1);
    assert_eq!(turn[0]["model"], "gpt-5.5-mini");
    assert_eq!(turn[0]["effort"], "low");
}

#[test]
fn claude_set_model_sends_a_control_request() {
    let f = bound(Provider::Claude);
    let bin = recorder(&f);
    let mut s = start_with_bin(&bin, &f.config, &launch(&f, None, None, &[])).expect("start");
    s.set_model(Some("sonnet"), None).expect("pick");
    let lines = lines_of(&f, "stdin.txt", 2);
    let v: Value = serde_json::from_str(&lines[1]).expect("json");
    assert_eq!(v["type"], "control_request");
    assert_eq!(v["request"]["subtype"], "set_model");
    assert_eq!(v["request"]["model"], "sonnet");
}

#[test]
fn codex_lists_its_models_under_the_bound_account() {
    let f = bound(Provider::Codex);
    let models = skyelab_agent::list_codex_models(&codex_fake(), &f.config, &f.account, &f.dir)
        .expect("list");
    assert_eq!(models[0].id, "gpt-fake");
    assert_eq!(models[0].efforts, ["low", "medium", "high"]);
    // An unbound folder lists nothing.
    let other = f.dir.parent().expect("root").join("other");
    std::fs::create_dir(&other).expect("mkdir");
    assert!(
        skyelab_agent::list_codex_models(&codex_fake(), &f.config, &f.account, &other).is_err()
    );
}
