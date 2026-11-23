//! Acceptance: "Send to several models" starts one thread per model, each in the draft's folder under the folder's
//! account, linked as a comparison group: one sidebar row with a "×N" toggle, a Compare page, and "Keep this one"
//! archiving the others. A send whose account the folder no longer resolves to starts nothing, and "In worktrees"
//! gives each model its own worktree.

use super::changes_model_test::git;
use super::compare::{Compare, ToggleCompareGroup};
use super::thread::ThreadView;
use super::thread_model::Record;
use super::threads_test::{
    fixture, open_workspace, project_thread, setup, type_and_send, wait_until,
};
use super::*;
use gpui::{TestAppContext, VisualTestContext};

fn tick(view: &Entity<ThreadView>, models: &[&str], worktrees: bool, cx: &mut VisualTestContext) {
    let models = models.iter().map(|m| (*m).to_owned()).collect();
    view.update(cx, |v, _| {
        v.composer.compare = Some(Compare { models, worktrees })
    });
}

/// The comparison's stored rows, oldest first, once both have one.
fn group_rows(store: &SharedStore, n: usize, cx: &mut VisualTestContext) -> Vec<Thread> {
    let rows = || {
        let s = store.lock().expect("lock");
        let projects = s.list_projects().expect("projects");
        projects
            .iter()
            .flat_map(|p| s.list_threads(p.id).expect("threads"))
            .filter(|t| t.compare_group.is_some())
            .collect::<Vec<_>>()
    };
    wait_until(cx, "every model's row", |_| rows().len() == n);
    let mut rows = rows();
    rows.sort_by_key(|t| t.id);
    rows
}

/// The args the fake agent reported for `id`'s session.
fn agent_args(store: &SharedStore, id: i64) -> Option<String> {
    let s = store.lock().expect("lock");
    s.events_since(id, 0).expect("events").iter().find_map(|e| {
        match Record::from_row(&e.kind, &e.payload_json) {
            Ok(Record::Agent(skyelab_agent::Event::Unknown(v))) if v["type"] == "fake_env" => {
                v["args"].as_str().map(str::to_owned)
            }
            _ => None,
        }
    })
}

#[gpui::test]
fn siblings_share_folder_account_and_group_fold_in_the_sidebar_and_keep_archives(
    cx: &mut TestAppContext,
) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let draft = project_thread(&f, &ws, cx);
    tick(&draft, &["opus", "sonnet"], false, cx);
    type_and_send(&draft, cx);
    let rows = group_rows(&f.store, 2, cx);
    let group = rows[0].compare_group;
    for t in &rows {
        assert_eq!(t.cwd, f.project_dir, "the draft's folder");
        assert_eq!(t.account_id, "acme", "the folder's account");
        assert_eq!(t.compare_group, group, "one group");
        assert_eq!(t.worktree_path, None);
    }
    let group = group.expect("grouped");
    // Each thread ran its own model (the launches race, so either may have saved its row first).
    let mut models: Vec<String> = rows
        .iter()
        .map(|t| {
            wait_until(cx, "the agent's args", |_| {
                agent_args(&f.store, t.id).is_some()
            });
            let args = agent_args(&f.store, t.id).expect("args");
            args.split("--model ")
                .nth(1)
                .expect("--model")
                .split(' ')
                .next()
                .expect("id")
                .to_owned()
        })
        .collect();
    models.sort();
    assert_eq!(models, ["opus", "sonnet"]);
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.page, center::Page::Compare(group))
    });

    // The sidebar: one row for the group, "×2", its threads hidden until expanded.
    let ids = |ws: &Entity<Workspace>, cx: &mut VisualTestContext| {
        ws.read_with(cx, |ws, cx| {
            ws.sidebar
                .read(cx)
                .sections
                .iter()
                .flat_map(|s| s.items.iter().map(|i| i.id.to_string()))
                .collect::<Vec<_>>()
        })
    };
    let row = format!("compare-{group}");
    wait_until(cx, "the group row", |cx| ids(&ws, cx).contains(&row));
    let members: Vec<String> = rows.iter().map(|t| format!("thread-{}", t.id)).collect();
    assert!(!members.iter().any(|m| ids(&ws, cx).contains(m)), "folded");
    ws.read_with(cx, |ws, cx| {
        let marks = &ws.sidebar.read(cx).marks;
        assert_eq!(marks[row.as_str()].group, Some((2, false, group)));
    });
    cx.dispatch_action(ToggleCompareGroup { group });
    cx.run_until_parked();
    let shown = ids(&ws, cx);
    let at = shown.iter().position(|i| *i == row).expect("row");
    assert_eq!(shown[at + 1..at + 3], members[..], "members under the row");
    ws.read_with(cx, |ws, cx| {
        assert!(ws.sidebar.read(cx).marks[members[0].as_str()].nested);
    });

    // Keep the second: the first is archived.
    wait_until(cx, "both done", |cx| {
        ws.update(cx, |ws, cx| rows.iter().all(|t| !ws.working(t.id, cx)))
    });
    let (kept, other) = (rows[1].id, rows[0].id);
    ws.update_in(cx, |ws, window, cx| ws.finish_keep(kept, true, window, cx));
    wait_until(cx, "the other archived", |_| {
        let s = f.store.lock().expect("lock");
        s.thread(other)
            .expect("read")
            .expect("row")
            .archived_at
            .is_some()
    });
    let s = f.store.lock().expect("lock");
    assert!(
        s.thread(kept)
            .expect("read")
            .expect("row")
            .archived_at
            .is_none()
    );
}

