//! Acceptance: the Pull requests and Issues pages read each project with its own account's gh (fake gh only), ask
//! nothing of an account with no GitHub login, cache per filter, and start review and issue threads.

use super::super::center::Page;
use super::super::manage_harness::{Rig, rig};
use super::super::thread::composer::composer_context::ChipKind;
use super::super::threads_test::wait_until;
use super::*;
use gpui::VisualTestContext;
use skyelab_core::{Account, Binding};
use std::path::Path;

pub(super) fn git_repo(dir: &Path, origin: &str) {
    std::fs::create_dir_all(dir).expect("mkdir");
    for args in [&["init", "-q"][..], &["remote", "add", "origin", origin]] {
        let ok = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .status()
            .expect("git")
            .success();
        assert!(ok, "git {args:?}");
    }
}

/// A gh config dir; logged in to github.com unless `hosts` says otherwise.
fn gh_dir(root: &Path, name: &str, hosts: &str) -> PathBuf {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).expect("mkdir");
    std::fs::write(dir.join("fake-login"), "octo").expect("login");
    std::fs::write(dir.join("hosts.yml"), hosts).expect("hosts");
    dir
}

fn calls(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("fake-gh.list"))
        .map(|t| t.lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

fn account(id: &str, root: &Path, gh: Option<PathBuf>) -> Account {
    Account {
        id: id.into(),
        label: format!("{id} label"),
        config_dir: root.join(format!(".claude-{id}")),
        provider: Default::default(),
        gh_config_dir: gh,
        ssh_agent: false,
    }
}

/// The rig's project (account acme, gh `acme_gh`) plus `extra` projects: (folder name, origin, binding).
pub(super) fn setup(
    r: &Rig,
    acme_gh: Option<PathBuf>,
    accounts: Vec<Account>,
    extra: &[(&str, &str, Option<&str>)],
    cx: &mut VisualTestContext,
) -> AccountsConfig {
    let root = r.f.project_dir.parent().expect("root").to_path_buf();
    git_repo(&r.f.project_dir, "https://github.com/acme/demo.git");
    let mut config = AccountsConfig::load(&r.f.accounts_path).expect("load");
    config.accounts[0].gh_config_dir = acme_gh;
    for a in accounts {
        config.add_account(a).expect("account");
    }
    for (name, origin, bound) in extra {
        git_repo(&root.join(name), origin);
        if let Some(id) = bound {
            config
                .add_binding(Binding::new(root.join(name), (*id).into()))
                .expect("bind");
        }
    }
    config.save(&r.f.accounts_path).expect("save");
    let fake =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../skyelab-git/tests/fixtures/fake-gh.sh");
    let c = config.clone();
    r.ws.update(cx, |ws, cx| {
        cx.set_global(super::super::GhBin(fake));
        ws.config = c;
        for (name, _, _) in extra {
            ws.add_project(root.join(name), cx);
        }
    });
    let want = 1 + extra.len();
    wait_until(cx, "projects stored", |cx| {
        r.ws.read_with(cx, |ws, _| ws.projects.len() == want)
    });
    config
}

pub(super) fn page(r: &Rig, kind: Kind, cx: &mut VisualTestContext) -> Entity<GhPage> {
    r.ws.read_with(cx, |ws, _| match kind {
        Kind::Prs => ws.gh_pages.prs.clone(),
        Kind::Issues => ws.gh_pages.issues.clone(),
    })
    .expect("page made")
}

/// Each shown group's (project folder name, account id, listing), once none is loading.
pub(super) fn settled(
    r: &Rig,
    page: &Entity<GhPage>,
    cx: &mut VisualTestContext,
) -> Vec<(String, Option<String>, Listing)> {
    let read = |cx: &mut VisualTestContext| {
        page.read_with(cx, |p, _| {
            p.shown()
                .iter()
                .map(|(s, g)| {
                    let name = s
                        .path
                        .file_name()
                        .expect("name")
                        .to_string_lossy()
                        .into_owned();
                    (
                        name,
                        g.account.as_ref().map(|a| a.0.clone()),
                        g.listing.clone(),
                    )
                })
                .collect::<Vec<_>>()
        })
    };
    wait_until(cx, "every group read", |cx| {
        read(cx).iter().all(|g| g.2 != Listing::Loading)
    });
    let _ = r;
    read(cx)
}

#[cfg(unix)] // the fake gh is a shell script
#[gpui::test]
fn each_repo_is_listed_with_its_own_accounts_gh(cx: &mut gpui::TestAppContext) {
    let (r, cx) = rig(cx);
    let root = r.f.project_dir.parent().expect("root").to_path_buf();
    let gh_a = gh_dir(&root, "gh-acme", "github.com:\n");
    let gh_b = gh_dir(&root, "gh-beta", "github.com:\n");
    let beta = account("beta", &root, Some(gh_b.clone()));
    setup(
        &r,
        Some(gh_a.clone()),
        vec![beta],
        &[("tool", "git@github.com:beta/tool.git", Some("beta"))],
        cx,
    );

    // Opened without parking: every group shows Loading, since the reads run off the UI thread.
    let loading = r.ws.update_in(cx, |ws, window, cx| {
        ws.open_pull_requests(&OpenPullRequests, window, cx);
        let p = ws.gh_pages.prs.clone().expect("page made");
        let p = p.read(cx);
        p.shown().len() == 2 && p.shown().iter().all(|(_, g)| g.listing == Listing::Loading)
    });
    assert!(loading);
    assert_eq!(
        r.ws.read_with(cx, |ws, _| ws.page.clone()),
        Page::Github(Kind::Prs)
    );
    let p = page(&r, Kind::Prs, cx);

    let groups = settled(&r, &p, cx);
    assert_eq!(groups.len(), 2, "{groups:?}");
    for (name, account, listing) in &groups {
        let want = if name == "demo" { "acme" } else { "beta" };
        assert_eq!(account.as_deref(), Some(want));
        assert!(
            matches!(listing, Listing::Prs(l) if l.len() == 2),
            "{listing:?}"
        );
    }
    let (a, b) = (calls(&gh_a), calls(&gh_b));
    assert_eq!((a.len(), b.len()), (1, 1), "{a:?} {b:?}");
    assert!(
        a[0].contains("pr list") && a[0].contains("--repo github.com/acme/demo"),
        "{a:?}"
    );
    assert!(b[0].contains("--repo github.com/beta/tool"), "{b:?}");
    for (log, dir) in [(&a[0], &gh_a), (&b[0], &gh_b)] {
        assert!(log.contains(&format!("cwd={} ", dir.display())), "{log}");
        assert!(log.contains(&format!("cfg={} ", dir.display())), "{log}");
        assert!(log.ends_with("token="), "{log}");
    }
}

#[gpui::test]
fn no_login_means_no_gh_call_and_unbound_projects_are_left_out(cx: &mut gpui::TestAppContext) {
    let (r, cx) = rig(cx);
    let root = r.f.project_dir.parent().expect("root").to_path_buf();
    let gh_b = gh_dir(&root, "gh-beta", "gitlab.com:\n");
    let beta = account("beta", &root, Some(gh_b.clone()));
    setup(
        &r,
        None,
        vec![beta],
        &[
            ("tool", "https://github.com/beta/tool", Some("beta")),
            ("loose", "https://github.com/x/loose", None),
        ],
        cx,
    );
    cx.dispatch_action(OpenIssues);
    let groups = settled(&r, &page(&r, Kind::Issues, cx), cx);
    let names: Vec<_> = groups.iter().map(|g| g.0.as_str()).collect();
    assert_eq!(names, ["demo", "tool"], "the unbound one is not listed");
    assert!(groups.iter().all(|g| g.2 == Listing::SignIn), "{groups:?}");
    assert!(calls(&gh_b).is_empty());
    assert!(!root.join("loose").join("fake-gh.list").exists());
}

#[cfg(unix)] // the fake gh is a shell script
#[gpui::test]
fn filters_are_cached_and_refresh_reads_again(cx: &mut gpui::TestAppContext) {
    let (r, cx) = rig(cx);
    let root = r.f.project_dir.parent().expect("root").to_path_buf();
    let gh_a = gh_dir(&root, "gh-acme", "github.com:\n");
    std::fs::write(
        gh_a.join("fake-issues.json"),
        r#"[{"number":1,"title":"a","url":"https://github.com/acme/demo/issues/1","labels":[{"name":"bug"}]},
            {"number":2,"title":"b","url":"https://github.com/acme/demo/issues/2","labels":[{"name":"docs"}]}]"#,
    )
    .expect("fixture");
    std::fs::write(
        gh_a.join("fake-prs.json"),
        r#"[{"number":9,"title":"only","url":"https://github.com/acme/demo/pull/9","isDraft":true}]"#,
    )
    .expect("fixture");
    setup(&r, Some(gh_a.clone()), vec![], &[], cx);
    cx.dispatch_action(OpenPullRequests);
    let p = page(&r, Kind::Prs, cx);
    let groups = settled(&r, &p, cx);
    assert!(
        matches!(&groups[0].2, Listing::Prs(l) if l.len() == 1 && l[0].row.number == 9 && l[0].row.draft),
        "{groups:?}"
    );
    assert_eq!(calls(&gh_a).len(), 1);

    p.update(cx, |p, cx| p.set_filter(Filter::Pr(PrFilter::Mine), cx));
    settled(&r, &p, cx);
    assert_eq!(calls(&gh_a).len(), 2);
    assert!(calls(&gh_a)[1].contains("--author @me"));
    p.update(cx, |p, cx| {
        p.set_filter(Filter::Pr(PrFilter::ReviewRequested), cx)
    });
    settled(&r, &p, cx);
    assert!(calls(&gh_a)[2].contains("--search review-requested:@me"));

    // Back to a read filter: no gh call. Refresh: one.
    p.update(cx, |p, cx| p.set_filter(Filter::Pr(PrFilter::All), cx));
    cx.run_until_parked();
    assert_eq!(calls(&gh_a).len(), 3);
    p.update(cx, |p, cx| p.refresh(cx));
    settled(&r, &p, cx);
    assert_eq!(calls(&gh_a).len(), 4);

    // Issues: a picked label narrows the list; picking it again shows all.
    cx.dispatch_action(OpenIssues);
    let p = page(&r, Kind::Issues, cx);
    let groups = settled(&r, &p, cx);
    let Listing::Issues(list) = &groups[0].2 else {
        panic!("{groups:?}")
    };
    let numbers = |p: &Entity<GhPage>, cx: &mut VisualTestContext| {
        p.read_with(cx, |p, _| {
            p.issues_shown(list)
                .iter()
                .map(|r| r.row.number)
                .collect::<Vec<_>>()
        })
    };
    assert_eq!(numbers(&p, cx), [1, 2]);
    p.update(cx, |p, cx| p.pick_label(Some("bug".into()), cx));
    assert_eq!(numbers(&p, cx), [1]);
    p.update(cx, |p, cx| p.pick_label(Some("bug".into()), cx));
    assert_eq!(numbers(&p, cx), [1, 2]);
}

