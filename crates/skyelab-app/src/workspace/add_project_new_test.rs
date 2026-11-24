//! Acceptance: New project creates the folder with a git repo, binds it as picked and adds it; a folder that already
//! has files is refused.

use super::add_project_rig::{choose, rig, shown, type_in};
use gpui::TestAppContext;

#[gpui::test]
fn new_project_creates_a_repo_and_binds_the_pick(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx, |_, _| {});
    rig.open_add(cx);
    choose("New project", cx);
    assert_eq!(shown(cx).page.as_deref(), Some("New project"));
    type_in("app", cx);
    let s = shown(cx);
    assert_eq!(s.rows.len(), 1);
    assert!(s.rows[0].1.contains("already has files"), "{s:?}");

    cx.simulate_keystrokes("cmd-a backspace");
    type_in("demo", cx);
    choose("Create ~/code/demo", cx);
    assert_eq!(shown(cx).page.as_deref(), Some("Which account does demo run on?"));
    let demo = rig.home.join("code/demo");
    assert!(!demo.exists(), "nothing made before the pick");
    choose("WORK", cx);
    rig.wait_project(&demo, cx);
    assert!(demo.join(".git").is_dir(), "git init ran");
    assert_eq!(rig.bound_to(&demo).as_deref(), Some("work"));
}
