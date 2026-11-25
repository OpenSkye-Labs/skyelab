//! Acceptance: the first screen after connecting is the desktop's flat thread list. Every project's threads, in
//! Pinned, Working, then by the day each last changed; each row says its project, status, account and preview.

use gpui::TestAppContext;
use skyelab_mobile::{
    app::{Connect, MobileApp, Screen},
    source::{FakeSource, ThreadSource},
    thread_rows::{row_detail, sections},
};
use skyelab_protocol::{Project, Thread};

fn thread(id: i64, status: &str, pinned: bool, updated_at: i64) -> Thread {
    Thread {
        id,
        project_id: 1,
        title: format!("t{id}"),
        account_id: "work".into(),
        account_label: Some("Work".into()),
        status: status.into(),
        pinned,
        preview: Some("## Done\nThe **tests** pass.".into()),
        settled_at: None,
        snoozed_until: None,
        created_at: 0,
        updated_at,
    }
}

#[test]
fn sections_follow_the_desktop() {
    // Today starts at 1000, yesterday at 900, the 7-day window at 300.
    let starts = [1_000, 900, 300];
    let threads = vec![
        thread(1, "idle", false, 1_500),
        thread(2, "running", false, 10),
        thread(3, "idle", true, 10),
        thread(4, "idle", false, 950),
        thread(5, "done", false, 400),
        thread(6, "failed", false, 10),
        thread(7, "starting", false, 1_200),
    ];
    let got: Vec<(&str, Vec<i64>)> = sections(threads, starts)
        .into_iter()
        .map(|(title, ts)| (title, ts.iter().map(|t| t.id).collect()))
        .collect();
    assert_eq!(
        got,
        [
            ("Pinned", vec![3]),
            ("Working", vec![7, 2]),
            ("Today", vec![1]),
            ("Yesterday", vec![4]),
            ("Previous 7 days", vec![5]),
            ("Older", vec![6]),
        ]
    );
    assert!(
        sections(Vec::new(), starts).is_empty(),
        "empty sections are left out"
    );
}

#[test]
fn a_row_says_project_status_account_and_preview() {
    let project = Project {
        id: 1,
        name: "skyelab".into(),
        path: "/x".into(),
    };
    let detail = row_detail(&thread(1, "idle", false, 0), Some(&project));
    assert_eq!(detail, "skyelab · Work · idle\nDone The tests pass.");
    let mut bare = thread(2, "running", false, 0);
    bare.account_label = None;
    bare.preview = None;
    assert_eq!(row_detail(&bare, None), "work · running");
}

#[gpui::test]
fn connecting_lands_on_every_projects_threads(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let dir = tempfile::tempdir().expect("temp dir");
    let data = dir.path().to_path_buf();
    let connect: Connect =
        Box::new(|_: &str| Ok(Box::new(FakeSource::new()) as Box<dyn ThreadSource>));
    let (app, cx) = cx.add_window_view(|window, cx| MobileApp::new(data, connect, window, cx));
    let input = app.read_with(cx, |a, _| a.env_input.clone());
    input.update_in(cx, |s, window, cx| {
        s.set_value("laptop.tail1234.ts.net", window, cx)
    });
    app.update_in(cx, |a, window, cx| a.connect(window, cx));
    app.read_with(cx, |a, _| {
        assert_eq!(a.screen, Screen::Threads);
        let projects: std::collections::BTreeSet<i64> =
            a.threads().iter().map(|t| t.project_id).collect();
        assert!(
            projects.len() >= 2,
            "threads of every project: {projects:?}"
        );
    });
}