#[test]
fn only_github_https_links_open_in_the_browser() {
    assert!(browser_url("https://github.com/a/b/pull/1").is_some());
    for bad in [
        "javascript:alert(1)",
        "file:///etc/passwd",
        "https://evil.com/github.com/",
        "http://github.com/a",
    ] {
        assert_eq!(browser_url(bad), None, "{bad}");
    }
}

#[cfg(unix)]
#[gpui::test]
fn review_pr_starts_a_thread_that_links_the_pr(cx: &mut gpui::TestAppContext) {
    let (r, cx) = rig(cx);
    cx.executor().allow_parking();
    let url = "https://github.com/acme/demo/pull/12";
    let project = r.project;
    // An unsent new-thread draft the user left in this project.
    let owner = format!("project:{project}");
    r.f.store
        .lock()
        .expect("lock")
        .set_draft_text(&owner, "my own notes")
        .expect("draft");
    cx.dispatch_action(OpenPullRequests);
    let p = page(&r, Kind::Prs, cx);
    p.update(cx, |_, cx| {
        cx.emit(GhEvent::Review {
            project,
            number: 12,
            url: url.into(),
        })
    });
    cx.run_until_parked();
    let view =
        r.ws.read_with(cx, |ws, _| ws.current_thread())
            .expect("a draft shows");
    let text = view.read_with(cx, |v, cx| v.prompt.read(cx).value().to_string());
    assert!(
        text.starts_with("Review pull request #12: ") && text.contains(url),
        "{text}"
    );
    cx.run_until_parked();
    let kept =
        r.f.store
            .lock()
            .expect("lock")
            .draft(&owner)
            .expect("draft")
            .text;
    assert_eq!(
        kept, "my own notes",
        "the review prompt must not replace the project's draft"
    );

    view.update_in(cx, |v, window, cx| {
        v.send_prompt(&super::super::thread::SendPrompt, window, cx)
    });
    wait_until(cx, "the row saved", |cx| {
        view.read_with(cx, |v, _| v.thread.is_some())
    });
    let id = view
        .read_with(cx, |v, _| v.thread.as_ref().map(|t| t.id))
        .expect("row");
    assert_eq!(r.row(id).and_then(|t| t.pr_url).as_deref(), Some(url));
}

