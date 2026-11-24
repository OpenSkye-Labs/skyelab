//! Going back to Import after importing keeps the repository groups in step with the rows that are left.

use super::onboarding::Step;
use super::onboarding_test::rig;
use super::threads_test::wait_until;
use gpui::TestAppContext;
use std::path::Path;

fn sessions(dir: &Path, cwd: &Path, n: usize) {
    let d = dir.join("projects").join(skyelab_agent::import::claude_dir_name(cwd));
    std::fs::create_dir_all(&d).expect("mkdir");
    for i in 0..n {
        let line = format!("{{\"type\":\"user\",\"cwd\":{:?}}}\n", cwd.to_str().expect("utf8"));
        std::fs::write(d.join(format!("s{i}.jsonl")), line).expect("write");
    }
}

fn repo(path: &Path) {
    std::fs::create_dir_all(path.join(".git")).expect("mkdir");
    std::fs::write(path.join(".git/HEAD"), "ref: refs/heads/main\n").expect("head");
}

#[gpui::test]
fn back_from_done_keeps_import_groups_in_step_with_rows(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx);
    let (a, b) = (rig.home.join("work/a"), rig.home.join("work/b"));
    repo(&a);
    repo(&b);
    let mine = rig.home.join(".claude");
    sessions(&mine, &a, 3); // ticked
    sessions(&mine, &b, 1); // not ticked
    rig.add("claude", cx);
    cx.run_until_parked();
    rig.setup.update(cx, |o, cx| o.go(Step::Import, cx));
    let setup = rig.setup.clone();
    wait_until(cx, "the scan", |cx| {
        setup.read_with(cx, |o, _| o.import.rows.len() == 2 && !o.import.scanning)
    });
    rig.setup.update(cx, |o, cx| o.next(cx));
    let ws = rig.ws.clone();
    wait_until(cx, "imported", |cx| ws.read_with(cx, |ws, _| ws.projects.len() == 1));
    rig.setup.update(cx, |o, cx| o.back(cx));
    cx.run_until_parked();
    let ok = rig.setup.read_with(cx, |o, _| {
        let n = o.import.rows.len();
        o.import
            .groups
            .iter()
            .flat_map(|g| g.rows.iter())
            .chain(o.import.other.iter())
            .all(|&i| i < n)
    });
    assert!(ok, "stale group index after import");
}