#[gpui::test]
fn a_send_for_another_account_starts_nothing(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let draft = project_thread(&f, &ws, cx);
    tick(&draft, &["opus", "sonnet"], false, cx);
    draft.update_in(cx, |t, window, cx| {
        t.prompt
            .update(cx, |p, cx| p.set_value("hello", window, cx))
    });
    let other = skyelab_core::Account {
        id: "other".into(),
        label: "Other".into(),
        config_dir: f.account_dir.clone(),
        provider: Default::default(),
        gh_config_dir: None,
        ssh_agent: false,
    };
    ws.update_in(cx, |ws, window, cx| {
        ws.compare_send(&draft, &other, window, cx)
    });
    cx.run_until_parked();
    std::thread::sleep(std::time::Duration::from_millis(200));
    cx.run_until_parked();
    let threads = {
        let s = f.store.lock().expect("lock");
        let p = s.list_projects().expect("projects")[0].id;
        s.list_threads(p).expect("threads")
    };
    assert!(threads.is_empty(), "nothing started: {threads:?}");
    draft.read_with(cx, |t, cx| {
        let notice = t.composer.notice.clone().expect("refused with a reason");
        assert!(notice.contains("isn't bound to Other"), "{notice}");
        assert_eq!(t.prompt.read(cx).value(), "hello", "the prompt stays");
    });
}

#[gpui::test]
fn in_worktrees_gives_each_model_its_own(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    std::fs::write(f.project_dir.join("README"), "hi\n").expect("file");
    git(&f.project_dir, &["init", "-q", "-b", "main"]);
    git(&f.project_dir, &["add", "."]);
    git(&f.project_dir, &["commit", "-q", "-m", "init"]);
    let (ws, cx) = open_workspace(&f, cx);
    let draft = project_thread(&f, &ws, cx);
    // Three at once: their worktrees are made in the same moment, from the same title.
    tick(&draft, &["opus", "sonnet", "haiku"], true, cx);
    type_and_send(&draft, cx);
    let rows = group_rows(&f.store, 3, cx);
    let paths: Vec<_> = rows
        .iter()
        .map(|t| t.worktree_path.clone().expect("a worktree each"))
        .collect();
    let unique: std::collections::HashSet<_> = paths.iter().collect();
    assert_eq!(unique.len(), 3, "separate worktrees: {paths:?}");
    for (t, p) in rows.iter().zip(&paths) {
        assert_eq!(&t.cwd, p, "runs in its worktree");
        assert!(p.starts_with(&f.project_dir), "{}", p.display());
        assert_eq!(t.account_id, "acme");
    }
}
