//! Acceptance: Import projects reads only the added accounts' own history. A folder is ticked only when it is a git
//! repo with at least 3 sessions, the newest within 30 days; one in two histories defaults to the account with more
//! sessions; a folder already bound keeps its account; Import binds each pick to its shown account and adds it.

use super::onboarding::Step;
use super::onboarding_test::{Rig, rig};
use super::threads_test::wait_until;
use gpui::{TestAppContext, VisualTestContext};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// `n` Claude sessions of the account in `dir` that ran in `cwd`, last changed `days` ago.
fn sessions(dir: &Path, cwd: &Path, n: usize, days: u64) {
    let d = dir.join("projects").join(skyelab_agent::import::claude_dir_name(cwd));
    std::fs::create_dir_all(&d).expect("mkdir");
    for i in 0..n {
        let f = d.join(format!("s{i}.jsonl"));
        let line = format!("{{\"type\":\"user\",\"cwd\":{:?}}}\n", cwd.to_str().expect("utf8"));
        std::fs::write(&f, line).expect("write");
        let at = SystemTime::now() - Duration::from_secs(days * 86_400);
        std::fs::File::options().write(true).open(&f).expect("open").set_modified(at).expect("mtime");
    }
}

fn repo(path: &Path) {
    std::fs::create_dir_all(path.join(".git")).expect("mkdir");
    std::fs::write(path.join(".git/HEAD"), "ref: refs/heads/main\n").expect("head");
}

fn row(rig: &Rig, path: &Path, cx: &mut VisualTestContext) -> (bool, String, bool) {
    rig.setup.read_with(cx, |o, _| {
        let r = o.import.rows.iter().find(|r| r.cand.path == path).expect("listed");
        (r.checked, r.account.clone(), r.locked)
    })
}

#[gpui::test]
fn import_ticks_recent_repos_and_binds_each_to_its_shown_account(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx);
    let h = rig.home.clone();
    let p = |name: &str| -> PathBuf { h.join("work").join(name) };
    for name in ["recent", "few", "old", "shared", "bound/inner"] {
        repo(&p(name));
    }
    std::fs::create_dir_all(p("plain")).expect("mkdir");
    std::fs::create_dir_all(p("theirs")).expect("mkdir");
    let (mine, work, other) = (h.join(".claude"), h.join(".claude-work"), h.join(".codex"));
    sessions(&mine, &p("recent"), 3, 1);
    sessions(&mine, &p("plain"), 3, 1);
    sessions(&mine, &p("few"), 2, 1);
    sessions(&mine, &p("old"), 3, 31);
    sessions(&mine, &p("shared"), 1, 1);
    sessions(&work, &p("shared"), 4, 1);
    sessions(&mine, &p("bound/inner"), 3, 1);
    // A found config dir that isn't added: never read.
    sessions(&other, &p("theirs"), 5, 1);
    rig.add("claude", cx);
    rig.add("claude-work", cx);
    let bound = p("bound");
    rig.ws.update(cx, |ws, cx| {
        ws.edit_accounts(
            |c| c.add_binding(skyelab_core::Binding::new(bound.clone(), "claude-work".into())),
            cx,
        )
        .expect("bind")
    });
    cx.run_until_parked();
    rig.setup.update(cx, |o, cx| o.go(Step::Import, cx));
    let setup = rig.setup.clone();
    wait_until(cx, "the scan", |cx| setup.read_with(cx, |o, _| !o.import.rows.is_empty() && !o.import.scanning));

    assert_eq!(row(&rig, &p("recent"), cx), (true, "claude".into(), false));
    for name in ["plain", "few", "old"] {
        assert!(!row(&rig, &p(name), cx).0, "{name} misses one rule: not ticked");
    }
    // 1 + 4 sessions: ticked, on the account with 4.
    assert_eq!(row(&rig, &p("shared"), cx), (true, "claude-work".into(), false));
    assert_eq!(row(&rig, &p("bound/inner"), cx), (true, "claude-work".into(), true));
    let listed = rig.setup.read_with(cx, |o, _| o.import.rows.iter().any(|r| r.cand.path == p("theirs")));
    assert!(!listed, "a found-but-not-added dir's history is never read");

    rig.setup.update(cx, |o, cx| {
        o.set_row_account(&p("bound/inner"), "claude", cx);
        o.toggle_row(&p("plain"), cx);
        o.toggle_row(&p("plain"), cx);
    });
    assert_eq!(row(&rig, &p("bound/inner"), cx).1, "claude-work", "a bound folder keeps its account");
    let at = cx.debug_bounds("onboarding-next").expect("Import button").center();
    cx.simulate_click(at, gpui::Modifiers::default());
    let ws = rig.ws.clone();
    wait_until(cx, "three projects", |cx| ws.read_with(cx, |ws, _| ws.projects.len() == 3));
    let saved = rig.saved();
    let on = |name: &str| saved.resolve(&p(name)).map(|a| a.id.clone());
    assert_eq!(on("recent").as_deref(), Some("claude"));
    assert_eq!(on("shared").as_deref(), Some("claude-work"));
    assert_eq!(on("bound/inner").as_deref(), Some("claude-work"));
    assert_eq!(saved.bindings.len(), 3, "bound, recent, shared: the inner folder rides on its parent");
    assert_eq!(rig.setup.read_with(cx, |o, _| o.step), Step::Done);
}
