//! Review fixes: the shown page follows projects that load after it, and Back never shows this machine's GitHub
//! page while another machine is shown.

use super::super::center::Page;
use super::super::manage_harness::rig;
use super::super::threads_test::wait_until;
use super::tests::{git_repo, page, settled, setup};
use super::*;

#[gpui::test]
fn the_shown_page_lists_a_project_that_loads_after_it(cx: &mut gpui::TestAppContext) {
    let (r, cx) = rig(cx);
    setup(&r, None, vec![], &[], cx);
    cx.dispatch_action(OpenIssues);
    let names = |cx: &mut gpui::VisualTestContext| {
        let p = page(&r, Kind::Issues, cx);
        settled(&r, &p, cx)
            .into_iter()
            .map(|g| g.0)
            .collect::<Vec<_>>()
    };
    assert_eq!(names(cx), ["demo"]);
    // As at a restore: the projects load (here, one more) while the page is already shown.
    let tool = r.f.project_dir.parent().expect("root").join("tool");
    git_repo(&tool, "https://github.com/acme/tool");
    let mut config = AccountsConfig::load(&r.f.accounts_path).expect("load");
    config
        .add_binding(skyelab_core::Binding::new(tool.clone(), "acme".into()))
        .expect("bind");
    config.save(&r.f.accounts_path).expect("save");
    r.ws.update(cx, |ws, cx| {
        ws.config = config;
        ws.add_project(tool, cx)
    });
    wait_until(cx, "the project stored", |cx| {
        r.ws.read_with(cx, |ws, _| ws.projects.len() == 2)
    });
    cx.run_until_parked();
    assert_eq!(names(cx), ["demo", "tool"]);
}

#[gpui::test]
fn back_never_shows_a_local_github_page_on_another_machine(cx: &mut gpui::TestAppContext) {
    use super::super::env_harness::{
        FakeTailnet, RemoteSideExt as _, connect, local_workspace, remote_side,
    };
    let remote = remote_side(FakeTailnet::default());
    let (ws, _, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    cx.dispatch_action(OpenPullRequests);
    cx.run_until_parked();
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.page.clone()),
        Page::Github(Kind::Prs)
    );
    connect(&ws, &remote, cx);
    ws.update_in(cx, |ws, window, cx| ws.show(Page::Welcome, window, cx));
    cx.dispatch_action(super::super::GoBack);
    cx.run_until_parked();
    let page = ws.read_with(cx, |ws, _| ws.page.clone());
    assert!(!matches!(page, Page::Github(_)), "{page:?}");
}