#[gpui::test]
fn issue_thread_gets_a_bound_capped_chip_that_a_rebind_drops(cx: &mut gpui::TestAppContext) {
    let (r, cx) = rig(cx);
    let root = r.f.project_dir.parent().expect("root").to_path_buf();
    cx.dispatch_action(OpenIssues);
    let p = page(&r, Kind::Issues, cx);
    let row = IssueRow {
        number: 31,
        title: "Crash".into(),
        url: "https://github.com/acme/demo/issues/31".into(),
        author: "bob".into(),
        labels: vec!["bug".into()],
        assignees: vec![],
        updated_at: String::new(),
        body: "z".repeat(5000),
        closed: false,
    };
    let project = r.project;
    p.update(cx, |_, cx| {
        cx.emit(GhEvent::StartIssue {
            project,
            account: "acme".into(),
            row,
        })
    });
    cx.run_until_parked();
    let view =
        r.ws.read_with(cx, |ws, _| ws.current_thread())
            .expect("a draft shows");
    let chips = view.read_with(cx, |v, _| v.composer.contexts.clone());
    assert_eq!(chips.len(), 1);
    assert_eq!(
        (chips[0].kind, chips[0].label.as_ref()),
        (ChipKind::Issue, "Issue #31")
    );
    assert!(
        chips[0]
            .text
            .contains("https://github.com/acme/demo/issues/31")
    );
    let zs = chips[0].text.chars().filter(|c| *c == 'z').count();
    assert_eq!(zs, terminal_context::MAX_ISSUE_BODY_CHARS);

    let mut config = AccountsConfig::load(&r.f.accounts_path).expect("load");
    config
        .add_account(account("beta", &root, None))
        .expect("account");
    config.bindings.clear();
    config
        .add_binding(Binding::new(r.f.project_dir.clone(), "beta".into()))
        .expect("bind");
    view.update(cx, |v, cx| v.set_config(config, cx));
    assert!(view.read_with(cx, |v, _| v.composer.contexts.is_empty()));
}

