//! Scratch threads: the picked account's folder is created, bound to that account on disk and listed as the
//! "Scratch · <account>" project with a new thread open in it. An unknown account binds and stores nothing.

use super::manage_harness::rig;
use super::scratch::NewScratchThreadFor;
use super::threads_test::wait_until;
use super::*;
use gpui::TestAppContext;

#[gpui::test]
fn scratch_thread_binds_its_folder_to_the_picked_account(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    let root =
        r.f.project_dir
            .parent()
            .expect("root")
            .join("data")
            .join("scratch");
    r.ws.update(cx, |ws, _| ws.scratch_root = Some(root.clone()));
    let pick = NewScratchThreadFor {
        account: "acme".into(),
    };
    r.ws.update_in(cx, |ws, window, cx| {
        ws.new_scratch_thread_for(&pick, window, cx)
    });
    wait_until(cx, "scratch project listed", |cx| {
        r.ws.read_with(cx, |ws, _| {
            ws.projects.iter().any(|p| p.name == "Scratch · Acme Team")
        })
    });
    let dir = root.join("acme");
    assert!(dir.is_dir(), "created on first use");
    let on_disk = AccountsConfig::load(&r.f.accounts_path).expect("load");
    assert_eq!(on_disk.resolve(&dir).map(|a| a.id.as_str()), Some("acme"));
    let open = r.ws.read_with(cx, |ws, cx| {
        ws.open_threads.iter().any(|v| v.read(cx).cwd == dir)
    });
    assert!(open, "a new thread is open in the scratch folder");
    // Again: the same project, not a second one.
    r.ws.update_in(cx, |ws, window, cx| {
        ws.new_scratch_thread_for(&pick, window, cx)
    });
    cx.run_until_parked();
    let n = r.ws.read_with(cx, |ws, _| {
        ws.projects.iter().filter(|p| p.path == dir).count()
    });
    assert_eq!(n, 1);
}

#[gpui::test]
fn unknown_account_starts_nothing(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    let root =
        r.f.project_dir
            .parent()
            .expect("root")
            .join("data")
            .join("scratch");
    r.ws.update(cx, |ws, _| ws.scratch_root = Some(root.clone()));
    let before = r.ws.read_with(cx, |ws, _| ws.projects.len());
    let pick = NewScratchThreadFor {
        account: "ghost".into(),
    };
    r.ws.update_in(cx, |ws, window, cx| {
        ws.new_scratch_thread_for(&pick, window, cx)
    });
    cx.run_until_parked();
    assert_eq!(r.ws.read_with(cx, |ws, _| ws.projects.len()), before);
    assert!(!root.join("ghost").exists());
    let on_disk = AccountsConfig::load(&r.f.accounts_path).expect("load");
    assert_eq!(on_disk.bindings.len(), 1, "only the fixture's own binding");
}
