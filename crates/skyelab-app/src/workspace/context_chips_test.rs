//! Acceptance: `#` lists this project's threads and linked pull requests of the composer's own account; a picked
//! thread becomes a chip of its title and last reply, sent as a fenced block; a thread that ran on another account,
//! or sits in a folder bound to another account, is never offered and can't be added.

use super::composer_context::ChipKind;
use super::composer_context_pick::{context_items, may_reference};
use super::composer_menu::{ContextPick, MenuKind};
use crate::workspace::thread_model::Item;
use crate::workspace::worktree_thread_test::{fixture, open_thread, send, wait_until};
use skyelab_core::Binding;
use skyelab_store::{NewThread, Store, Thread};
use std::path::Path;

/// Seeds a thread with a reply; returns it.
fn seed(s: &mut Store, project: i64, title: &str, cwd: &Path, account: &str) -> Thread {
    let t = s
        .create_thread(&NewThread {
            project_id: project,
            title,
            cwd,
            account_id: account,
            status: "idle",
        })
        .expect("thread");
    let reply =
        format!(r#"{{"Assistant":{{"content":[{{"type":"text","text":"reply of {title}"}}]}}}}"#);
    s.append_event(t.id, "agent", &reply).expect("reply");
    t
}

#[gpui::test]
fn picking_a_thread_sends_its_gist_and_other_accounts_stay_out(cx: &mut gpui::TestAppContext) {
    let mut f = fixture();
    // A subfolder bound to the other account.
    let sub = f.project.join("sub");
    std::fs::create_dir_all(&sub).expect("sub");
    f.config
        .add_binding(Binding::new(sub.clone(), "other".into()))
        .expect("bind sub");
    let (ws, thread, cx) = open_thread(&f, cx);
    let project = ws
        .read_with(cx, |ws, _| ws.project.as_ref().map(|p| p.id))
        .expect("project");

    let (sibling, foreign, stale, rebound) = {
        let mut s = f.store.lock().expect("store");
        let sibling = seed(&mut s, project, "sibling", &f.project, "acme");
        s.set_thread_pr_url(sibling.id, Some("https://github.com/o/r/pull/7"))
            .expect("pr");
        let foreign = seed(&mut s, project, "foreign", &sub, "other");
        s.set_thread_pr_url(foreign.id, Some("https://github.com/o/r/pull/8"))
            .expect("pr");
        // Stored under another account in acme's folder, and acme's thread in a folder now bound elsewhere.
        let stale = seed(&mut s, project, "stale", &f.project, "other");
        let rebound = seed(&mut s, project, "rebound", &sub, "acme");
        (sibling.id, foreign.id, stale.id, rebound.id)
    };

    // The pure filter, on every candidate.
    let threads = f
        .store
        .lock()
        .expect("store")
        .list_threads(project)
        .expect("list");
    for t in &threads {
        assert_eq!(
            may_reference(t, "acme", &f.config, &f.project),
            t.id == sibling,
            "{}",
            t.title
        );
    }
    let none = Default::default();
    let items = context_items(&threads, None, "acme", &f.config, &f.project, &none, 0);
    let picks: Vec<_> = items.iter().map(|i| i.context).collect();
    assert_eq!(
        picks,
        [
            Some(ContextPick::Thread(sibling)),
            Some(ContextPick::Pr(sibling))
        ]
    );
    assert_eq!(items[1].label, "PR #7 · sibling");
    // The other account sees only its own thread there.
    let theirs = context_items(&threads, None, "other", &f.config, &f.project, &none, 0);
    assert!(
        theirs
            .iter()
            .all(|i| i.context == Some(ContextPick::Thread(foreign))
                || i.context == Some(ContextPick::Pr(foreign)))
    );

    // `#` in the composer opens the menu with the same rows.
    cx.simulate_keystrokes("#");
    wait_until(cx, "the # menu", |cx| {
        thread.read_with(cx, |t, _| {
            t.composer.menu.as_ref().is_some_and(|m| {
                m.kind == MenuKind::Context && m.ranked.is_some() && m.rows.len() == 2
            })
        })
    });

    // A pick of a thread that isn't this account's (as a stale menu might hold) adds nothing.
    for id in [foreign, stale, rebound] {
        thread.update(cx, |t, cx| t.add_context_pick(ContextPick::Thread(id), cx));
        thread.update(cx, |t, cx| t.add_context_pick(ContextPick::Pr(id), cx));
    }
    cx.run_until_parked();
    assert!(thread.read_with(cx, |t, _| t.composer.contexts.is_empty()));

    thread.update_in(cx, |t, window, cx| t.accept_row(0, window, cx));
    wait_until(cx, "the thread chip", |cx| {
        thread.read_with(cx, |t, _| t.composer.contexts.len() == 1)
    });
    let chip = thread.read_with(cx, |t, _| t.composer.contexts[0].clone());
    assert_eq!(
        (chip.kind, chip.label.as_ref()),
        (ChipKind::Thread, "sibling")
    );
    assert_eq!(chip.text, "Title: sibling\nLast reply:\nreply of sibling");
    // The chip keeps its thread, whose sidebar card its hover shows.
    assert_eq!(chip.thread, Some(sibling));
    ws.update(cx, |ws, cx| ws.load_threads(cx));
    wait_until(cx, "the sidebar's card for the thread", |cx| {
        cx.update(|_, cx| {
            cx.try_global::<super::panels::ThreadCards>()
                .and_then(|c| c.0.get(&sibling).map(|card| card.title.to_string()))
                == Some("sibling".into())
        })
    });
    assert_eq!(
        thread.read_with(cx, |t, cx| t.prompt.read(cx).value()),
        "",
        "the # token goes"
    );

    // The PR chip too; then the project folder is rebound to the other account: both chips go, and none is sent.
    cx.simulate_keystrokes("#");
    wait_until(cx, "the # menu again", |cx| {
        thread.read_with(cx, |t, _| {
            t.composer.menu.as_ref().is_some_and(|m| m.ranked.is_some())
        })
    });
    thread.update_in(cx, |t, window, cx| t.accept_row(1, window, cx));
    wait_until(cx, "the PR chip", |cx| {
        thread.read_with(cx, |t, _| t.composer.contexts.len() == 2)
    });
    let pr = thread.read_with(cx, |t, _| t.composer.contexts[1].clone());
    assert_eq!(
        (pr.kind, pr.label.as_ref()),
        (ChipKind::PullRequest, "PR #7")
    );
    assert_eq!(
        pr.text,
        "URL: https://github.com/o/r/pull/7\nThread: sibling"
    );
    let mut rebound = f.config.clone();
    rebound.bindings.retain(|b| b.path != f.project);
    rebound
        .add_binding(Binding::new(f.project.clone(), "other".into()))
        .expect("rebind");
    thread.update(cx, |t, cx| t.set_config(rebound, cx));
    assert!(
        thread.read_with(cx, |t, _| t.composer.contexts.is_empty()),
        "a rebind drops them"
    );
    // Back to acme: a fresh pick works again.
    thread.update(cx, |t, cx| t.set_config(f.config.clone(), cx));
    thread.update(cx, |t, cx| {
        t.add_context_pick(ContextPick::Thread(sibling), cx)
    });
    wait_until(cx, "the chip again", |cx| {
        thread.read_with(cx, |t, _| t.composer.contexts.len() == 1)
    });

    send(cx);
    wait_until(cx, "the prompt", |cx| {
        thread.read_with(cx, |t, _| {
            t.items.iter().any(|i| matches!(i, Item::Prompt(_)))
        })
    });
    let sent = thread.read_with(cx, |t, _| {
        t.items.iter().find_map(|i| match i {
            Item::Prompt(p) => Some(p.clone()),
            _ => None,
        })
    });
    assert_eq!(
        sent.as_deref(),
        Some(
            "fix it\n\nContext from the thread \"sibling\":\n```text\nTitle: sibling\nLast reply:\nreply of sibling\n```"
        )
    );
}

/// Two threads with the same title read differently in `#`: each row shows its age, worktree branch and last reply.
#[test]
fn same_titled_threads_differ_in_the_menu() {
    let f = fixture();
    let mut s = f.store.lock().expect("store");
    let project = s.create_project("demo", &f.project).expect("project").id;
    let a = seed(&mut s, project, "Fix login", &f.project, "acme");
    let b = seed(&mut s, project, "Fix login", &f.project, "acme");
    s.append_event(
        b.id,
        "agent",
        r###"{"Assistant":{"content":[{"type":"text","text":"## Done\n**Tests** pass now"}]}}"###,
    )
    .expect("reply");
    let mut threads = s.list_threads(project).expect("list");
    let hour = 3_600_000;
    for t in &mut threads {
        if t.id == a.id {
            t.worktree_branch = Some("fix-login-2".into());
            t.updated_at = 0;
        } else {
            t.updated_at = 2 * hour;
        }
    }
    let replies = s.last_replies(Some(project)).expect("replies");
    let items = context_items(
        &threads,
        None,
        "acme",
        &f.config,
        &f.project,
        &replies,
        3 * hour,
    );
    let detail = |id| {
        items
            .iter()
            .find(|i| i.context == Some(ContextPick::Thread(id)))
            .map(|i| i.detail.clone())
            .expect("row")
    };
    assert_eq!(detail(a.id), "3h · fix-login-2 · reply of Fix login");
    assert_eq!(
        detail(b.id),
        "1h · Done Tests pass now",
        "plain text, latest reply"
    );
}
