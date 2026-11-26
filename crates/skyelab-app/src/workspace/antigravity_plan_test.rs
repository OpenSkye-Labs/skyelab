//! Acceptance: Antigravity's Plan mode writes its plan to a file asking for feedback (outside the folder, in its
//! brain dir) and ends its turn. SkyeLab shows it as a plan card from the tool call (never reading that file), the
//! reply's `file:///…/plan.md` link scrolls to the card, and Approve leaves Plan and sends the go-ahead.
#![cfg(unix)]

use super::thread::ThreadView;
use super::thread::plan_review::APPROVED;
use super::thread_items::ItemsView;
use super::thread_model::Item;
use super::*;
use crate::test_wait::wait_until;
use gpui::TestAppContext;
use skyelab_agent::{Decision, Mode};
use skyelab_core::{Account, Binding, Provider};
use skyelab_store::Store;
use std::sync::{Arc, Mutex};

fn fake_antigravity() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../skyelab-agent/tests/fixtures/fake-antigravity.sh")
}

fn plan_card(t: &ThreadView) -> Option<skyelab_agent::timeline::ApprovalItem> {
    t.items.iter().find_map(|i| match i {
        Item::Approval(a) if a.tool == skyelab_agent::PLAN_TOOL => Some(a.clone()),
        _ => None,
    })
}

#[gpui::test]
fn a_plan_written_for_review_shows_as_a_plan_card_and_approve_goes_on(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::settings::init(cx);
        skyelab_terminal::init(cx);
        init(cx);
        cx.set_global(AntigravityBin(fake_antigravity()));
    });
    let home = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(home.path()).expect("canonicalize");
    let project = root.join("demo");
    std::fs::create_dir_all(&project).expect("project dir");
    let gemini_home = root.join(".antigravity-work");
    std::fs::create_dir_all(&gemini_home).expect("gemini_home");
    std::fs::write(gemini_home.join("fake-login"), "me@example.com\n").expect("login");

    let accounts_path = root.join("accounts.toml");
    let mut config = AccountsConfig::default();
    config
        .add_account(Account {
            id: "agy".into(),
            label: "Antigravity".into(),
            provider: Provider::Antigravity,
            config_dir: gemini_home.clone(),
            gh_config_dir: None,
            ssh_agent: false,
        })
        .expect("add agy");
    config
        .add_account(Account {
            id: "acme".into(),
            label: "Acme".into(),
            provider: Provider::Claude,
            config_dir: root.join(".claude-acme"),
            gh_config_dir: None,
            ssh_agent: false,
        })
        .expect("add claude");
    config
        .add_binding(Binding::new(project.clone(), "agy".into()))
        .expect("bind");
    config.save(&accounts_path).expect("save");

    let store: SharedStore = Arc::new(Mutex::new(Store::open_in_memory().expect("store")));
    let backend = Backend {
        store: store.clone(),
        claude_bin: root.join("no-such-claude"),
    };
    let (ws, cx) = cx.add_window_view(|window, cx| {
        let ws = Workspace::new(
            root.clone(),
            Some(accounts_path.clone()),
            config.clone(),
            None,
            backend,
            window,
            cx,
        );
        window.focus(&ws.focus, cx);
        ws
    });
    ws.update(cx, |ws, cx| ws.add_project(project.clone(), cx));
    wait_until(cx, "project stored", |cx| {
        ws.read_with(cx, |ws, _| !ws.projects.is_empty())
    });
    let id = ws.read_with(cx, |ws, _| ws.projects[0].id);
    cx.dispatch_action(SelectProject { id });
    cx.run_until_parked();
    cx.dispatch_action(NewThread);
    cx.run_until_parked();

    let thread: Entity<ThreadView> = ws
        .read_with(cx, |ws, _| ws.open_threads.last().cloned())
        .expect("thread tab");

    thread.update(cx, |t, cx| t.set_mode(Mode::Plan, cx));
    thread.update_in(cx, |t, window, cx| t.focus_prompt(window, cx));
    cx.simulate_input("review_plan please");
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-enter"
    } else {
        "ctrl-enter"
    });
    wait_until(cx, "the plan card", |cx| {
        thread.read_with(cx, |t, _| {
            assert_eq!(t.error, None, "thread errored");
            plan_card(t).is_some() && t.items.iter().any(|i| matches!(i, Item::Done { .. }))
        })
    });
    let card = thread.read_with(cx, |t, _| plan_card(t)).expect("card");
    assert_eq!(card.raw["plan"], "# Plan\n\n1. Add subtract(a, b) to calc.py\n2. Test it");
    let artifact = card.raw["artifact"].as_str().expect("its file").to_owned();
    assert!(artifact.starts_with(gemini_home.to_str().expect("utf-8")) && artifact.ends_with("/plan.md"));
    assert!(!std::path::Path::new(&artifact).exists(), "the fake never wrote it: the card comes from the call");
    assert_eq!(card.decision, None, "waits for review after the turn ended");

    // The reply's file:/// link to it scrolls to the card; another outside path opens nothing.
    assert_eq!(
        crate::workspace::timeline_text::file_url(&format!("file://{artifact}")).as_deref(),
        Some(artifact.as_str())
    );
    assert!(thread.update(cx, |t, cx| t.reveal_plan(&artifact, cx)));
    assert!(!thread.update(cx, |t, cx| t.reveal_plan("/etc/passwd", cx)));

    // Approve: out of Plan, the card settles, and the go-ahead is sent.
    thread.update_in(cx, |t, window, cx| ItemsView::answer_plan(t, &card.id, true, window, cx));
    wait_until(cx, "the go-ahead answered", |cx| {
        thread.read_with(cx, |t, _| {
            assert_eq!(t.error, None, "thread errored");
            t.items.iter().any(|i| matches!(i, Item::Text(x) if x == "Implementing the plan."))
        })
    });
    thread.read_with(cx, |t, _| {
        assert_eq!(t.mode, Mode::Default, "left Plan");
        assert_eq!(plan_card(t).and_then(|c| c.decision), Some(Decision::AllowOnce));
        assert!(t.items.iter().any(|i| matches!(i, Item::Prompt(p) if p == APPROVED)));
    });
}
