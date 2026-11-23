//! Acceptance: a window's saved terminal tabs reopen with their split tree, sizes, names and folders; a thread's tabs
//! wait until it shows; and every reopened shell gets its folder's account env through the same job as a new one.

use super::terminal_panel::TerminalPanel;
use super::worktree_thread_test::{fixture, open_thread, send, wait_until};
use gpui::{Entity, VisualTestContext};
use skyelab_core::terminal_layout::{Axis, PaneTree, SavedTerminalTab};
use std::path::{Path, PathBuf};

/// Prints the shell's CLAUDE_CONFIG_DIR between markers and returns it.
fn config_dir(panel: &Entity<TerminalPanel>, at: usize, cx: &mut VisualTestContext) -> String {
    let view = panel.read_with(cx, |p, _| p.panes[at].view.clone());
    view.read_with(cx, |v, _| {
        v.terminal()
            .write(b"echo \"CCD=[$CLAUDE_CONFIG_DIR]\"\n".as_slice())
    });
    let mut found = String::new();
    wait_until(cx, "the echo", |cx| {
        // Long values wrap at the grid's width: read the screen as one line.
        let text = view
            .read_with(cx, |v, _| v.terminal().text())
            .replace('\n', "");
        // The typed line shows `[$CLAUDE`; the output line has the value.
        match text
            .split("CCD=[")
            .skip(1)
            .find(|rest| !rest.starts_with('$') && rest.contains(']'))
        {
            Some(rest) => {
                found = rest.split(']').next().unwrap_or_default().to_owned();
                true
            }
            None => false,
        }
    });
    found
}

#[gpui::test]
fn saved_tabs_reopen_with_layout_names_folders_and_the_account_env(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    let (ws, thread, cx) = open_thread(&f, cx);
    send(cx); // a thread gets its row at its first send
    let thread_id = super::delegation_test::thread_id(&thread, cx);
    let web = f.project.join("web");
    std::fs::create_dir_all(&web).expect("mkdir");
    // Not bound to any account.
    let loose = f.project.parent().expect("parent").to_path_buf();

    let mut panes = PaneTree::Leaf(f.project.clone());
    panes.split(&f.project, loose.clone(), Axis::Row);
    panes.split(&loose, web.clone(), Axis::Column);
    panes.resize(&[], 0, 0.7);
    let mine = SavedTerminalTab {
        thread: Some(thread_id),
        name: Some("Dev".into()),
        panes,
    };
    let everywhere = SavedTerminalTab {
        thread: None,
        name: None,
        panes: PaneTree::Leaf(f.project.clone()),
    };
    let elsewhere = SavedTerminalTab {
        thread: Some(thread_id + 1000),
        name: None,
        panes: PaneTree::Leaf(f.project.clone()),
    };
    let gone = SavedTerminalTab {
        thread: None,
        name: Some("Gone".into()),
        panes: PaneTree::Leaf(PathBuf::from("/no/such/folder/anywhere")),
    };
    let saved = vec![mine.clone(), everywhere.clone(), elsewhere.clone(), gone];
    ws.update_in(cx, |ws, window, cx| ws.restore_terminals(saved, window, cx));
    wait_until(cx, "the shown tabs", |cx| {
        ws.read_with(cx, |ws, _| ws.terminals.len() == 2)
    });
    cx.run_until_parked();

    let layouts = ws.read_with(cx, |ws, cx| ws.terminal_layouts(cx));
    assert!(
        layouts.contains(&mine),
        "layout, sizes, name, thread and folders as saved: {layouts:?}"
    );
    assert!(layouts.contains(&everywhere));
    assert_eq!(
        layouts.last(),
        Some(&elsewhere),
        "another thread's tab waits, and is saved again"
    );
    assert_eq!(layouts.len(), 3, "a tab whose folder is gone is dropped");

    let panel = ws.read_with(cx, |ws, cx| {
        ws.terminals
            .iter()
            .find(|p| p.read(cx).name.is_some())
            .cloned()
            .expect("the named tab")
    });
    let owner = panel.read_with(cx, |p, _| p.owner);
    assert_eq!(
        owner,
        Some(thread.entity_id()),
        "it goes with its thread again"
    );

    // Each pane: the folder's account (or none), and the env built for it, never the parent's.
    let accounts: Vec<(PathBuf, Option<String>)> = panel.read_with(cx, |p, _| {
        p.panes
            .iter()
            .map(|p| (p.cwd.clone(), p.account.as_ref().map(|a| a.id.clone())))
            .collect()
    });
    assert_eq!(
        accounts,
        vec![
            (f.project.clone(), Some("acme".into())),
            (loose.clone(), None),
            (web.clone(), Some("acme".into())),
        ]
    );
    let acme = config_dir(&panel, 0, cx);
    assert_eq!(Path::new(&acme), f.account_dir, "the bound account's dir");
    assert_eq!(
        config_dir(&panel, 2, cx),
        acme,
        "a subfolder runs on the same account"
    );
    let unbound = config_dir(&panel, 1, cx);
    let root = super::terminal_launch::unbound_root().expect("data dir");
    assert!(
        Path::new(&unbound).starts_with(&root),
        "an unbound folder gets an empty private home, not the parent's: {unbound}"
    );
    ws.read_with(cx, |ws, cx| {
        for p in &ws.terminals {
            p.read(cx).shutdown(cx);
        }
    });
}
