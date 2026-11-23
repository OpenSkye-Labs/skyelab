//! The mod+P picker lists tracked and untracked-but-not-ignored files, Enter previews, mod+Enter goes to the editor;
//! the Search tab finds text and opens the preview at the matched line.

use super::super::worktree_thread_test::{Fixture, fixture, open_thread, wait_until};
use super::super::*;
use super::picker::{self, OnPick, rank_files};
use gpui::{TestAppContext, VisualTestContext};
use gpui_component::Root;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

const EDITOR_KEY: &str = if cfg!(target_os = "macos") {
    "cmd-enter"
} else {
    "ctrl-enter"
};

fn write_files(project: &std::path::Path) {
    std::fs::write(project.join(".gitignore"), "secret.env\n").expect("write");
    std::fs::write(project.join("secret.env"), "TOKEN=x\n").expect("write");
    std::fs::create_dir_all(project.join("src")).expect("mkdir");
    std::fs::write(
        project.join("src/new.rs"),
        "fn one() {}\nfn two() {}\nfn needle() {}\n",
    )
    .expect("write");
}

/// Like `open_thread`, in a `Root` window: dialogs need one.
fn open_in_root<'a>(
    f: &Fixture,
    cx: &'a mut TestAppContext,
) -> (Entity<Workspace>, &'a mut VisualTestContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::settings::init(cx);
        skyelab_terminal::init(cx);
        init(cx);
    });
    let backend = Backend {
        store: f.store.clone(),
        claude_bin: Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh"),
    };
    let (config, folder) = (f.config.clone(), f.project.clone());
    let mut ws = None;
    let window = cx.add_window(|window, cx| {
        let view = cx.new(|cx| Workspace::new(folder, None, config, None, backend, window, cx));
        let focus = view.read(cx).focus.clone();
        window.focus(&focus, cx);
        ws = Some(view.clone());
        Root::new(view, window, cx)
    });
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    let ws = ws.expect("workspace built");
    let dir = f.project.clone();
    ws.update(cx, |ws, cx| ws.add_project(dir, cx));
    wait_until(cx, "project stored", |cx| {
        ws.read_with(cx, |ws, _| ws.project.is_some())
    });
    cx.dispatch_action(NewThread);
    cx.run_until_parked();
    (ws, cx)
}

#[test]
fn rank_prefers_file_names_and_caps() {
    let files: Vec<String> = ["src/news/a.rs", "src/new.rs", "docs/renew.md", "Cargo.toml"]
        .map(String::from)
        .into();
    let ranked = rank_files("new", &files, 10);
    assert_eq!(files[ranked[0]], "src/new.rs");
    assert!(!ranked.iter().any(|&i| files[i] == "Cargo.toml"));
    assert_eq!(rank_files("", &files, 2), [0, 1]);
    assert_eq!(rank_files("@src/new", &files, 1).len(), 1);
}

fn open_picker(
    root: PathBuf,
    picked: &Rc<RefCell<Vec<(PathBuf, bool)>>>,
    cx: &mut VisualTestContext,
) -> gpui::Entity<picker::FilePicker> {
    let log = picked.clone();
    let on_pick: OnPick = Rc::new(move |path, editor, _, _| log.borrow_mut().push((path, editor)));
    let picker = cx.update(|window, cx| {
        let prev = window.focused(cx).expect("focused");
        picker::open(root, prev, on_pick, window, cx)
    });
    wait_until(cx, "files listed", |cx| {
        picker.read_with(cx, |p, _| !p.files.is_empty())
    });
    picker
}

