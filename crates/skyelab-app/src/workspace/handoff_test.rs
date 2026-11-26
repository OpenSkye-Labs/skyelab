//! Acceptance: "Continue in Codex" / "Continue in Claude" starts a new thread in the same project under the folder's
//! bound account of the other provider, linked to its source, whose first session gets the source's transcript ahead
//! of the typed message. With no binding to that provider, nothing is created and a dialog asks to bind the folder.

use crate::workspace::thread::ThreadView;
use crate::workspace::thread_extras::handoff::{ContinueInClaude, ContinueInCodex, HandOffThread};
use crate::workspace::threads_test::{
    Fixture, fixture, open_workspace, project_thread, setup, type_and_send, wait_done, wait_until,
};
use crate::workspace::{AccountsFile, CodexBin, Workspace};
use gpui::{Entity, TestAppContext, VisualTestContext};
use gpui_component::WindowExt as _;
use skyelab_core::{Account, AccountsConfig, Binding, Provider};
use std::path::Path;

fn fake_codex() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../skyelab-agent/tests/fixtures/fake-codex.sh")
}

/// The fixture's config with a Codex account "oai" added and the project bound to `account` (`None`: unbound).
fn bind(f: &Fixture, account: Option<&str>) -> AccountsConfig {
    let mut config = AccountsConfig::load(&f.accounts_path).expect("load");
    if !config.accounts.iter().any(|a| a.id == "oai") {
        config
            .add_account(Account {
                id: "oai".into(),
                label: "OpenAI".into(),
                provider: Provider::Codex,
                config_dir: f.project_dir.parent().expect("root").join(".codex-oai"),
                gh_config_dir: None,
                ssh_agent: false,
            })
            .expect("add codex account");
    }
    config.bindings.clear();
    if let Some(account) = account {
        config
            .add_binding(Binding::new(f.project_dir.clone(), account.into()))
            .expect("bind");
    }
    config.save(&f.accounts_path).expect("save");
    config
}

/// Saves the binding and tells the workspace, as Settings does.
fn rebind(f: &Fixture, ws: &Entity<Workspace>, account: Option<&str>, cx: &mut VisualTestContext) {
    let config = bind(f, account);
    let file = f.accounts_path.clone();
    cx.update(|_, cx| cx.set_global(AccountsFile(file)));
    ws.update(cx, |ws, cx| {
        ws.config = config;
        ws.push_config(cx);
    });
    cx.run_until_parked();
}

fn thread_count(f: &Fixture, project_id: i64) -> usize {
    f.store
        .lock()
        .expect("lock")
        .list_threads(project_id)
        .expect("list")
        .len()
}

/// Hands thread `src` to `to`, waits for the new row, checks it, sends a message and waits until the agent's request
/// line shows the transcript.
fn hand_off_and_send(
    f: &Fixture,
    ws: &Entity<Workspace>,
    src: &Entity<ThreadView>,
    to: Provider,
    account: &str,
    cx: &mut VisualTestContext,
) {
    let src_id = src.read_with(cx, |t, _| t.thread.as_ref().expect("row").id);
    cx.dispatch_action(HandOffThread { id: src_id, to });
    wait_until(cx, "the handoff opened", |cx| {
        ws.read_with(cx, |ws, _| {
            ws.threads.iter().any(|t| t.forked_from == Some(src_id))
        })
    });
    let row = ws
        .read_with(cx, |ws, _| {
            ws.threads
                .iter()
                .find(|t| t.forked_from == Some(src_id))
                .cloned()
        })
        .expect("row");
    assert_eq!(row.account_id, account, "under the folder's bound account");
    assert_eq!(
        row.permission_mode, "bypass",
        "the new-thread default, not the store's Ask"
    );
    let context = f
        .store
        .lock()
        .expect("lock")
        .fork_context(row.id)
        .expect("read")
        .expect("a transcript for the first session");
    assert!(
        context.contains("<conversation>") && context.contains(r#"hi\nthere"#),
        "{context}"
    );
    let view = ws
        .read_with(cx, |ws, cx| ws.open_view(row.id, cx))
        .expect("the new thread's tab");
    wait_until(cx, "the source link", |cx| {
        view.read_with(cx, |t, _| {
            t.composer
                .extras
                .forked_from
                .as_ref()
                .is_some_and(|(id, label)| *id == src_id && label.starts_with("Continued from"))
        })
    });
    view.update_in(cx, |t, window, cx| t.focus_prompt(window, cx));
    cx.simulate_input("next step");
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-enter"
    } else {
        "ctrl-enter"
    });
    wait_until(cx, "the prompt shown as typed", |cx| {
        view.read_with(cx, |t, _| {
            t.items
                .iter()
                .any(|i| matches!(i, crate::workspace::thread_model::Item::Prompt(p) if p == "next step"))
        })
    });
    wait_until(cx, "the agent got the transcript", |_| {
        let s = f.store.lock().expect("lock");
        s.events_since(row.id, 0)
            .expect("events")
            .iter()
            .any(|e| e.kind == "agent" && e.payload_json.contains("conversation>"))
    });
}

