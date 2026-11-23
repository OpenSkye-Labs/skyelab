//! Acceptance: "Regenerate title" names the thread again, replacing even the user's title, under the thread's own
//! account; with the folder no longer bound to it, nothing runs and the user is told why.

use crate::workspace::manage::{ManageThread, ThreadOp};
use crate::workspace::manage_harness::rig;
use crate::workspace::thread_model::Record;
use crate::workspace::threads_test::wait_until;
use gpui::TestAppContext;
use skyelab_agent::{Block, Event};

#[gpui::test]
fn regenerate_replaces_any_title_and_refuses_an_unbound_folder(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    let id = r.thread("my own name", cx);
    {
        let mut s = r.f.store.lock().expect("lock");
        s.set_thread_title(id, "my own name").expect("rename");
        for rec in [
            Record::Prompt("fix the login bug".into()),
            Record::Agent(Event::Assistant {
                content: vec![Block::Text {
                    text: "Fixed the session check.".into(),
                }],
            }),
        ] {
            let (kind, payload) = rec.to_row();
            s.append_event(id, kind, &payload).expect("event");
        }
    }
    cx.dispatch_action(ManageThread {
        id,
        op: ThreadOp::RegenerateTitle,
    });
    let want = "Fake title from .claude-acme";
    wait_until(cx, "the new title", |cx| r.listed(cx) == [want]);
    assert_eq!(r.row(id).expect("row").title, want);
    let log = std::fs::read_to_string(r.f.project_dir.join(".fake-title-call")).expect("called");
    assert!(
        log.contains("fix the login bug") && log.contains("Fixed the session check."),
        "{log}"
    );

    // Unbound now: refused, no second call, the title stays.
    let mut config = skyelab_core::AccountsConfig::load(&r.f.accounts_path).expect("load");
    config.bindings.clear();
    config.save(&r.f.accounts_path).expect("save");
    let file = r.f.accounts_path.clone();
    cx.update(|_, cx| cx.set_global(crate::workspace::AccountsFile(file)));
    cx.dispatch_action(ManageThread {
        id,
        op: ThreadOp::RegenerateTitle,
    });
    wait_until(cx, "the refusal", |cx| !r.notices(cx).is_empty());
    assert!(
        r.notices(cx)
            .iter()
            .any(|n| n.contains("isn't bound to acme")),
        "{:?}",
        r.notices(cx)
    );
    let log = std::fs::read_to_string(r.f.project_dir.join(".fake-title-call")).expect("log");
    assert_eq!(
        log.matches("\n---\n").count(),
        1,
        "no call under another account"
    );
    assert_eq!(r.row(id).expect("row").title, want);
}
