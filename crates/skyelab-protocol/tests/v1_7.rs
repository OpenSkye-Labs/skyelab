//! Protocol 1.7: the shell snapshot, its pushed changes, and the thread row's sidebar fields. Every addition is
//! optional on the wire, so a 1.6 peer's rows still parse and a 1.6 client ignores the new fields.

use serde_json::json;
use skyelab_protocol::{
    Call, Project, Reply, Request, ServerMessage, ShellChange, ShellDelta, ShellSnapshot, Thread,
    ThreadRow,
};

fn thread(id: i64, project_id: i64, updated_at: i64) -> Thread {
    Thread {
        id,
        project_id,
        title: format!("t{id}"),
        account_id: "work".into(),
        status: "idle".into(),
        updated_at,
        ..Thread::default()
    }
}

#[test]
fn the_row_fields_round_trip_and_stay_off_the_wire_when_empty() {
    let mut t = thread(3, 1, 2);
    let wire = serde_json::to_value(&t).expect("serializes");
    for key in [
        "cwd",
        "permission_mode",
        "worktree_branch",
        "archived_at",
        "unread",
    ] {
        assert!(wire.get(key).is_none(), "{key} sent while empty: {wire}");
    }
    t.row = ThreadRow {
        cwd: "/src/app".into(),
        permission_mode: "plan".into(),
        worktree_branch: Some("fix".into()),
        pr_state: Some("open".into()),
        archived_at: Some(9),
        unread: true,
        ..ThreadRow::default()
    };
    let wire = serde_json::to_value(&t).expect("serializes");
    assert_eq!(wire["worktree_branch"], "fix", "flat on the thread: {wire}");
    assert_eq!(serde_json::from_value::<Thread>(wire).expect("parses"), t);
}

#[test]
fn a_1_6_thread_parses_with_an_empty_row() {
    let old = json!({"id": 3, "project_id": 1, "title": "t", "account_id": "work", "status": "idle",
                     "created_at": 1, "updated_at": 2});
    let t: Thread = serde_json::from_value(old).expect("a 1.6 thread parses");
    assert_eq!(t.row, ThreadRow::default());
    let p: Project =
        serde_json::from_value(json!({"id": 1, "name": "app", "path": "/a"})).expect("project");
    assert_eq!(p.created_at, 0);
}

#[test]
fn subscribe_snapshot_and_change_round_trip() {
    let req = Request {
        id: 4,
        call: Call::SubscribeShell,
    };
    let text = serde_json::to_string(&req).expect("serializes");
    assert_eq!(text, r#"{"id":4,"method":"subscribe_shell"}"#);
    assert_eq!(Request::parse(&text).expect("parses"), req);

    let snap = ShellSnapshot {
        seq: 7,
        projects: vec![Project {
            id: 1,
            name: "app".into(),
            path: "/a".into(),
            created_at: 5,
        }],
        threads: vec![thread(3, 1, 2)],
        truncated: true,
    };
    let reply = ServerMessage::Response {
        id: 4,
        result: Reply::Shell(snap),
    };
    assert_eq!(
        serde_json::from_str::<ServerMessage>(&reply.to_json()).expect("parses"),
        reply
    );

    let push = ServerMessage::Shell(ShellChange {
        seq: 8,
        delta: ShellDelta::ThreadRemoved { id: 3 },
    });
    let text = push.to_json();
    assert!(text.contains(r#""type":"shell""#), "{text}");
    assert_eq!(
        serde_json::from_str::<ServerMessage>(&text).expect("parses"),
        push
    );
}

#[test]
fn apply_keeps_the_snapshot_order_and_is_idempotent() {
    let mut s = ShellSnapshot {
        seq: 0,
        projects: vec![Project {
            id: 1,
            name: "b".into(),
            ..Project::default()
        }],
        threads: vec![thread(1, 1, 10), thread(2, 1, 5)],
        truncated: false,
    };
    s.apply(ShellDelta::ProjectUpdated(Project {
        id: 2,
        name: "a".into(),
        ..Project::default()
    }));
    assert_eq!(
        s.projects.iter().map(|p| p.id).collect::<Vec<_>>(),
        [2, 1],
        "by name"
    );

    let mut renamed = thread(2, 1, 20);
    renamed.title = "renamed".into();
    s.apply(ShellDelta::ThreadUpdated(Box::new(renamed.clone())));
    s.apply(ShellDelta::ThreadUpdated(Box::new(renamed)));
    assert_eq!(
        s.threads.iter().map(|t| t.id).collect::<Vec<_>>(),
        [2, 1],
        "newest first, no duplicate"
    );
    assert_eq!(s.threads[0].title, "renamed");

    s.apply(ShellDelta::ThreadUpdated(Box::new(thread(3, 2, 1))));
    s.apply(ShellDelta::ProjectRemoved { id: 1 });
    assert_eq!(
        s.threads.iter().map(|t| t.id).collect::<Vec<_>>(),
        [3],
        "its threads go with it"
    );
    s.apply(ShellDelta::ThreadRemoved { id: 3 });
    s.apply(ShellDelta::ThreadRemoved { id: 3 });
    assert!(s.threads.is_empty());
}
