//! Acceptance: the Agents & accounts page shows Claude Code, Codex and the GitHub CLI with their versions, the found
//! config folders with Add, and Log in on an added account that is signed out.

use super::onboarding::Step;
use super::onboarding_test::rig;
use super::threads_test::wait_until;
use gpui::TestAppContext;

#[gpui::test]
fn agents_page_lists_versions_found_dirs_and_log_in(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx);
    rig.setup.update(cx, |o, cx| o.go(Step::Agents, cx));
    cx.run_until_parked();
    let versions: Vec<_> = rig.setup.read_with(cx, |o, _| {
        o.agents
            .iter()
            .map(|a| (a.name, a.version.clone().and_then(Result::ok)))
            .collect()
    });
    assert_eq!(
        versions,
        [
            ("Claude Code", Some("2.1.0 (Claude Code)".into())),
            ("Codex", Some("codex-cli 0.99.0".into())),
            ("Antigravity", Some("1.5.17".into())),
            ("GitHub CLI", Some("gh version 2.99.0 (fake)".into())),
        ]
    );
    for name in ["Claude Code", "Codex", "Antigravity", "GitHub CLI"] {
        let selector: &'static str = Box::leak(format!("setup-agent-{name}").into_boxed_str());
        assert!(cx.debug_bounds(selector).is_some(), "{name} card drawn");
    }
    rig.add("claude-work", cx);
    wait_until(cx, "the sign-in answer", |cx| cx.debug_bounds("setup-log-in-claude-work").is_some());
}
