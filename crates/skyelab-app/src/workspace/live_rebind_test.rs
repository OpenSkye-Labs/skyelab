//! A send to a still-running agent re-checks the folder's binding. Rebound or unbound since the agent started, the
//! message never reaches it: the agent ends and the thread shows "This thread can't continue here", with the other
//! provider's Continue button when the folder is now bound there.

use super::composer_review_test::{account, rebind, send, sent, settle, slow_claude, view_with};
use super::thread::ThreadView;
use crate::workspace::{AccountsFile, CodexBin};
use gpui::{Entity, TestAppContext, VisualTestContext};
use skyelab_core::{AccountsConfig, Provider};
use std::path::Path;

fn running(view: &Entity<ThreadView>, cx: &mut VisualTestContext) -> bool {
    view.read_with(cx, |t, _| {
        t.session.is_some() && !t.starting && !t.turn_running()
    })
}

/// Waits for the refusal: the agent gone and the notice set. Returns the notice.
fn refused(view: &Entity<ThreadView>, cx: &mut VisualTestContext) -> String {
    settle(cx, |cx| {
        view.read_with(cx, |t, _| t.session.is_none() && t.resume_refused.is_some())
    });
    view.read_with(cx, |t, cx| {
        assert!(t.session.is_none(), "the old agent was ended");
        assert_eq!(
            t.prompt.read(cx).value().as_ref(),
            "two",
            "the text is kept"
        );
        t.resume_refused
            .as_ref()
            .map(|r| r.to_string())
            .expect("the can't-continue notice")
    })
}

fn cta_shown(view: &Entity<ThreadView>, cx: &mut VisualTestContext) -> bool {
    view.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    cx.debug_bounds("handoff-cta").is_some()
}

fn ends_the_running_agent(unbind: bool, cx: &mut TestAppContext) {
    let scratch = tempfile::tempdir().expect("tempdir");
    let bin = slow_claude(scratch.path());
    let (_home, dir, mut config, view, cx) = view_with(cx, Some(bin));
    send(&view, "one", cx);
    settle(cx, |cx| running(&view, cx) && sent(&dir).len() == 1);
    if unbind {
        config.bindings.clear();
    } else {
        rebind(&mut config, &dir, "beta");
    }
    view.update(cx, |t, cx| t.set_config(config, cx));
    send(&view, "two", cx);
    let why = refused(&view, cx);
    let want = if unbind {
        "isn't bound to acme"
    } else {
        "now bound to beta"
    };
    assert!(why.contains(want), "{why}");
    assert!(!sent(&dir).iter().any(|s| s == "two"), "{:?}", sent(&dir));
    assert!(!cta_shown(&view, cx), "no other provider to continue in");
}

#[gpui::test]
fn a_rebound_folder_ends_the_running_agent(cx: &mut TestAppContext) {
    ends_the_running_agent(false, cx);
}

#[gpui::test]
fn an_unbound_folder_ends_the_running_agent(cx: &mut TestAppContext) {
    ends_the_running_agent(true, cx);
}

#[gpui::test]
fn a_rebind_only_on_disk_is_caught_before_the_send(cx: &mut TestAppContext) {
    let scratch = tempfile::tempdir().expect("tempdir");
    let bin = slow_claude(scratch.path());
    let (home, dir, config, view, cx) = view_with(cx, Some(bin));
    let file = home.path().join("accounts.toml");
    config.save(&file).expect("save");
    cx.update(|_, cx| cx.set_global(AccountsFile(file.clone())));
    send(&view, "one", cx);
    settle(cx, |cx| running(&view, cx) && sent(&dir).len() == 1);
    // The view's copy still says acme; accounts.toml says beta.
    let mut on_disk = config.clone();
    rebind(&mut on_disk, &dir, "beta");
    on_disk.save(&file).expect("save");
    send(&view, "two", cx);
    settle(cx, |cx| {
        view.read_with(cx, |t, _| t.session.is_none() && t.resume_refused.is_some())
    });
    let why = view.read_with(cx, |t, _| t.resume_refused.clone().map(|r| r.to_string()));
    assert!(
        why.as_deref()
            .is_some_and(|w| w.contains("now bound to beta")),
        "{why:?}"
    );
    assert!(!sent(&dir).iter().any(|s| s == "two"), "{:?}", sent(&dir));
    assert!(view.read_with(cx, |t, _| t.session.is_none()), "ended");
}