#[gpui::test]
fn claude_thread_continues_in_codex(cx: &mut TestAppContext) {
    setup(cx);
    cx.update(|cx| cx.set_global(CodexBin(fake_codex())));
    let f = fixture(true);
    bind(&f, Some("acme"));
    let (ws, cx) = open_workspace(&f, cx);
    let src = project_thread(&f, &ws, cx);
    type_and_send(&src, cx);
    wait_done(&src, cx);
    // A Claude thread offers only the other provider.
    src.update_in(cx, |t, window, cx| t.focus_prompt(window, cx));
    let (codex, claude) = cx.update(|window, cx| {
        (
            window.is_action_available(&ContinueInCodex, cx),
            window.is_action_available(&ContinueInClaude, cx),
        )
    });
    assert!(
        codex && !claude,
        "codex offered: {codex}, claude offered: {claude}"
    );

    // Still bound to Claude: nothing is created, and a dialog asks to bind the folder.
    let project_id = src.read_with(cx, |t, _| t.project_id);
    let src_id = src.read_with(cx, |t, _| t.thread.as_ref().expect("row").id);
    let before = thread_count(&f, project_id);
    cx.dispatch_action(HandOffThread {
        id: src_id,
        to: Provider::Codex,
    });
    wait_until(cx, "the bind dialog", |cx| {
        cx.update(|window, cx| window.has_active_dialog(cx))
    });
    assert_eq!(
        thread_count(&f, project_id),
        before,
        "nothing under a Claude binding"
    );
    cx.update(|window, cx| window.close_dialog(cx));

    // Unbound: the same.
    rebind(&f, &ws, None, cx);
    cx.dispatch_action(HandOffThread {
        id: src_id,
        to: Provider::Codex,
    });
    wait_until(cx, "the bind dialog", |cx| {
        cx.update(|window, cx| window.has_active_dialog(cx))
    });
    assert_eq!(thread_count(&f, project_id), before, "nothing when unbound");
    cx.update(|window, cx| window.close_dialog(cx));

    // Bound to Codex: the source can't resume there, and its banner offers the handoff.
    rebind(&f, &ws, Some("oai"), cx);
    src.update_in(cx, |t, window, cx| t.focus_prompt(window, cx));
    // A live process keeps its session; the next one is what the binding is checked for.
    cx.dispatch_action(crate::workspace::thread::RestartAgentSession);
    wait_until(cx, "the agent ended", |cx| {
        src.read_with(cx, |t, _| t.session.is_none())
    });
    cx.simulate_input("more");
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-enter"
    } else {
        "ctrl-enter"
    });
    wait_until(cx, "the refused banner", |cx| {
        src.read_with(cx, |t, _| t.resume_refused.is_some())
    });
    src.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("handoff-cta").is_some(),
        "the banner offers Continue in Codex"
    );

    hand_off_and_send(&f, &ws, &src, Provider::Codex, "oai", cx);
}

#[gpui::test]
fn codex_thread_continues_in_claude(cx: &mut TestAppContext) {
    setup(cx);
    cx.update(|cx| cx.set_global(CodexBin(fake_codex())));
    let f = fixture(true);
    bind(&f, Some("oai"));
    let (ws, cx) = open_workspace(&f, cx);
    let src = project_thread(&f, &ws, cx);
    type_and_send(&src, cx);
    wait_done(&src, cx);
    src.update_in(cx, |t, window, cx| t.focus_prompt(window, cx));
    let (codex, claude) = cx.update(|window, cx| {
        (
            window.is_action_available(&ContinueInCodex, cx),
            window.is_action_available(&ContinueInClaude, cx),
        )
    });
    assert!(
        claude && !codex,
        "codex offered: {codex}, claude offered: {claude}"
    );
    rebind(&f, &ws, Some("acme"), cx);
    hand_off_and_send(&f, &ws, &src, Provider::Claude, "acme", cx);
}

fn handoffs(f: &Fixture, project_id: i64, src: i64) -> Vec<skyelab_store::Thread> {
    let s = f.store.lock().expect("lock");
    let rows = s.list_threads(project_id).expect("list");
    rows.into_iter()
        .filter(|t| t.forked_from == Some(src))
        .collect()
}

