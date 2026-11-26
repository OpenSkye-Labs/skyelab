//! A GitHub-started run in Ask mode: Claude loads none of the account's settings (so no allow rule runs a tool
//! unasked), it runs in a new worktree from the default branch even though the bot says Local checkout, nothing
//! allows beyond one call, and the bot's card says it waits for approval.

use super::super::center::Page;
use super::super::lock;
use super::super::threads_test::wait_until;
use super::bots_test::{bot, github, prs, tick_at};
use super::test_util::{NOW, open, rig, runs};
use gpui::TestAppContext;
use serde_json::Value;
use skyelab_agent::Decision;
use skyelab_core::bots::POLL_EVERY_MS;

fn find_env(v: &Value) -> Option<&Value> {
    match v {
        Value::Object(m) if m.get("type").is_some_and(|t| t == "fake_env") => Some(v),
        Value::Object(m) => m.values().find_map(find_env),
        Value::Array(a) => a.iter().find_map(find_env),
        _ => None,
    }
}

#[gpui::test]
fn a_github_run_asks_every_call_in_a_fresh_worktree(cx: &mut TestAppContext) {
    let rig = rig(true);
    let (ws, cx) = open(&rig, cx);
    let gh = github(&rig, cx);
    let project = rig.f.project_dir.clone();
    // The folder has a PR's code checked out: the run must never start from it.
    for args in [
        &["checkout", "-q", "-b", "pr-code"][..],
        &["add", "evil.txt"],
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "commit",
            "-q",
            "-m",
            "pr",
        ],
    ] {
        if args[0] == "add" {
            std::fs::write(project.join("evil.txt"), "pr code").expect("write");
        }
        let ok = std::process::Command::new("git")
            .args(args)
            .current_dir(&project)
            .status();
        assert!(ok.expect("git").success(), "git {args:?}");
    }
    let config_dir = skyelab_core::AccountsConfig::load(&rig.f.accounts_path)
        .expect("load")
        .accounts[0]
        .config_dir
        .clone();
    std::fs::create_dir_all(&config_dir).expect("mkdir");
    std::fs::write(
        config_dir.join("settings.json"),
        r#"{"permissions":{"allow":["Bash(make deploy)"]}}"#,
    )
    .expect("settings");
    prs(&gh, &[12]);
    let mut t = bot(&rig, "default", 6);
    t.prompt = "permit".into();
    assert!(!t.worktree, "saved as Local checkout");
    lock(&rig.f.store)
        .expect("store")
        .save_scheduled_task(&t)
        .expect("save");
    ws.update_in(cx, |ws, window, cx| ws.show(Page::Bots, window, cx));
    cx.run_until_parked();
    let page = ws.read_with(cx, |ws, cx| {
        ws.bots.as_ref().map(|b| b.read(cx).list.clone())
    });
    let page = page.expect("the page");

    tick_at(NOW, cx);
    prs(&gh, &[12, 13]);
    tick_at(NOW + POLL_EVERY_MS, cx);
    tick_at(NOW + POLL_EVERY_MS + 15_000, cx);
    wait_until(cx, "the bot's thread", |_| runs(&rig, t.id).len() == 1);
    let thread = runs(&rig, t.id)[0].id;
    let env = || {
        let events = lock(&rig.f.store)
            .expect("store")
            .events_since(thread, 0)
            .expect("events");
        events.iter().find_map(|e| {
            let v: Value = serde_json::from_str(&e.payload_json).ok()?;
            find_env(&v).cloned()
        })
    };
    wait_until(cx, "the agent's args", |_| env().is_some());
    let env = env().expect("fake_env");
    let argv: Vec<&str> = env["argv"]
        .as_array()
        .expect("argv")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let i = argv
        .iter()
        .position(|a| *a == "--setting-sources")
        .expect("flag");
    assert_eq!(
        argv[i + 1],
        "",
        "no setting source, so no allow rule: {argv:?}"
    );
    let cwd = std::path::PathBuf::from(env["cwd"].as_str().expect("cwd"));
    assert!(
        cwd.to_string_lossy().contains("/.skyelab/worktrees/"),
        "{cwd:?}"
    );
    assert!(
        cwd.join(".git").exists() && !cwd.join("evil.txt").exists(),
        "from main, not the checkout"
    );

    // `make deploy` asks, though the account allows it; the card says so, and only Allow once is offered.
    let view = ws.read_with(cx, |ws, cx| {
        ws.open_threads
            .iter()
            .find(|v| v.read(cx).scheduled_task.is_some())
            .cloned()
    });
    let view = view.expect("the run's view");
    wait_until(cx, "the ask", |cx| {
        view.read_with(cx, |v, _| {
            crate::workspace::thread_rows::waiting_ask(v).is_some()
        })
    });
    cx.run_until_parked();
    assert!(
        page.read_with(cx, |p, _| p.waiting.contains(&t.id)),
        "the card waits"
    );
    let id = view.read_with(cx, |v, _| {
        v.items.iter().find_map(|i| match i {
            crate::workspace::thread_model::Item::Approval(a) => Some(a.id.clone()),
            _ => None,
        })
    });
    let id = id.expect("ask id");
    view.update(cx, |v, cx| {
        use crate::workspace::thread_items::ItemsView as _;
        assert!(!v.offers_thread_allow() && !v.takes_project_rules());
        assert!(
            v.answer_then(&id, Decision::AllowThread, |_| {}, cx)
                .is_err()
        );
        v.answer(&id, Decision::AllowOnce, cx);
    });
    wait_until(cx, "the card stops waiting", |cx| {
        page.read_with(cx, |p, _| !p.waiting.contains(&t.id))
    });
}