/// Adds the Codex account "oai" to `config`.
fn with_codex(config: &mut AccountsConfig, dir: &Path) {
    let root = dir.parent().expect("root");
    config
        .add_account(account(root, "oai", Provider::Codex))
        .expect("codex account");
}

#[gpui::test]
fn a_claude_thread_rebound_to_codex_offers_continue_in_codex(cx: &mut TestAppContext) {
    let scratch = tempfile::tempdir().expect("tempdir");
    let bin = slow_claude(scratch.path());
    let (_home, dir, mut config, view, cx) = view_with(cx, Some(bin));
    with_codex(&mut config, &dir);
    view.update(cx, |t, cx| t.set_config(config.clone(), cx));
    send(&view, "one", cx);
    settle(cx, |cx| running(&view, cx));
    rebind(&mut config, &dir, "oai");
    view.update(cx, |t, cx| t.set_config(config, cx));
    send(&view, "two", cx);
    let why = refused(&view, cx);
    assert!(why.contains("now bound to oai"), "{why}");
    assert!(cta_shown(&view, cx), "Continue in Codex shows");
}

#[gpui::test]
fn a_codex_thread_rebound_to_claude_offers_continue_in_claude(cx: &mut TestAppContext) {
    let codex =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../skyelab-agent/tests/fixtures/fake-codex.sh");
    let (_home, dir, mut config, view, cx) = view_with(cx, None);
    cx.update(|_, cx| cx.set_global(CodexBin(codex)));
    with_codex(&mut config, &dir);
    rebind(&mut config, &dir, "oai");
    view.update(cx, |t, cx| t.set_config(config.clone(), cx));
    send(&view, "one", cx);
    settle(cx, |cx| running(&view, cx));
    assert!(running(&view, cx), "the Codex agent runs");
    rebind(&mut config, &dir, "acme");
    view.update(cx, |t, cx| t.set_config(config, cx));
    send(&view, "two", cx);
    let why = refused(&view, cx);
    assert!(why.contains("now bound to acme"), "{why}");
    assert_eq!(
        view.read_with(cx, |t, _| t.provider()),
        Some(Provider::Codex)
    );
    assert!(cta_shown(&view, cx), "Continue in Claude shows");
}

#[gpui::test]
fn an_unreadable_accounts_file_does_not_kill_the_agent(cx: &mut TestAppContext) {
    let scratch = tempfile::tempdir().expect("tempdir");
    let bin = slow_claude(scratch.path());
    let (home, dir, config, view, cx) = view_with(cx, Some(bin));
    let file = home.path().join("accounts.toml");
    config.save(&file).expect("save");
    cx.update(|_, cx| cx.set_global(AccountsFile(file.clone())));
    send(&view, "one", cx);
    settle(cx, |cx| running(&view, cx) && sent(&dir).len() == 1);
    std::fs::write(&file, "this is = = not toml").expect("write");
    send(&view, "two", cx);
    settle(cx, |cx| view.read_with(cx, |t, _| t.error.is_some()));
    view.read_with(cx, |t, _| {
        assert!(t.session.is_some(), "the agent keeps running");
        assert!(t.resume_refused.is_none());
        let e = t.error.as_deref().unwrap_or_default().to_owned();
        assert!(e.contains("accounts.toml can't be read"), "{e}");
    });
    assert!(!sent(&dir).iter().any(|s| s == "two"), "{:?}", sent(&dir));
}

#[gpui::test]
fn a_refused_send_records_the_thread_stopped(cx: &mut TestAppContext) {
    let scratch = tempfile::tempdir().expect("tempdir");
    let bin = slow_claude(scratch.path());
    let (_home, dir, mut config, view, cx) = view_with(cx, Some(bin));
    send(&view, "one", cx);
    settle(cx, |cx| running(&view, cx) && sent(&dir).len() == 1);
    rebind(&mut config, &dir, "beta");
    view.update(cx, |t, cx| t.set_config(config, cx));
    send(&view, "two", cx);
    refused(&view, cx);
    let status = |cx: &mut VisualTestContext| {
        view.read_with(cx, |t, _| {
            let id = t.thread.as_ref().expect("row").id;
            crate::workspace::lock(&t.backend.store)
                .expect("store")
                .thread(id)
                .expect("read")
                .expect("row")
                .status
        })
    };
    // Written once the killed agent has ended, which can take seconds on a loaded machine.
    settle(cx, |cx| matches!(status(cx).as_str(), "stopped" | "exited"));
    assert_eq!(status(cx), "stopped", "ended on purpose, not exited");
}
