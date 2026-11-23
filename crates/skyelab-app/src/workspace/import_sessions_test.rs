//! Acceptance: Import sessions lists only the bound account's sessions for the folder, imports them as threads that
//! carry the session id and its history, resumes them on send, never imports one twice, and writes nothing in the
//! account dirs.

use super::import_sessions::ImportSessions;
use super::manage_harness::rig;
use super::thread_model::Record;
use super::threads_test::resume::{env_reports, send_text};
use super::threads_test::{stored_records, wait_until};
use gpui::TestAppContext;
use gpui_component::WindowExt as _;
use skyelab_agent::{Block, Event};
use skyelab_core::{Account, AccountsConfig, Binding};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const OURS: &str = "5f1c2a3b-0000-4000-8000-00000000c1a0";

/// Writes the agent crate's Claude fixture as session `id` in `home`, recorded as run in `cwd`.
fn session(home: &Path, cwd: &Path, id: &str) {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../skyelab-agent/tests/fixtures/claude-transcript.jsonl");
    let text = std::fs::read_to_string(fixture).expect("fixture is in the repo");
    let cwd_json = serde_json::to_string(cwd.to_str().expect("utf-8")).expect("a string");
    let dir = home
        .join("projects")
        .join(skyelab_agent::import::claude_dir_name(cwd));
    std::fs::create_dir_all(&dir).expect("temp dir");
    std::fs::write(
        dir.join(format!("{id}.jsonl")),
        text.replace("__CWD__", &cwd_json[1..cwd_json.len() - 1]),
    )
    .expect("temp dir");
}

/// Every file under `dir` with its bytes and modified time.
fn snapshot(dir: &Path) -> BTreeMap<PathBuf, (Vec<u8>, std::time::SystemTime)> {
    let mut out = BTreeMap::new();
    let mut todo = vec![dir.to_owned()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).expect("readable").flatten() {
            let path = e.path();
            if path.is_dir() {
                todo.push(path);
            } else {
                let meta = e.metadata().expect("metadata");
                let bytes = std::fs::read(&path).expect("readable");
                out.insert(path, (bytes, meta.modified().expect("mtime")));
            }
        }
    }
    out
}

#[gpui::test]
fn import_sessions_lists_the_bound_accounts_sessions_and_imports_them_once(
    cx: &mut TestAppContext,
) {
    let (rig, cx) = rig(cx);
    let f = &rig.f;
    let other_dir = f.account_dir.with_file_name(".claude-other");
    let sub = f.project_dir.join("vendor");
    std::fs::create_dir_all(&sub).expect("temp dir");
    // `vendor` is bound to another account: its sessions (even in acme's dir) are not acme's to continue, and the
    // other account's sessions for the project folder are never offered.
    let mut config = AccountsConfig::load(&f.accounts_path).expect("load");
    config
        .add_account(Account {
            id: "other".into(),
            label: "Other Co".into(),
            config_dir: other_dir.clone(),
            gh_config_dir: None,
            ssh_agent: false,
            provider: Default::default(),
        })
        .expect("add account");
    config
        .add_binding(Binding::new(sub.clone(), "other".into()))
        .expect("bind");
    config.save(&f.accounts_path).expect("save");
    let file = f.accounts_path.clone();
    cx.update(|_, cx| cx.set_global(super::AccountsFile(file)));
    session(&f.account_dir, &f.project_dir, OURS);
    session(&f.account_dir, &sub, "00000000-0000-4000-8000-0000000000b2");
    session(
        &other_dir,
        &f.project_dir,
        "00000000-0000-4000-8000-0000000000c3",
    );
    let before = (snapshot(&f.account_dir), snapshot(&other_dir));

    let picker = |cx: &mut gpui::VisualTestContext| {
        rig.ws.update(cx, |ws, _| ws.import_picker.take());
        cx.dispatch_action(ImportSessions);
        let ws = rig.ws.clone();
        wait_until(cx, "the picker opened", |cx| {
            ws.read_with(cx, |ws, _| ws.import_picker.is_some())
        });
        rig.ws
            .read_with(cx, |ws, _| ws.import_picker.clone())
            .and_then(|p| p.upgrade())
            .expect("picker")
    };

    let p = picker(cx);
    let listed: Vec<(String, bool)> = p.read_with(cx, |p, _| {
        p.rows
            .iter()
            .map(|r| (r.t.id.clone(), r.imported))
            .collect()
    });
    assert_eq!(listed, vec![(OURS.to_owned(), false)]);
    p.update_in(cx, |p, window, cx| {
        p.toggle(0, cx);
        p.import(window, cx)
    });
    let ws = rig.ws.clone();
    wait_until(cx, "the imported thread", |cx| {
        ws.read_with(cx, |ws, _| {
            ws.threads
                .iter()
                .any(|t| t.agent_session_id.as_deref() == Some(OURS))
        })
    });
    let row = rig
        .ws
        .read_with(cx, |ws, _| {
            ws.threads
                .iter()
                .find(|t| t.agent_session_id.as_deref() == Some(OURS))
                .cloned()
        })
        .expect("row");
    assert_eq!(
        (row.title.as_str(), row.account_id.as_str()),
        ("Login fix", "acme")
    );
    assert_eq!(row.cwd, f.project_dir);
    let records = stored_records(&f.store, row.id);
    assert_eq!(
        records.first(),
        Some(&Record::Prompt("Fix the login bug".into()))
    );
    assert!(records.iter().any(|r| matches!(
        r,
        Record::Agent(Event::Assistant { content }) if matches!(content.as_slice(), [Block::ToolUse { name, .. }] if name == "Bash")
    )));
    assert!(matches!(
        records.last(),
        Some(Record::Agent(Event::Result { .. }))
    ));
    assert!(
        rig.notices(cx)
            .iter()
            .any(|n| n.starts_with("Imported 1 session.")),
        "{:?}",
        rig.notices(cx)
    );

    // Again: marked, nothing to pick, and the store refuses a second thread for it.
    let p = picker(cx);
    p.read_with(cx, |p, _| {
        assert!(p.rows[0].imported);
        assert!(p.picked().is_empty());
    });
    let (project, picks) = p.read_with(cx, |p, _| {
        (
            p.project.clone(),
            p.rows.iter().map(|r| r.t.clone()).collect::<Vec<_>>(),
        )
    });
    let config = AccountsConfig::load(&f.accounts_path).expect("load");
    let again = super::import_sessions::import_picks(&f.store, &config, &project, "acme", &picks)
        .expect("runs");
    assert!(again.threads.is_empty());
    let sessions = f
        .store
        .lock()
        .expect("lock")
        .list_threads(rig.project)
        .expect("threads")
        .iter()
        .filter(|t| t.agent_session_id.as_deref() == Some(OURS))
        .count();
    assert_eq!(sessions, 1);

    assert_eq!(
        (snapshot(&f.account_dir), snapshot(&other_dir)),
        before,
        "account dirs untouched"
    );

    // Sending continues the same session under the bound account.
    cx.update(|window, cx| window.close_dialog(cx));
    let view = rig.open(row.id, cx);
    send_text(&view, "go on", cx);
    wait_until(cx, "the resumed agent's env report", |_| {
        !env_reports(&f.store, row.id).is_empty()
    });
    let args = env_reports(&f.store, row.id)[0]["args"]
        .as_str()
        .expect("args")
        .to_owned();
    assert!(args.contains(&format!("--resume {OURS}")), "{args}");
}