#[gpui::test]
fn file_picker_lists_and_picks(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    write_files(&f.project);
    let (_ws, cx) = open_in_root(&f, cx);
    let picked = Rc::new(RefCell::new(Vec::new()));

    let picker = open_picker(f.project.clone(), &picked, cx);
    let files = picker.read_with(cx, |p, _| p.files.to_vec());
    assert_eq!(files, [".gitignore", "README.md", "src/new.rs"]);

    cx.simulate_keystrokes("n e w");
    wait_until(cx, "ranked", |cx| {
        picker.read_with(cx, |p, _| p.shown.first().map(|&i| p.files[i].clone()))
            == Some("src/new.rs".into())
    });
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(*picked.borrow(), [(PathBuf::from("src/new.rs"), false)]);

    let picker = open_picker(f.project.clone(), &picked, cx);
    cx.simulate_keystrokes("r e a d");
    wait_until(cx, "ranked", |cx| {
        picker.read_with(cx, |p, _| p.shown.first().map(|&i| p.files[i].clone()))
            == Some("README.md".into())
    });
    cx.simulate_keystrokes(EDITOR_KEY);
    cx.run_until_parked();
    assert_eq!(
        picked.borrow().last(),
        Some(&(PathBuf::from("README.md"), true))
    );
}

#[gpui::test]
fn picker_enter_opens_the_preview(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    write_files(&f.project);
    let (ws, cx) = open_in_root(&f, cx);
    cx.simulate_keystrokes(FILE_PICKER_KEY);
    cx.simulate_keystrokes("n e w . r s");
    // Enter does nothing until the list has loaded and ranked, so press it until the preview opens.
    wait_until(cx, "the preview tab", |cx| {
        cx.simulate_keystrokes("enter");
        ws.read_with(cx, |ws, _| {
            ws.files.preview.as_ref().and_then(|p| p.upgrade())
        })
        .is_some()
    });
    let tab = ws
        .read_with(cx, |ws, _| {
            ws.files.preview.as_ref().and_then(|p| p.upgrade())
        })
        .expect("preview");
    wait_until(cx, "the file's text", |cx| {
        tab.read_with(cx, |t, cx| t.editor.read(cx).value().contains("fn needle"))
    });
    assert_eq!(
        tab.read_with(cx, |t, _| t.path.clone()),
        PathBuf::from("src/new.rs")
    );
}

#[gpui::test]
fn search_tab_opens_preview_at_line(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    write_files(&f.project);
    let (ws, _thread, cx) = open_thread(&f, cx);
    cx.simulate_keystrokes(SEARCH_FILES_KEY);
    cx.run_until_parked();
    let search = ws
        .read_with(cx, |ws, _| {
            ws.files.search.as_ref().and_then(|s| s.upgrade())
        })
        .expect("search tab");
    cx.simulate_keystrokes("n e e d l e");
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(200)); // past the typing pause
    wait_until(cx, "results", |cx| {
        search.read_with(cx, |s, _| {
            s.found.as_ref().is_some_and(|f| !f.files.is_empty())
        })
    });
    let found = search.read_with(cx, |s, _| s.found.clone()).expect("found");
    assert_eq!(found.files.len(), 1);
    assert_eq!(found.files[0].path, "src/new.rs");
    assert_eq!(found.files[0].lines[0].line, 3);

    search.update(cx, |s, cx| s.open_match(0, 3, cx));
    let tab = ws
        .read_with(cx, |ws, _| {
            ws.files.preview.as_ref().and_then(|p| p.upgrade())
        })
        .expect("preview");
    wait_until(cx, "the file at line 3", |cx| {
        tab.read_with(cx, |t, cx| {
            let e = t.editor.read(cx);
            e.value().contains("fn needle") && e.cursor_position().line == 2
        })
    });
}

#[gpui::test]
fn file_picker_key_stays_the_shells_in_a_terminal_off_macos(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        init(cx);
    });
    let unbound = cx.update(|cx| {
        cx.key_bindings().borrow().bindings().any(|b| {
            b.action().name() == "zed::NoAction"
                && b.predicate().is_some_and(|p| p.to_string() == "Terminal")
                && b.keystrokes()
                    .iter()
                    .map(|k| k.unparse(gpui::OperatingSystem::Linux))
                    .collect::<Vec<_>>()
                    == [FILE_PICKER_KEY]
        })
    });
    // ctrl-p is readline's previous command; cmd-p has no shell meaning.
    assert_eq!(unbound, !cfg!(target_os = "macos"));
}
