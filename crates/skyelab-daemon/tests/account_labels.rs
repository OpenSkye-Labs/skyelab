//! Remote threads carry their account's label from the serving machine's accounts.toml, so a client can show
//! "Work (Team)" rather than `work`. The label is additive on the wire (protocol 1.1).

use std::path::Path;

use skyelab_core::{Account, AccountsConfig};
use skyelab_daemon::{Backend, NotWired, StoreBackend};
use skyelab_protocol::{Thread, VERSION};
use skyelab_store::{NewThread, Store};

fn store() -> (Store, i64) {
    let store = Store::open_in_memory().expect("store");
    let project = store
        .create_project("app", Path::new("/src/app"))
        .expect("project");
    for account in ["work", "home"] {
        store
            .create_thread(&NewThread {
                project_id: project.id,
                title: account,
                cwd: Path::new("/src/app"),
                account_id: account,
                status: "idle",
            })
            .expect("thread");
    }
    (store, project.id)
}

fn labels(backend: &dyn Backend, project_id: i64) -> Vec<(String, Option<String>)> {
    let mut threads = backend.list_threads(project_id).expect("threads");
    threads.sort_by_key(|t| t.id);
    threads
        .into_iter()
        .map(|t| (t.account_id, t.account_label))
        .collect()
}

#[test]
fn threads_carry_the_label_from_accounts_toml() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("accounts.toml");
    AccountsConfig {
        accounts: vec![Account {
            id: "work".into(),
            label: "Work (Team)".into(),
            config_dir: dir.path().join("claude-work"),
        }],
        ..AccountsConfig::default()
    }
    .save(&path)
    .expect("save accounts");
    let (store, project) = store();
    let backend = StoreBackend::new(store, NotWired).with_accounts(Some(path.clone()));
    assert_eq!(
        labels(&backend, project),
        [
            ("work".into(), Some("Work (Team)".into())),
            ("home".into(), None), // not in this machine's accounts
        ]
    );

    // A file that no longer loads sends no labels, never the last good ones.
    std::fs::write(&path, "accounts = [[[").expect("break the file");
    assert_eq!(
        labels(&backend, project),
        [("work".into(), None), ("home".into(), None)]
    );
}

#[test]
fn no_accounts_file_means_no_labels() {
    let (store, project) = store();
    let backend = StoreBackend::new(store, NotWired).with_accounts(None);
    assert!(labels(&backend, project).iter().all(|(_, l)| l.is_none()));
}

#[test]
fn the_label_round_trips_and_older_peers_still_parse() {
    assert!(VERSION.minor >= 1, "an additive field is a minor bump");
    let thread = Thread {
        id: 1,
        project_id: 2,
        title: "t".into(),
        account_id: "work".into(),
        account_label: Some("Work (Team)".into()),
        status: "idle".into(),
        created_at: 0,
        updated_at: 0,
    };
    let json = serde_json::to_string(&thread).expect("json");
    assert_eq!(
        serde_json::from_str::<Thread>(&json).expect("parse"),
        thread
    );

    // Without a label the field is left out, so a 1.0 peer sees exactly what it used to.
    let bare = Thread {
        account_label: None,
        ..thread
    };
    let json = serde_json::to_string(&bare).expect("json");
    assert!(!json.contains("account_label"), "{json}");
    // And a 1.0 server's thread, with no such field, parses as no label.
    assert_eq!(
        serde_json::from_str::<Thread>(&json).expect("parse"),
        bare
    );
}