#[gpui::test]
fn palette_lists_both_pages(cx: &mut gpui::TestAppContext) {
    let (r, cx) = rig(cx);
    let labels: Vec<String> = r.ws.update_in(cx, |ws, window, cx| {
        let focus = ws.focus.clone();
        ws.palette_entries(&focus, window, cx)
            .iter()
            .map(|e| e.label.to_string())
            .collect()
    });
    for want in ["Open Pull Requests", "Open Issues"] {
        assert!(labels.iter().any(|l| l == want), "{want}: {labels:?}");
    }
    cx.dispatch_action(OpenIssues);
    assert_eq!(
        r.ws.read_with(cx, |ws, _| ws.page.clone()),
        Page::Github(Kind::Issues)
    );
    assert_eq!(
        r.ws.read_with(cx, |ws, cx| ws.page.key(cx)),
        "github:issues"
    );
}

#[gpui::test]
fn remote_view_ignores_the_actions(cx: &mut gpui::TestAppContext) {
    use super::super::env_harness::{
        FakeTailnet, RemoteSideExt as _, connect, local_workspace, remote_side,
    };
    let remote = remote_side(FakeTailnet::default());
    let (ws, _, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    connect(&ws, &remote, cx);
    let before = ws.read_with(cx, |ws, _| ws.page.clone());
    assert!(ws.read_with(cx, |ws, _| ws.is_remote()));
    cx.dispatch_action(OpenPullRequests);
    cx.dispatch_action(OpenIssues);
    cx.run_until_parked();
    assert_eq!(ws.read_with(cx, |ws, _| ws.page.clone()), before);
}
