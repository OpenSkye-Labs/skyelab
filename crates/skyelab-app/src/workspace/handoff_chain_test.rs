//! A handoff of a handoff (Claude → Codex → Claude) still carries the first thread's history: a handoff copies no
//! events, so its own events alone would lose it.

use crate::workspace::thread_extras::handoff::HandOffThread;
use crate::workspace::threads_test::{
    fixture, open_workspace, project_thread, setup, type_and_send, wait_done, wait_until,
};
use crate::workspace::{AccountsFile, CodexBin};
use gpui::TestAppContext;
use skyelab_core::{Account, AccountsConfig, Binding, Provider};
use std::path::Path;

#[gpui::test]
fn a_handoff_of_a_handoff_keeps_the_first_threads_history(cx: &mut TestAppContext) {
    setup(cx);
    let codex =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../skyelab-agent/tests/fixtures/fake-codex.sh");
    cx.update(|cx| cx.set_global(CodexBin(codex)));
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let bind = |account: &str, cx: &mut gpui::VisualTestContext| {
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
    };
    let a = project_thread(&f, &ws, cx);
    type_and_send(&a, cx); // "hi\nthere"
    wait_done(&a, cx);
    let a_id = a.read_with(cx, |t, _| t.thread.as_ref().expect("row").id);
    let child = |of: i64, cx: &mut gpui::VisualTestContext| {
        wait_until(cx, "the handoff row", |cx| {
            ws.read_with(cx, |ws, _| {
                ws.threads.iter().any(|t| t.forked_from == Some(of))
            })
        });
        ws.read_with(cx, |ws, _| {
            ws.threads
                .iter()
                .find(|t| t.forked_from == Some(of))
                .expect("row")
                .id
        })
    };
    bind("oai", cx);
    cx.dispatch_action(HandOffThread {
        id: a_id,
        to: Provider::Codex,
    });
    let b_id = child(a_id, cx);
    bind("acme", cx);
    cx.dispatch_action(HandOffThread {
        id: b_id,
        to: Provider::Claude,
    });
    let c_id = child(b_id, cx);
    let ctx = f
        .store
        .lock()
        .expect("lock")
        .fork_context(c_id)
        .expect("read")
        .expect("ctx");
    assert!(
        ctx.contains(r#"hi\nthere"#),
        "A's history lost in A→B→C handoff:\n{ctx}"
    );
}
