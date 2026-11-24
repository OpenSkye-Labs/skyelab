//! Fork and Continue-in are single-flight per source thread: a double click makes one thread, not two.

use crate::workspace::thread_extras::fork::ForkThread;
use crate::workspace::thread_extras::handoff::HandOffThread;
use crate::workspace::threads_test::{
    Fixture, fixture, open_workspace, project_thread, setup, type_and_send, wait_done, wait_until,
};
use crate::workspace::{AccountsFile, CodexBin, Workspace};
use gpui::{Entity, TestAppContext, VisualTestContext};
use skyelab_core::{Account, AccountsConfig, Binding, Provider};
use std::path::Path;

/// Binds the fixture's folder to `account`, adding the Codex account "oai" first.
pub(super) fn bind(f: &Fixture, ws: &Entity<Workspace>, account: &str, cx: &mut VisualTestContext) {
    let mut c = AccountsConfig::load(&f.accounts_path).expect("load");
    if !c.accounts.iter().any(|a| a.id == "oai") {
        c.add_account(Account {
            id: "oai".into(),
            label: "OpenAI".into(),
            provider: Provider::Codex,
            config_dir: f.project_dir.parent().expect("root").join(".codex-oai"),
            gh_config_dir: None,
            ssh_agent: false,
        })
        .expect("add");
    }
    c.bindings.clear();
    c.add_binding(Binding::new(f.project_dir.clone(), account.into()))
        .expect("bind");
    c.save(&f.accounts_path).expect("save");
    let file = f.accounts_path.clone();
    cx.update(|_, cx| cx.set_global(AccountsFile(file)));
    ws.update(cx, |ws, cx| {
        ws.config = c;
        ws.push_config(cx);
    });
    cx.run_until_parked();
}

pub(super) fn use_fake_codex(cx: &mut TestAppContext) {
    let codex =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../skyelab-agent/tests/fixtures/fake-codex.sh");
    cx.update(|cx| cx.set_global(CodexBin(codex)));
}

fn children(f: &Fixture, of: i64) -> usize {
    f.store
        .lock()
        .expect("lock")
        .list_all_threads()
        .expect("threads")
        .iter()
        .filter(|t| t.forked_from == Some(of))
        .count()
}

#[gpui::test]
fn a_double_fork_or_handoff_makes_one_thread(cx: &mut TestAppContext) {
    setup(cx);
    use_fake_codex(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let a = project_thread(&f, &ws, cx);
    type_and_send(&a, cx);
    wait_done(&a, cx);
    let id = a.read_with(cx, |t, _| t.thread.as_ref().expect("row").id);

    let fork = ForkThread { id, turns: 1 };
    ws.update_in(cx, |ws, window, cx| {
        ws.fork_thread(&fork, window, cx);
        ws.fork_thread(&fork, window, cx);
    });
    wait_until(cx, "the fork row", |_| children(&f, id) >= 1);
    for _ in 0..20 {
        cx.run_until_parked();
    }
    assert_eq!(children(&f, id), 1, "one fork");
    // Done: a later fork is a new request, not a repeat.
    ws.update_in(cx, |ws, window, cx| ws.fork_thread(&fork, window, cx));
    wait_until(cx, "a second fork", |_| children(&f, id) == 2);

    bind(&f, &ws, "oai", cx);
    let hand = HandOffThread {
        id,
        to: Provider::Codex,
    };
    ws.update_in(cx, |ws, window, cx| {
        ws.hand_off_thread(&hand, window, cx);
        ws.hand_off_thread(&hand, window, cx);
    });
    wait_until(cx, "the handoff row", |_| children(&f, id) >= 3);
    for _ in 0..20 {
        cx.run_until_parked();
    }
    assert_eq!(children(&f, id), 3, "one handoff");
}