#[gpui::test]
fn handoff_dispatched_twice_creates_one_thread(cx: &mut TestAppContext) {
    setup(cx);
    cx.update(|cx| cx.set_global(CodexBin(fake_codex())));
    let f = fixture(true);
    bind(&f, Some("acme"));
    let (ws, cx) = open_workspace(&f, cx);
    let src = project_thread(&f, &ws, cx);
    type_and_send(&src, cx);
    wait_done(&src, cx);
    rebind(&f, &ws, Some("oai"), cx);
    let (project_id, src_id) = src.read_with(cx, |t, _| {
        (t.project_id, t.thread.as_ref().expect("row").id)
    });
    // Both clicks land before the first handoff's background work does.
    cx.update(|window, cx| {
        for _ in 0..2 {
            window.dispatch_action(
                Box::new(HandOffThread {
                    id: src_id,
                    to: Provider::Codex,
                }),
                cx,
            );
        }
    });
    cx.run_until_parked();
    let n = handoffs(&f, project_id, src_id).len();
    assert_eq!(n, 1, "DOUBLE_HANDOFF created {n} threads");
}

#[gpui::test]
fn handoff_keeps_the_source_worktree(cx: &mut TestAppContext) {
    setup(cx);
    cx.update(|cx| cx.set_global(CodexBin(fake_codex())));
    let f = fixture(true);
    bind(&f, Some("acme"));
    let (ws, cx) = open_workspace(&f, cx);
    let src = project_thread(&f, &ws, cx);
    let project_id = src.read_with(cx, |t, _| t.project_id);
    let wt = f.project_dir.join(".skyelab/worktrees/feat");
    std::fs::create_dir_all(&wt).expect("worktree dir");
    let wt_row = (f.store.lock().expect("lock"))
        .create_thread(&skyelab_store::NewThread {
            project_id,
            title: "feat",
            cwd: &wt,
            account_id: "acme",
            status: "stopped",
        })
        .expect("worktree thread");
    rebind(&f, &ws, Some("oai"), cx);
    cx.dispatch_action(HandOffThread {
        id: wt_row.id,
        to: Provider::Codex,
    });
    wait_until(cx, "the handoff row", |_| {
        !handoffs(&f, project_id, wt_row.id).is_empty()
    });
    let row = handoffs(&f, project_id, wt_row.id).remove(0);
    assert_eq!(row.account_id, "oai");
    assert_eq!(row.cwd, wt, "HANDOFF_LOST_WORKTREE");
}

/// The bind dialog lists the Codex accounts; picking one binds the folder to it and the handoff goes on, with no
/// folder picker and no account chosen for the user.
#[gpui::test]
fn the_bind_dialog_binds_the_picked_account_and_continues(cx: &mut TestAppContext) {
    setup(cx);
    cx.update(|cx| cx.set_global(CodexBin(fake_codex())));
    let f = fixture(true);
    bind(&f, Some("acme"));
    let (ws, cx) = open_workspace(&f, cx);
    rebind(&f, &ws, Some("acme"), cx);
    let src = project_thread(&f, &ws, cx);
    type_and_send(&src, cx);
    wait_done(&src, cx);
    let src_id = src.read_with(cx, |t, _| t.thread.as_ref().expect("row").id);
    cx.dispatch_action(HandOffThread {
        id: src_id,
        to: Provider::Codex,
    });
    wait_until(cx, "the bind dialog", |cx| {
        cx.update(|window, cx| window.has_active_dialog(cx))
    });
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("handoff-account-acme").is_none(),
        "only Codex accounts are offered"
    );
    let at = cx
        .debug_bounds("handoff-account-oai")
        .expect("the Codex account is offered")
        .center();
    cx.simulate_click(at, gpui::Modifiers::default());
    wait_until(cx, "the handoff opened", |cx| {
        ws.read_with(cx, |ws, _| {
            ws.threads.iter().any(|t| t.forked_from == Some(src_id))
        })
    });
    let row = ws
        .read_with(cx, |ws, _| {
            ws.threads
                .iter()
                .find(|t| t.forked_from == Some(src_id))
                .cloned()
        })
        .expect("row");
    assert_eq!(row.account_id, "oai");
    let saved = AccountsConfig::load(&f.accounts_path).expect("load");
    assert_eq!(
        saved.resolve(&f.project_dir).map(|a| a.id.as_str()),
        Some("oai"),
        "the folder is bound to the picked account"
    );
}
