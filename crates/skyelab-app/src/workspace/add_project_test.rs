//! Acceptance: Add project opens a palette page (T3's), never the system folder picker: New project, Clone from URL,
//! Browse… (the picker, only when chosen) and the folders the accounts' agents ran in.

use super::add_project_rig::{dialog_open, rig, shown};
use gpui::TestAppContext;

/// A Claude session of account `id` that ran in `cwd`.
fn history(home: &std::path::Path, id: &str, name: &str, cwd: &std::path::Path) {
    let dir = home.join(format!(".claude-{id}/projects/-x"));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let line = format!("{{\"type\":\"user\",\"cwd\":{:?}}}\n", cwd.to_str().expect("utf8"));
    std::fs::write(dir.join(format!("{name}.jsonl")), line).expect("write");
}

#[gpui::test]
fn add_project_opens_the_palette_page_with_recent_folders(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx, |home, _| {
        std::fs::create_dir_all(home.join("work/hist/.git")).expect("mkdir");
        std::fs::write(home.join("work/hist/.git/HEAD"), "ref: refs/heads/dev\n").expect("head");
        history(home, "home", "s1", &home.join("work/hist"));
        history(home, "home", "s2", &home.join("gone")); // no longer on disk
    });
    rig.open_add(cx);
    assert!(dialog_open(cx), "a palette, not the system picker");
    let s = shown(cx);
    assert_eq!(s.page.as_deref(), Some("Add project"));
    let titles: Vec<_> = s.rows.iter().map(|r| r.1.as_str()).collect();
    assert_eq!(titles, ["New project", "Clone from URL", "Browse…", "hist"]);
    let hist = &s.rows[3];
    assert_eq!(hist.0, "Recent");
    assert_eq!(hist.2, "git · dev · ~/work/hist");
}
