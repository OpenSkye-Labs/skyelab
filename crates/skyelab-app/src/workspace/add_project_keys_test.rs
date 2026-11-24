//! Acceptance: the Add project page's path field. `~` expands, a folder's subfolders list with their git branch,
//! Tab completes the highlighted folder, Enter on a folder browses into it and Enter on the Add row adds.

use super::add_project_rig::{choose, keys, rig, shown, type_in};
use gpui::TestAppContext;

#[gpui::test]
fn typing_a_path_lists_completes_and_browses(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx, |_, _| {});
    rig.open_add(cx);
    type_in("~/code/a", cx);
    let s = shown(cx);
    let app = s.rows.iter().find(|r| r.1 == "app").expect("app listed");
    assert_eq!((app.0.as_str(), app.2.as_str()), ("Folders", "git · main · ~/code/app"));
    assert!(s.rows.iter().any(|r| r.1 == "api" && r.2 == "~/code/api"), "{s:?}");

    // Tab completes the highlighted folder into the field.
    keys("cmd-a backspace", cx);
    type_in("~/co", cx);
    assert_eq!(shown(cx).rows[0].1, "code");
    keys("tab", cx);
    let sep = std::path::MAIN_SEPARATOR;
    assert_eq!(shown(cx).query, format!("~/code{sep}"));

    // Enter on a folder row browses into it; the typed folder itself is the Add row on top.
    let s = shown(cx);
    assert_eq!(s.rows[0].1, "Add ~/code");
    choose("api", cx);
    assert_eq!(shown(cx).query, format!("~/code{sep}api{sep}"));
    assert_eq!(shown(cx).rows[0].1, "Add ~/code/api");

    // Enter on the Add row: an unbound folder asks for its account next.
    keys("enter", cx);
    let s = shown(cx);
    assert_eq!(s.page.as_deref(), Some("Which account does api run on?"));
    assert!(rig.projects(cx).is_empty(), "nothing added before a pick");
}
