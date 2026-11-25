//! Acceptance: a project with no GitHub remote gets a plain empty state on both pages (never git's stderr), and gh
//! is never asked about it, signed in or not.

use super::super::manage_harness::rig;
use super::tests::{account, calls, gh_dir, page, settled, setup};
use super::*;
use skyelab_git::NoGitHub;

#[cfg(unix)] // the fake gh is a shell script
#[gpui::test]
fn projects_without_a_github_origin_say_why(cx: &mut gpui::TestAppContext) {
    let (r, cx) = rig(cx);
    let root = r.f.project_dir.parent().expect("root").to_path_buf();
    let gh_b = gh_dir(&root, "gh-beta", "github.com:\n");
    let beta = account("beta", &root, Some(gh_b.clone()));
    setup(
        &r,
        None,
        vec![beta],
        &[
            ("lab", "https://gitlab.com/beta/lab.git", Some("beta")),
            ("local", "https://github.com/beta/local.git", Some("beta")),
        ],
        cx,
    );
    let ok = std::process::Command::new("git")
        .args(["remote", "remove", "origin"])
        .current_dir(root.join("local"))
        .status()
        .expect("git")
        .success();
    assert!(ok);
    // demo is on GitHub with no gh login: it asks to sign in. The beta projects say what their remote lacks,
    // and beta's gh (signed in) is never run.
    for kind in [Kind::Prs, Kind::Issues] {
        match kind {
            Kind::Prs => cx.dispatch_action(OpenPullRequests),
            Kind::Issues => cx.dispatch_action(OpenIssues),
        }
        let groups = settled(&r, &page(&r, kind, cx), cx);
        let listing = |name: &str| {
            groups
                .iter()
                .find(|g| g.0 == name)
                .map(|g| g.2.clone())
                .expect("group shown")
        };
        assert_eq!(listing("demo"), Listing::SignIn);
        assert_eq!(listing("lab"), Listing::NoGitHub(NoGitHub::OtherHost));
        assert_eq!(listing("local"), Listing::NoGitHub(NoGitHub::NoOrigin));
    }
    assert!(calls(&gh_b).is_empty(), "{:?}", calls(&gh_b));
}
