//! Criterion 16: a linked caller lists exactly the projects whose folder binds to its linked account here.

use super::remote_delegation_harness::*;
use gpui::TestAppContext;
use skyelab_protocol::{DelegateOp, ErrorCode};

#[gpui::test]
fn lists_only_the_linked_accounts_projects(cx: &mut TestAppContext) {
    let (b, cx) = b_side(
        Setup {
            link_to: Some("work"),
            ..Setup::default()
        },
        cx,
    );
    let listed = call(&b, &caller(), DelegateOp::ListProjects, cx).expect("listed");
    let mut names: Vec<&str> = listed
        .as_array()
        .expect("an array")
        .iter()
        .map(|p| p["name"].as_str().expect("name"))
        .collect();
    names.sort_unstable();
    assert_eq!(names, ["work-a", "work-b"]);
}

#[gpui::test]
fn an_unlinked_caller_lists_nothing(cx: &mut TestAppContext) {
    let (b, cx) = b_side(Setup::default(), cx);
    let e = call(&b, &caller(), DelegateOp::ListProjects, cx).expect_err("refused");
    assert_eq!(e.code, ErrorCode::DelegationNotAllowed);
}
